import { fxCrawler } from './fx.js';
import { stockCrawler } from './stock.js';
import { emartCrawler } from './emart.js';
import { nintendoCrawler } from './nintendo.js';
import { cgvCrawler } from './cgv.js';
import { lotteCrawler } from './lotte.js';
import { megaboxCrawler } from './megabox.js';

// 크롤러 레지스트리 - 각 provider는 서로 다른 프로그램처럼 개별 구현해 여기에 등록한다

// watches 테이블 행
export interface WatchRow {
  id: number;
  category: string;
  provider: string;
  name: string;
  url: string | null;
  schedule: string; // JSON ScheduleRule[]
  config: string; // JSON (provider별 폼 데이터)
  enabled: number; // 0 = 임시 중지
  state: string | null; // 크롤러 전용 상태 JSON
  ends_at: string | null;
  deleted_at: string | null;
  created_at: string;
  updated_at: string;
}

/**
 * provider별 크롤러. 같은 시각에 매칭된 같은 provider의 watch들은
 * 배열로 묶여 한 번에 처리된다 (알림당 1크롤링이 아니라 종류당 1크롤링).
 */
export interface Crawler {
  /**
   * 배치 실행. 감지 시 watch별 enqueueNotification() 호출과
   * 필요하면 watch.url(리다이렉트 URL) 자동 설정이 구현의 책무.
   * @param watches 이번 회차에 매칭된 같은 provider의 watch들
   * @param log 진행 로그 출력 함수
   */
  run: (watches: WatchRow[], log: (msg: string) => void) => Promise<void>;
  /**
   * 등록·수정 시 config 검증
   * @param config provider별 폼 데이터
   * @param excludeId 수정 중인 watch id (중복 검사에서 제외, 신규면 null)
   * @returns 문제 없으면 null, 있으면 오류 메시지
   */
  validateConfig?: (config: Record<string, unknown>, excludeId: number | null) => string | null;
}

// provider → 크롤러 (전 카탈로그 구현 완료)
const registry: Record<string, Crawler> = {
  fx: fxCrawler,
  stock: stockCrawler,
  emart: emartCrawler,
  nintendo: nintendoCrawler,
  cgv: cgvCrawler,
  lotte: lotteCrawler,
  megabox: megaboxCrawler,
};

export function getCrawler(provider: string): Crawler | undefined {
  return registry[provider];
}
