import webpush from 'web-push';
import { db } from './db.js';
import { config } from './config.js';
import { groupNotificationTitle } from './catalog.js';

const MAX_ATTEMPTS = 5;
// 재시도 백오프: 10초 → 1분 → 5분 → 15분
const BACKOFF_MS = [10_000, 60_000, 300_000, 900_000];
const POLL_MS = 5_000;
// 같은 회차에 쌓인 같은 종류(source)는 브라우저 알림 하나로 합친다 - 넘치면 나눠서 발송
const GROUP_MAX = 4;
// 합쳐진 본문의 항목당 최대 길이 (넘으면 말줄임)
const GROUP_LINE_MAX = 50;
// 발송 완료 알림 보관 기간 (failed는 수동 확인용이라 자동 삭제하지 않음)
const SENT_RETENTION = "-30 days";

export interface NotificationInput {
  // 넣는 주체 (크롤러·잡 이름)
  source: string;
  title: string;
  body?: string;
  // 알림 클릭 시 이동 경로
  url?: string;
  // 이 알림을 만든 watch (홈 목록 '최근 알림' 표기용)
  watchId?: number;
}

interface NotificationRow {
  id: number;
  source: string;
  title: string;
  body: string;
  url: string | null;
  attempts: number;
}

interface SubRow {
  id: number;
  endpoint: string;
  keys_p256dh: string;
  keys_auth: string;
}

/**
 * 알림 큐에 등록한다. 발송은 워커가 담당.
 * @returns 생성된 알림 id
 */
export function enqueueNotification(input: NotificationInput): number {
  const result = db
    .prepare(
      `INSERT INTO notifications (source, title, body, url, watch_id, next_attempt_at)
       VALUES (?, ?, ?, ?, ?, ?)`,
    )
    .run(input.source, input.title, input.body ?? '', input.url ?? null, input.watchId ?? null, Date.now());
  return Number(result.lastInsertRowid);
}

// 합쳐진 알림의 항목 한 줄: 개별 제목(이모지 제외) + 본문 첫 줄
function summaryLine(row: NotificationRow): string {
  const title = row.title.replace(/^[\p{Extended_Pictographic}\u{FE0F}\u{200D}]+\s*/u, '');
  const firstBody = row.body.split('\n')[0]?.trim() ?? '';
  const line = firstBody ? `${title} · ${firstBody}` : title;
  return line.length > GROUP_LINE_MAX ? `${line.slice(0, GROUP_LINE_MAX - 1)}…` : line;
}

// 브라우저 알림 페이로드. 2건 이상이면 제목·본문을 합치고 ids로 일괄 읽음 처리를 지원한다
function buildPayload(rows: NotificationRow[], unread: number): string {
  if (rows.length === 1) {
    const row = rows[0]!;
    return JSON.stringify({
      // id: OS 알림 클릭 시 서비스워커가 해당 알림을 읽음 처리하는 데 사용
      id: row.id,
      title: row.title,
      body: row.body,
      url: row.url ?? '/',
      unread,
    });
  }
  return JSON.stringify({
    ids: rows.map((r) => r.id),
    title: groupNotificationTitle(rows[0]!.source, rows.length),
    body: rows.map(summaryLine).join('\n'),
    url: '/',
    unread,
  });
}

// 모든 구독 기기로 Web Push 발송 (같은 종류 묶음은 알림 1개로 합쳐서). 만료 구독(404/410)은
// 정리하고 전달 수를 반환. 구독이 있는데 전부 실패하면 throw → 큐 재시도로 이어진다
async function broadcast(rows: NotificationRow[], log: (msg: string) => void): Promise<number> {
  const subs = db.prepare('SELECT * FROM push_subscriptions').all() as SubRow[];
  if (subs.length === 0) return 0;

  // unread = 앱 아이콘 배지 카운트 (이번 묶음 포함) - 서비스워커가 setAppBadge에 사용
  const prev = db
    .prepare(`SELECT COUNT(*) AS c FROM notifications WHERE status = 'sent' AND read_at IS NULL`)
    .get() as { c: number };
  const payload = buildPayload(rows, prev.c + rows.length);
  let delivered = 0;
  let lastError = '';
  for (const sub of subs) {
    try {
      await webpush.sendNotification(
        { endpoint: sub.endpoint, keys: { p256dh: sub.keys_p256dh, auth: sub.keys_auth } },
        payload,
      );
      delivered += 1;
    } catch (err) {
      const status = (err as { statusCode?: number }).statusCode;
      // 만료된 구독은 정리 (실패로 치지 않음)
      if (status === 404 || status === 410) {
        db.prepare('DELETE FROM push_subscriptions WHERE id = ?').run(sub.id);
      } else {
        lastError = err instanceof Error ? err.message : String(err);
        log(`푸시 발송 실패 (구독 ${sub.id}): ${lastError}`);
      }
    }
  }
  const remaining = db.prepare('SELECT COUNT(*) AS c FROM push_subscriptions').get() as { c: number };
  if (delivered === 0 && remaining.c > 0) {
    throw new Error(lastError || 'push_failed');
  }
  return delivered;
}

