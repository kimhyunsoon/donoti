import { db } from '../db.js';
import { enqueueNotification } from '../notify.js';
import { crawlLog } from '../crawl-log.js';
import type { WatchRow } from './index.js';

// 수집 재시도 간 대기 (1차 실패 후 1초, 2차 실패 후 2초)
const RETRY_DELAYS = [1_000, 2_000];

/**
 * 수집 호출을 최대 3회 시도한다. 중간 실패는 일자별 로그에 남긴다.
 * @param label 로그 표기용 이름 (예: 'CGV 상영표 0013 20260821')
 * @param fn 시도 함수 - attempt(0부터)를 받아 시도별 전략(쿠키 갱신 등)을 쓸 수 있다
 */
export async function withRetry<T>(label: string, fn: (attempt: number) => Promise<T>): Promise<T> {
  let lastError: unknown;
  for (let attempt = 0; attempt < 3; attempt++) {
    try {
      return await fn(attempt);
    } catch (err) {
      lastError = err;
      const message = err instanceof Error ? err.message : String(err);
      // 4xx는 재시도해도 같은 결과 (미취급 상품 등 정상 흐름) - 즉시 중단
      if (/_http_4\d\d/.test(message)) throw err;
      crawlLog(`시도 ${attempt + 1}/3 실패: ${label} - ${message}`);
      if (attempt < 2) await new Promise((resolve) => setTimeout(resolve, RETRY_DELAYS[attempt]));
    }
  }
  throw lastError;
}

interface FailState {
  failing?: boolean;
}

/**
 * 최종 수집 실패 처리: 일자별 로그 기록 + 실패 알림 발송.
 * 연속 실패는 최초 1회만 알림 (watch.state.failing으로 억제, 복구되면 리셋)
 */
export function reportCrawlFailure(watch: WatchRow, message: string): void {
  crawlLog(`수집 실패: watch ${watch.id} "${watch.name}" (${watch.provider}) - ${message}`);
  const state = JSON.parse(watch.state ?? '{}') as FailState;
  if (state.failing) return;
  state.failing = true;
  watch.state = JSON.stringify(state);
  db.prepare('UPDATE watches SET state = ? WHERE id = ?').run(watch.state, watch.id);
  enqueueNotification({
    source: watch.provider,
    title: `⚠️ 수집 실패 · ${watch.name}`,
    body: `${message}\n복구되면 알림이 재개돼요`,
    watchId: watch.id,
  });
}

/** 수집 성공 시 실패 상태 해제 (다음 실패 때 다시 알림이 가도록) */
export function clearCrawlFailure(watch: WatchRow): void {
  const state = JSON.parse(watch.state ?? '{}') as FailState;
  if (!state.failing) return;
  delete state.failing;
  watch.state = JSON.stringify(state);
  db.prepare('UPDATE watches SET state = ? WHERE id = ?').run(watch.state, watch.id);
}
