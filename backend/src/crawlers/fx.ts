import { db } from '../db.js';
import { enqueueNotification } from '../notify.js';
import { notificationTitle } from '../catalog.js';
import { withRetry, reportCrawlFailure, clearCrawlFailure } from './retry.js';
import type { Crawler, WatchRow } from './index.js';

// 환율 크롤러 - 네이버 금융 front-api (하나은행 고시 환율, 장중 수시 갱신)

// 지원 통화 (frontend/src/catalog.ts FX_CURRENCIES와 동기 유지)
// per: 고시 기준 단위 (엔·루피아는 100 단위로 고시됨)
const CURRENCIES: Record<string, { label: string; reuters: string; per: number }> = {
  USD: { label: '달러', reuters: 'FX_USDKRW', per: 1 },
  JPY: { label: '엔', reuters: 'FX_JPYKRW', per: 100 },
  EUR: { label: '유로', reuters: 'FX_EURKRW', per: 1 },
  CNY: { label: '위안', reuters: 'FX_CNYKRW', per: 1 },
  IDR: { label: '루피아', reuters: 'FX_IDRKRW', per: 100 },
  TWD: { label: '대만달러', reuters: 'FX_TWDKRW', per: 1 },
  HKD: { label: '홍콩달러', reuters: 'FX_HKDKRW', per: 1 },
};

interface NaverQuote {
  isSuccess: boolean;
  result?: {
    closePrice: string; // '1,381.20'
    fluctuationsRatio: string; // '-0.98'
  };
}

// 알림 클릭 시 이동할 네이버 환율 상세 (모바일)
function detailUrl(reuters: string): string {
  return `https://m.stock.naver.com/marketindex/exchange/${reuters}`;
}

async function fetchQuote(reuters: string): Promise<{ price: string; ratio: number }> {
  return withRetry(`환율 ${reuters}`, async () => {
    const res = await fetch(
      `https://m.stock.naver.com/front-api/marketIndex/productDetail?category=exchange&reutersCode=${reuters}`,
      { headers: { 'User-Agent': 'Mozilla/5.0' }, signal: AbortSignal.timeout(10_000) },
    );
    if (!res.ok) throw new Error(`naver_http_${res.status}`);
    const data = (await res.json()) as NaverQuote;
    if (!data.isSuccess || !data.result?.closePrice) throw new Error('naver_bad_response');
    return { price: data.result.closePrice, ratio: Number(data.result.fluctuationsRatio) };
  });
}

export const fxCrawler: Crawler = {
  // 같은 회차의 환율 알림들을 묶어 통화별 1회만 조회한다
  async run(watches: WatchRow[], log: (msg: string) => void): Promise<void> {
    // 통화 → 해당 watch 목록
    const byCurrency = new Map<string, WatchRow[]>();
    for (const watch of watches) {
      const currency = (JSON.parse(watch.config) as { currency?: string }).currency ?? '';
      if (!CURRENCIES[currency]) {
        log(`환율 알림 통화 설정 없음: watch ${watch.id} "${watch.name}" - 건너뜀`);
        continue;
      }
      byCurrency.set(currency, [...(byCurrency.get(currency) ?? []), watch]);
    }

    await Promise.allSettled(
      [...byCurrency.entries()].map(async ([currency, group]) => {
        const info = CURRENCIES[currency]!;
        let quote: { price: string; ratio: number };
        try {
          quote = await fetchQuote(info.reuters);
        } catch (err) {
          const message = err instanceof Error ? err.message : String(err);
          log(`환율 조회 실패 (${currency}): ${message}`);
          for (const watch of group) reportCrawlFailure(watch, `환율 조회 실패 (${currency}): ${message}`);
          return;
        }
        for (const watch of group) clearCrawlFailure(watch);
        const ratioText = quote.ratio > 0 ? `+${quote.ratio}` : `${quote.ratio}`;
        const body = `${quote.price}원\n전일 대비 ${ratioText}%`;
        const url = detailUrl(info.reuters);
        for (const watch of group) {
          // 리다이렉트 URL 자동 설정 (사용자가 직접 넣지 않으므로 비어있을 때 채움)
          if (!watch.url) {
            db.prepare('UPDATE watches SET url = ? WHERE id = ? AND url IS NULL').run(url, watch.id);
          }
          enqueueNotification({
            source: 'fx',
            title: notificationTitle('fx', watch.name),
            body,
            url: watch.url ?? url,
            watchId: watch.id,
          });
        }
        log(`환율 조회 완료: ${currency} ${quote.price}원 (알림 ${group.length}건)`);
      }),
    );
  },

  validateConfig(config: Record<string, unknown>, excludeId: number | null): string | null {
    const currency = config.currency;
    if (typeof currency !== 'string' || !CURRENCIES[currency]) return '통화를 선택해 주세요';
    // 1통화 1알림 - 활성 목록에 같은 통화가 이미 있으면 거부
    const dup = db
      .prepare(
        `SELECT id FROM watches
         WHERE provider = 'fx' AND deleted_at IS NULL AND id != ?
           AND json_extract(config, '$.currency') = ?`,
      )
      .get(excludeId ?? -1, currency);
    if (dup) return `${CURRENCIES[currency]!.label} 환율 알림이 이미 있어요`;
    return null;
  },
};