/**
 * 알림 큐 워커를 시작한다. pending을 주기적으로 FIFO 소비해 Web Push로 발송.
 * @param log 진행 로그 출력 함수 (app.log.info 등)
 * @returns 워커 중지 함수
 */
export function startNotifier(log: (msg: string) => void): () => void {
  webpush.setVapidDetails(config.vapid.subject, config.vapid.publicKey, config.vapid.privateKey);

  // 발송 중 크래시 잔재 복구 (재발송될 수 있으나 개인 알림 용도로 허용)
  db.prepare(`UPDATE notifications SET status = 'pending' WHERE status = 'sending'`).run();

  let busy = false;
  let lastCleanup = 0;

  const tick = async (): Promise<void> => {
    if (busy) return;
    busy = true;
    try {
      const rows = db
        .prepare(
          `SELECT id, source, title, body, url, attempts FROM notifications
           WHERE status = 'pending' AND next_attempt_at <= ?
           ORDER BY id LIMIT 10`,
        )
        .all(Date.now()) as NotificationRow[];

      // 같은 종류(source)끼리 묶는다 - 알림센터 행은 그대로 두고 브라우저 알림만 합쳐진다
      const groups = new Map<string, NotificationRow[]>();
      for (const row of rows) groups.set(row.source, [...(groups.get(row.source) ?? []), row]);

      for (const group of groups.values()) {
        // 너무 길어지지 않게 GROUP_MAX건씩 나눠서 발송
        for (let i = 0; i < group.length; i += GROUP_MAX) {
          const chunk = group.slice(i, i + GROUP_MAX);
          for (const row of chunk) {
            db.prepare(`UPDATE notifications SET status = 'sending' WHERE id = ?`).run(row.id);
          }
          const label =
            chunk.length === 1 ? `"${chunk[0]!.title}"` : `${chunk.length}건 (합쳐서 발송)`;
          try {
            const delivered = await broadcast(chunk, log);
            for (const row of chunk) {
              db.prepare(
                `UPDATE notifications
                 SET status = 'sent', delivered = ?, sent_at = datetime('now'), last_error = NULL
                 WHERE id = ?`,
              ).run(delivered, row.id);
            }
            log(`알림 발송 완료: ${chunk[0]!.source} ${label} (${delivered}기기)`);
          } catch (err) {
            const message = err instanceof Error ? err.message : String(err);
            for (const row of chunk) {
              const attempts = row.attempts + 1;
              if (attempts >= MAX_ATTEMPTS) {
                db.prepare(
                  `UPDATE notifications SET status = 'failed', attempts = ?, last_error = ? WHERE id = ?`,
                ).run(attempts, message, row.id);
              } else {
                const backoff = BACKOFF_MS[Math.min(attempts - 1, BACKOFF_MS.length - 1)]!;
                db.prepare(
                  `UPDATE notifications
                   SET status = 'pending', attempts = ?, next_attempt_at = ?, last_error = ?
                   WHERE id = ?`,
                ).run(attempts, Date.now() + backoff, message, row.id);
              }
            }
            const finals = chunk.filter((r) => r.attempts + 1 >= MAX_ATTEMPTS).length;
            log(
              finals === chunk.length
                ? `알림 최종 실패: ${chunk[0]!.source} ${label} - ${message}`
                : `알림 발송 실패: ${chunk[0]!.source} ${label} - ${message} (재시도 예약)`,
            );
          }
        }
      }

      // 오래된 발송 완료 알림 정리 (시간당 1회)
      if (Date.now() - lastCleanup > 3_600_000) {
        lastCleanup = Date.now();
        db.prepare(`DELETE FROM notifications WHERE status = 'sent' AND sent_at < datetime('now', ?)`).run(
          SENT_RETENTION,
        );
      }
    } finally {
      busy = false;
    }
  };

  const timer = setInterval(() => void tick(), POLL_MS);
  void tick();
  log(`알림 워커 시작 (폴링 ${POLL_MS / 1000}초)`);
  return (): void => clearInterval(timer);
}
