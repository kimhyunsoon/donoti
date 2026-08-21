import webpush from 'web-push';
import { db } from './db.js';
import { config } from './config.js';

const MAX_ATTEMPTS = 5;
// 재시도 백오프: 10초 → 1분 → 5분 → 15분
const BACKOFF_MS = [10_000, 60_000, 300_000, 900_000];
const POLL_MS = 5_000;
// 발송 완료 알림 보관 기간 (failed는 수동 확인용이라 자동 삭제하지 않음)
const SENT_RETENTION = "-30 days";

export interface NotificationInput {
  // 넣는 주체 (크롤러·잡 이름)
  source: string;
  title: string;
  body?: string;
  // 알림 클릭 시 이동 경로
  url?: string;
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
      `INSERT INTO notifications (source, title, body, url, next_attempt_at)
       VALUES (?, ?, ?, ?, ?)`,
    )
    .run(input.source, input.title, input.body ?? '', input.url ?? null, Date.now());
  return Number(result.lastInsertRowid);
}

// 모든 구독 기기로 Web Push 발송. 만료 구독(404/410)은 정리하고 전달 수를 반환.
// 구독이 있는데 전부 실패하면 throw → 큐 재시도로 이어진다
async function broadcast(row: NotificationRow, log: (msg: string) => void): Promise<number> {
  const subs = db.prepare('SELECT * FROM push_subscriptions').all() as SubRow[];
  if (subs.length === 0) return 0;

  // unread = 앱 아이콘 배지 카운트 (이번 알림 포함) - 서비스워커가 setAppBadge에 사용
  const prev = db
    .prepare(`SELECT COUNT(*) AS c FROM notifications WHERE status = 'sent' AND read_at IS NULL`)
    .get() as { c: number };
  const payload = JSON.stringify({
    title: row.title,
    body: row.body,
    url: row.url ?? '/',
    unread: prev.c + 1,
  });
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

      for (const row of rows) {
        db.prepare(`UPDATE notifications SET status = 'sending' WHERE id = ?`).run(row.id);
        try {
          const delivered = await broadcast(row, log);
          db.prepare(
            `UPDATE notifications
             SET status = 'sent', delivered = ?, sent_at = datetime('now'), last_error = NULL
             WHERE id = ?`,
          ).run(delivered, row.id);
          log(`알림 발송 완료: ${row.source} "${row.title}" (${delivered}기기)`);
        } catch (err) {
          const attempts = row.attempts + 1;
          const message = err instanceof Error ? err.message : String(err);
          if (attempts >= MAX_ATTEMPTS) {
            db.prepare(
              `UPDATE notifications SET status = 'failed', attempts = ?, last_error = ? WHERE id = ?`,
            ).run(attempts, message, row.id);
            log(`알림 최종 실패: ${row.source} "${row.title}" - ${message}`);
          } else {
            const backoff = BACKOFF_MS[Math.min(attempts - 1, BACKOFF_MS.length - 1)]!;
            db.prepare(
              `UPDATE notifications
               SET status = 'pending', attempts = ?, next_attempt_at = ?, last_error = ?
               WHERE id = ?`,
            ).run(attempts, Date.now() + backoff, message, row.id);
            log(`알림 발송 실패 (${attempts}/${MAX_ATTEMPTS}), ${backoff / 1000}초 후 재시도: ${message}`);
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
