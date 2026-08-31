import { db } from '../db.js';
import { enqueueNotification } from '../notify.js';
import { notificationTitle } from '../catalog.js';
import { withRetry, reportCrawlFailure, clearCrawlFailure } from './retry.js';
import type { Crawler, WatchRow } from './index.js';

// 닌텐도 스토어(store.nintendo.co.kr) 가격·할인 감시 - 상품 링크 붙여넣기 방식
// 웹 스토어는 Magento 기반: 상품 페이지 HTML의 price_info JSON에 판매가(final)·정가(regular)가 있고
// eShop 세일도 여기에 special price로 반영된다 (할인 시 final < regular)

const STORE_HOST = 'store.nintendo.co.kr';
const UA = {
  'User-Agent':
    'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36',
};
const TIMEOUT = 15_000;

// 등록 폼이 사용하는 링크 해석 결과
export interface NintendoResolved {
  productId: string;
  name: string;
  // 현재 판매가 (할인 반영)
  price: number;
  // 정가
  regularPrice: number;
  link: string;
}

interface NintendoConfig {
  productId?: string;
  name?: string;
  link?: string;
  // 가격 변동 시에만 알림 (기본 true, 1회차는 무조건 알림)
  onPriceChange?: boolean;
}

// watch.state에 기억하는 직전 알림 가격 (retry.ts의 failing 키와 공존)
interface PriceState {
  lastPrice?: number;
}

// 상품 페이지에서 이름·판매가·정가 파싱
async function fetchProduct(link: string): Promise<{ name: string; price: number; regularPrice: number }> {
  return withRetry(`닌텐도 상품 ${link}`, async () => {
    const res = await fetch(link, { headers: UA, signal: AbortSignal.timeout(TIMEOUT) });
    if (!res.ok) throw new Error(`nintendo_http_${res.status}`);
    const html = await res.text();
    // 첫 price_info가 메인 상품의 priceBox 설정 (formatted_prices 전까지가 숫자 필드 구간)
    const info = html.match(/"price_info":\{([\s\S]*?)"formatted_prices"/)?.[1] ?? '';
    const final = info.match(/"final_price":([\d.]+)/)?.[1];
    const regular = info.match(/"regular_price":([\d.]+)/)?.[1];
    const title = html.match(/<title>([^<]*)<\/title>/)?.[1]?.trim();
    if (!final || !regular || !title) throw new Error('nintendo_parse');
    return { name: title, price: Math.round(Number(final)), regularPrice: Math.round(Number(regular)) };
  });
}

/**
 * 닌텐도 스토어 상품 링크를 해석해 상품명·판매가·정가를 돌려준다. 실패 시 한국어 메시지로 throw
 */
export async function resolveNintendoLink(input: string): Promise<NintendoResolved> {
  // 공유 텍스트에 상품명 등이 섞여 있어도 URL만 뽑아 쓴다
  const extracted = input.match(/https?:\/\/\S+/)?.[0] ?? '';
  let url: URL;
  try {
    url = new URL(extracted);
  } catch {
    throw new Error('올바른 링크가 아니에요');
  }
  if (url.hostname !== STORE_HOST) {
    throw new Error('닌텐도 스토어(store.nintendo.co.kr) 상품 링크만 등록할 수 있어요');
  }
  const link = `https://${STORE_HOST}${url.pathname}`;
  try {
    const product = await fetchProduct(link);
    return { productId: url.pathname.replace(/^\/+/, ''), ...product, link };
  } catch {
    throw new Error('상품 정보를 불러오지 못했어요. 상품 페이지 링크인지 확인해 주세요');
  }
}

// watch 하나 크롤링. '가격 변동 시에만 알림'(기본)은 직전 알림 가격과 다를 때만 발송 (1회차는 무조건)
async function runWatch(watch: WatchRow, log: (msg: string) => void): Promise<void> {
  const config = JSON.parse(watch.config) as NintendoConfig;
  if (!config.link) {
    log(`닌텐도 알림 설정 없음: watch ${watch.id} "${watch.name}" - 건너뜀`);
    return;
  }

  const product = await fetchProduct(config.link);
  const state = JSON.parse(watch.state ?? '{}') as PriceState;
  const lastPrice = typeof state.lastPrice === 'number' ? state.lastPrice : null;
  const changed = lastPrice === null || lastPrice !== product.price;

  if (changed) {
    // 다음 회차 비교 기준을 갱신 (retry.ts가 같은 state를 쓰므로 in-memory 행도 함께 갱신)
    state.lastPrice = product.price;
    watch.state = JSON.stringify(state);
    db.prepare('UPDATE watches SET state = ? WHERE id = ?').run(watch.state, watch.id);
  }
  if (config.onPriceChange !== false && !changed) {
    log(`닌텐도 가격 변동 없음: "${watch.name}" ${product.price.toLocaleString('ko-KR')}원 - 알림 생략`);
    return;
  }

  const won = (n: number): string => `${n.toLocaleString('ko-KR')}원`;
  const discounted = product.price < product.regularPrice;
  const lines = [
    discounted
      ? `${won(product.price)} · ${Math.round((1 - product.price / product.regularPrice) * 100)}% 할인`
      : won(product.price),
  ];
  if (discounted) lines.push(`정가 ${won(product.regularPrice)}`);
  if (lastPrice !== null && lastPrice !== product.price) {
    lines.push(`직전 알림 ${won(lastPrice)} → ${won(product.price)}`);
  }
  const body = lines.join('\n');

  if (!watch.url) {
    db.prepare('UPDATE watches SET url = ? WHERE id = ? AND url IS NULL').run(config.link, watch.id);
  }
  enqueueNotification({
    source: 'nintendo',
    title: notificationTitle('nintendo', watch.name),
    body,
    url: watch.url ?? config.link,
    watchId: watch.id,
  });
  log(`닌텐도 가격 확인: "${watch.name}" ${body.replace(/\n/g, ' / ')}`);
}

export const nintendoCrawler: Crawler = {
  async run(watches: WatchRow[], log: (msg: string) => void): Promise<void> {
    await Promise.allSettled(
      watches.map(async (watch) => {
        try {
          await runWatch(watch, log);
          clearCrawlFailure(watch);
        } catch (err) {
          const message = err instanceof Error ? err.message : String(err);
          log(`닌텐도 크롤링 실패 (watch ${watch.id}): ${message}`);
          reportCrawlFailure(watch, `닌텐도 수집 실패: ${message}`);
        }
      }),
    );
  },

  validateConfig(config: Record<string, unknown>): string | null {
    const c = config as NintendoConfig;
    if (typeof c.productId !== 'string' || c.productId === '') return '닌텐도 스토어 링크를 불러와 주세요';
    if (typeof c.link !== 'string' || !c.link.startsWith(`https://${STORE_HOST}/`)) {
      return '닌텐도 스토어 링크를 불러와 주세요';
    }
    return null;
  },
};
