import { db } from './db.js';
import { kstNow, matchesSchedule, type ScheduleRule } from './watch-schedule.js';
import { getCrawler, type WatchRow } from './crawlers/index.js';

// 동시 실행 방지 - 이전 배치가 아직 안 끝난 provider는 이번 틱을 건너뛴다
const running = new Set<string>();

function tick(log: (msg: string) => void): void {
  const now = kstNow();

  // 종료 시점이 지난 알림은 목록에서 제거 (soft delete - 종료된 알림 페이지에서 복구 가능)
  const expired = db
    .prepare(
      `UPDATE watches SET deleted_at = datetime('now'), updated_at = datetime('now')
       WHERE deleted_at IS NULL AND ends_at IS NOT NULL AND ends_at <= ?`,
    )
    .run(now.stamp);
  if (expired.changes > 0) log(`종료 시점이 지난 알림 ${expired.changes}건 정리`);

  // 임시 중지(enabled=0)는 크롤링 대상에서 제외
  const watches = db
    .prepare('SELECT * FROM watches WHERE deleted_at IS NULL AND enabled = 1')
    .all() as WatchRow[];

  // 같은 시각에 매칭된 watch는 provider별로 묶어 배치 1회로 처리한다
  const matched = new Map<string, WatchRow[]>();
  for (const watch of watches) {
    let rules: ScheduleRule[];
    try {
      rules = JSON.parse(watch.schedule) as ScheduleRule[];
    } catch {
      continue;
    }
    if (!matchesSchedule(rules, now.day, now.hhmm)) continue;
    matched.set(watch.provider, [...(matched.get(watch.provider) ?? []), watch]);
  }

  for (const [provider, group] of matched) {
    const crawler = getCrawler(provider);
    if (!crawler) {
      log(`크롤러 미구현: ${provider} (알림 ${group.length}건) - 건너뜀`);
      continue;
    }
    if (running.has(provider)) {
      log(`이전 크롤링 진행 중: ${provider} - 이번 회차 건너뜀`);
      continue;
    }
    running.add(provider);
    crawler
      .run(group, log)
      .catch((err) => {
        const message = err instanceof Error ? err.message : String(err);
        log(`크롤링 실패 (${provider}, 알림 ${group.length}건): ${message}`);
      })
      .finally(() => running.delete(provider));
  }
}

/**
 * 크롤링 스케줄러를 시작한다. 매분 0초(KST)에 스케줄을 평가해 일치하는 크롤러를 실행.
 * @param log 진행 로그 출력 함수 (app.log.info 등)
 * @returns 스케줄러 중지 함수
 */
export function startScheduler(log: (msg: string) => void): () => void {
  let timer: ReturnType<typeof setTimeout>;
  const arm = (): void => {
    // 매 틱 다음 분 0초에 재정렬 (setInterval 드리프트 누적 방지)
    timer = setTimeout(() => {
      try {
        tick(log);
      } catch (err) {
        log(`스케줄러 틱 오류: ${err instanceof Error ? err.message : String(err)}`);
      }
      arm();
    }, 60_000 - (Date.now() % 60_000));
  };
  arm();
  log('크롤링 스케줄러 시작 (매분 평가, KST)');
  return (): void => clearTimeout(timer);
}
