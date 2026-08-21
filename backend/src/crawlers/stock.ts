import { db } from '../db.js';
import { enqueueNotification } from '../notify.js';
import { notificationTitle } from '../catalog.js';
import { withRetry, reportCrawlFailure, clearCrawlFailure } from './retry.js';
import type { StockSymbol } from './stock-symbols.js';
import type { Crawler, WatchRow } from './index.js';

// 주식 크롤러 - 네이버 증권 시세 API (국내 m.stock / 해외 api.stock)
// 종목은 stock_symbols 마스터로 관리, watch.config = { code }

interface NaverBasic {
  closePrice?: string; // '278,500'
  fluctuationsRatio?: string; // '2.77'
}

// 시세 API 주소 (market·kind에 따라 도메인·경로가 다르다)
function quoteUrl(s: StockSymbol): string {
  const host = s.market === 'domestic' ? 'https://m.stock.naver.com/api' : 'https://api.stock.naver.com';
  return `${host}/${s.kind === 'index' ? 'index' : 'stock'}/${s.code}/basic`;
}

// 알림 클릭 시 이동할 네이버 증권 상세 (모바일)
function detailUrl(s: StockSymbol): string {
  const section = s.market === 'domestic' ? 'domestic' : 'worldstock';
  return `https://m.stock.naver.com/${section}/${s.kind === 'index' ? 'index' : 'stock'}/${s.code}/total`;
}

// 시세 표기: 국내 종목·ETF '278,500원' / 해외 종목·ETF '$311.30' / 지수 '6,890.01'
function formatPrice(s: StockSymbol, price: string): string {
  if (s.kind === 'index') return price;
  return s.market === 'domestic' ? `${price}원` : `$${price}`;
}

async function fetchQuote(s: StockSymbol): Promise<{ price: string; ratio: number }> {
  return withRetry(`주식 ${s.code}`, async () => {
    const res = await fetch(quoteUrl(s), {
      headers: { 'User-Agent': 'Mozilla/5.0' },
      signal: AbortSignal.timeout(10_000),
    });
    if (!res.ok) throw new Error(`naver_http_${res.status} (${s.code})`);
    const data = (await res.json()) as NaverBasic;
    if (!data.closePrice) throw new Error(`naver_bad_response (${s.code})`);
    return { price: data.closePrice, ratio: Number(data.fluctuationsRatio ?? 0) };
  });
}

function findSymbol(watch: WatchRow): StockSymbol | null {
  const code = (JSON.parse(watch.config) as { code?: string }).code ?? '';
  return (db.prepare('SELECT * FROM stock_symbols WHERE code = ?').get(code) as StockSymbol) ?? null;
}

export const stockCrawler: Crawler = {
  // 같은 회차의 주식 알림들을 묶어 종목별 1회만 조회한다
  async run(watches: WatchRow[], log: (msg: string) => void): Promise<void> {
    // 종목 코드 → watch 목록
    const byCode = new Map<string, { symbol: StockSymbol; group: WatchRow[] }>();
    for (const watch of watches) {
      const symbol = findSymbol(watch);
      if (!symbol) {
        log(`주식 알림 종목 설정 없음: watch ${watch.id} "${watch.name}" - 건너뜀`);
        continue;
      }
      const entry = byCode.get(symbol.code) ?? { symbol, group: [] };
      entry.group.push(watch);
      byCode.set(symbol.code, entry);
    }

    await Promise.allSettled(
      [...byCode.values()].map(async ({ symbol, group }) => {
        let quote: { price: string; ratio: number };
        try {
          quote = await fetchQuote(symbol);
        } catch (err) {
          const message = err instanceof Error ? err.message : String(err);
          log(`주식 조회 실패 (${symbol.code}): ${message}`);
          for (const watch of group) reportCrawlFailure(watch, `주식 조회 실패 (${symbol.name}): ${message}`);
          return;
        }
        for (const watch of group) clearCrawlFailure(watch);
        const ratioText = quote.ratio > 0 ? `+${quote.ratio}` : `${quote.ratio}`;
        const body = `${formatPrice(symbol, quote.price)}\n전일 대비 ${ratioText}%`;
        const url = detailUrl(symbol);
        for (const watch of group) {
          // 리다이렉트 URL 자동 설정 (사용자가 직접 넣지 않으므로 비어있을 때 채움)
          if (!watch.url) {
            db.prepare('UPDATE watches SET url = ? WHERE id = ? AND url IS NULL').run(url, watch.id);
          }
          enqueueNotification({
            source: 'stock',
            title: notificationTitle('stock', watch.name),
            body,
            url: watch.url ?? url,
            watchId: watch.id,
          });
        }
        log(`주식 조회 완료: ${symbol.name}(${symbol.code}) ${quote.price} (알림 ${group.length}건)`);
      }),
    );
  },

  validateConfig(config: Record<string, unknown>, excludeId: number | null): string | null {
    const code = config.code;
    if (typeof code !== 'string' || code === '') return '종목을 선택해 주세요';
    const symbol = db.prepare('SELECT name FROM stock_symbols WHERE code = ?').get(code) as
      | { name: string }
      | undefined;
    if (!symbol) return '알 수 없는 종목이에요';
    // 1종목 1알림 - 활성 목록에 같은 종목이 이미 있으면 거부
    const dup = db
      .prepare(
        `SELECT id FROM watches
         WHERE provider = 'stock' AND deleted_at IS NULL AND id != ?
           AND json_extract(config, '$.code') = ?`,
      )
      .get(excludeId ?? -1, code);
    if (dup) return `${symbol.name} 알림이 이미 있어요`;
    return null;
  },
};
