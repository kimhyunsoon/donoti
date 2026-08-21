import { appendFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { config } from './config.js';

const dir = join(config.dataDir, 'logs');
mkdirSync(dir, { recursive: true });

/** 수집(크롤링) 로그를 일자별(KST) 파일에 남긴다: {DATA_DIR}/logs/crawl-YYYY-MM-DD.log */
export function crawlLog(message: string): void {
  // KST = UTC+9 고정
  const stamp = new Date(Date.now() + 9 * 3_600_000).toISOString().slice(0, 19).replace('T', ' ');
  try {
    appendFileSync(join(dir, `crawl-${stamp.slice(0, 10)}.log`), `[${stamp}] ${message}\n`);
  } catch {
    // 로그 기록 실패는 수집을 막지 않는다
  }
}
