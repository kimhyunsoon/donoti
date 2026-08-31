import { db } from '../db.js';
import { enqueueNotification } from '../notify.js';
import { notificationTitle } from '../catalog.js';
import { withRetry, reportCrawlFailure, clearCrawlFailure } from './retry.js';
import type { Crawler, WatchRow } from './index.js';

// 플레이스테이션 스토어(store.playstation.com) 가격·할인 감시 - 상품 링크 붙여넣기 방식
// Next.js SSR 페이지의 cta 배타랑(__NEXT_DATA__ → batarangs.cta.text) 안 apollo 캐시에
// Product → webctas → price(basePriceValue=정가, discountedValue=판매가, endTime=할인 종료 epoch ms)가 있다
// 일반가/PS Plus 전용가 CTA는 serviceBranding(NONE/PS_PLUS)으로 구분한다
// (전용가 CTA가 목록 앞에 오고 isTiedToSubscription도 false라 branding만 신뢰할 것)
// concept 링크는 대표(기본) 에디션을, product 링크는 해당 에디션을 추적한다
// mode 'plusMonthly'는 링크 없이 PS Plus 월간 무료 게임(에센셜) 목록을 감시한다

const STORE_HOST = 'store.playstation.com';
// PS Plus 월간 무료 게임 목록 페이지 (스토어 카테고리 API는 폐기되어 빈 응답 - 이 페이지가 유일한 공개 소스)
const PLUS_MONTHLY_URL = 'https://www.playstation.com/ko-kr/ps-plus/whats-new/';
const UA = {
  'User-Agent':
    'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36',
  'Accept-Language': 'ko-KR,ko;q=0.9',
};
const TIMEOUT = 15_000;

// 등록 폼이 사용하는 링크 해석 결과 (닌텐도와 동일 형태 + PS Plus 전용가·할인 종료 시각)
export interface PlaystationResolved {
  productId: string;
  name: string;
  // 현재 판매가 (할인 반영)
  price: number;
  // 정가
  regularPrice: number;
  // PS Plus(에센셜 이상) 전용가 - 없으면 null
  plusPrice: number | null;
  link: string;
}

interface PlaystationConfig {
  // price(기본): 게임 가격 감시 / plusMonthly: PS Plus 월간 무료 게임 목록 감시
  mode?: 'price' | 'plusMonthly';
  productId?: string;
  name?: string;
  link?: string;
  // 가격 변동 시에만 알림 (기본 true, 1회차는 무조건 알림)
  onPriceChange?: boolean;
  // 무료 게임 목록 변동 시에만 알림 (기본 true, 1회차는 무조건 알림)
  onListChange?: boolean;
}

// watch.state에 기억하는 직전 알림 내용 (retry.ts의 failing 키와 공존)
interface PriceState {
  lastPrice?: number;
  lastPlusPrice?: number | null;
  // plusMonthly: 직전 알림의 게임 슬러그 목록 (정렬·조인)
  lastGames?: string;
}

// apollo 캐시 항목 (필요한 필드만)
interface CacheEntry {
  defaultProduct?: { __ref: string };
  name?: string;
  webctas?: { __ref: string }[];
  price?: {
    basePriceValue: number;
    discountedValue: number;
    serviceBranding?: string[];
    endTime?: string | null;
  };
}

interface FetchedProduct {
  name: string;
  price: number;
  regularPrice: number;
  // PS Plus 전용가 (일반 판매가와 다를 때만, 없으면 null)
  plusPrice: number | null;
  // 할인 종료 시각 (epoch ms, 없으면 null)
  discountEndsAt: number | null;
}

// 이름 뒤의 지원 언어 나열 괄호 제거 ('마블 울버린 (중국어(간체자), 한국어, ...)')
function cleanName(name: string): string {
  return name.replace(/\s*\((?=[^)]*(?:한국어|영어|일본어|중국어))[^)]*(?:\([^)]*\)[^)]*)*\)\s*$/, '').trim();
}

// 상품 페이지에서 이름·판매가·정가·할인 종료 시각 파싱
async function fetchProduct(link: string): Promise<FetchedProduct> {
  return withRetry(`PS 스토어 ${link}`, async () => {
    const res = await fetch(link, { headers: UA, signal: AbortSignal.timeout(TIMEOUT) });
    if (!res.ok) throw new Error(`playstation_http_${res.status}`);
    const html = await res.text();
    const next = html.match(
      /<script id="__NEXT_DATA__" type="application\/json"[^>]*>([\s\S]*?)<\/script>/,
    )?.[1];
    if (!next) throw new Error('playstation_parse_next');
    const pageProps = (JSON.parse(next) as {
      props: {
        pageProps: {
          conceptId?: string;
          productId?: string;
          batarangs?: { cta?: { text?: string } };
        };
      };
    }).props.pageProps;
    const ctaJson = pageProps.batarangs?.cta?.text?.match(
      /<script[^>]*type="application\/json">([\s\S]*?)<\/script>/,
    )?.[1];
    if (!ctaJson) throw new Error('playstation_parse_cta');
    const cache = (JSON.parse(ctaJson) as { cache: Record<string, CacheEntry> }).cache;

    // concept 페이지는 대표 상품, product 페이지는 해당 상품
    const productKey = pageProps.conceptId
      ? cache[`Concept:${pageProps.conceptId}`]?.defaultProduct?.__ref
      : `Product:${pageProps.productId}`;
    const product = productKey ? cache[productKey] : undefined;
    if (!product?.name) throw new Error('playstation_parse_product');

    // 일반가는 serviceBranding NONE, PS Plus 전용가는 PS_PLUS (전용가 CTA가 먼저 오므로 순서로 고르면 안 됨)
    const prices = (product.webctas ?? [])
      .map((c) => cache[c.__ref]?.price)
      .filter((p): p is NonNullable<typeof p> => p != null && typeof p.basePriceValue === 'number');
    const price =
      prices.find((p) => p.serviceBranding?.includes('NONE')) ??
      prices.find((p) => !p.serviceBranding?.includes('PS_PLUS')) ??
      prices[0];
    if (!price) throw new Error('playstation_parse_price');
    const plus = prices.find((p) => p !== price && p.serviceBranding?.includes('PS_PLUS'));
    return {
      name: cleanName(product.name),
      price: price.discountedValue,
      regularPrice: price.basePriceValue,
      plusPrice:
        plus && plus.discountedValue !== price.discountedValue ? plus.discountedValue : null,
      discountEndsAt: price.endTime ? Number(price.endTime) : null,
    };
  });
}

/**
 * 플레이스테이션 스토어 상품 링크를 해석해 상품명·판매가·정가를 돌려준다. 실패 시 한국어 메시지로 throw
 */
export async function resolvePlaystationLink(input: string): Promise<PlaystationResolved> {
  // 공유 텍스트에 상품명 등이 섞여 있어도 URL만 뽑아 쓴다
  const extracted = input.match(/https?:\/\/\S+/)?.[0] ?? '';
  let url: URL;
  try {
    url = new URL(extracted);
  } catch {
    throw new Error('올바른 링크가 아니에요');
  }
  // 다른 국가 링크도 한국 스토어로 정규화 (가격은 ko-kr 기준)
  const path = url.pathname.match(/\/(concept|product)\/([\w-]+)/);
  if (url.hostname !== STORE_HOST || !path) {
    throw new Error('플레이스테이션 스토어(store.playstation.com) 게임 링크만 등록할 수 있어요');
  }
  const link = `https://${STORE_HOST}/ko-kr/${path[1]}/${path[2]}`;
  try {
    const product = await fetchProduct(link);
    return {
      productId: `${path[1]}/${path[2]}`,
      name: product.name,
      price: product.price,
      regularPrice: product.regularPrice,
      plusPrice: product.plusPrice,
      link,
    };
  } catch {
    throw new Error('상품 정보를 불러오지 못했어요. 게임 페이지 링크인지 확인해 주세요');
  }
}

// 할인 종료 시각 표기 (KST '1월 12일 23:59까지')
function formatEndsAt(epochMs: number): string {
  const kst = new Date(epochMs + 9 * 3_600_000);
  const hhmm = `${String(kst.getUTCHours()).padStart(2, '0')}:${String(kst.getUTCMinutes()).padStart(2, '0')}`;
  return `${kst.getUTCMonth() + 1}월 ${kst.getUTCDate()}일 ${hhmm}까지`;
}

// watch 하나 크롤링. '가격 변동 시에만 알림'(기본)은 직전 알림과 일반가·전용가가 다를 때만 발송 (1회차는 무조건)
async function runWatch(watch: WatchRow, log: (msg: string) => void): Promise<void> {
  const config = JSON.parse(watch.config) as PlaystationConfig;
  if (!config.link) {
    log(`PS 스토어 알림 설정 없음: watch ${watch.id} "${watch.name}" - 건너뜀`);
    return;
  }

  const product = await fetchProduct(config.link);
  const state = JSON.parse(watch.state ?? '{}') as PriceState;
  const lastPrice = typeof state.lastPrice === 'number' ? state.lastPrice : null;
  const lastPlusPrice = typeof state.lastPlusPrice === 'number' ? state.lastPlusPrice : null;
  const changed =
    lastPrice === null || lastPrice !== product.price || lastPlusPrice !== product.plusPrice;

  if (changed) {
    // 다음 회차 비교 기준을 갱신 (retry.ts가 같은 state를 쓰므로 in-memory 행도 함께 갱신)
    state.lastPrice = product.price;
    state.lastPlusPrice = product.plusPrice;
    watch.state = JSON.stringify(state);
    db.prepare('UPDATE watches SET state = ? WHERE id = ?').run(watch.state, watch.id);
  }
  if (config.onPriceChange !== false && !changed) {
    log(`PS 스토어 가격 변동 없음: "${watch.name}" ${product.price.toLocaleString('ko-KR')}원 - 알림 생략`);
    return;
  }

  const won = (n: number): string => `${n.toLocaleString('ko-KR')}원`;
  const discounted = product.price < product.regularPrice;
  const lines = [
    discounted
      ? `${won(product.price)} · ${Math.round((1 - product.price / product.regularPrice) * 100)}% 할인`
      : won(product.price),
  ];
  if (discounted) {
    lines.push(
      product.discountEndsAt
        ? `정가 ${won(product.regularPrice)} · ${formatEndsAt(product.discountEndsAt)}`
        : `정가 ${won(product.regularPrice)}`,
    );
  }
  if (product.plusPrice !== null) {
    lines.push(`PS Plus 전용가 ${won(product.plusPrice)}`);
  }
  if (lastPrice !== null && lastPrice !== product.price) {
    lines.push(`직전 알림 ${won(lastPrice)} → ${won(product.price)}`);
  }
  const body = lines.join('\n');

  if (!watch.url) {
    db.prepare('UPDATE watches SET url = ? WHERE id = ? AND url IS NULL').run(config.link, watch.id);
  }
  enqueueNotification({
    source: 'playstation',
    title: notificationTitle('playstation', watch.name),
    body,
    url: watch.url ?? config.link,
    watchId: watch.id,
  });
  log(`PS 스토어 가격 확인: "${watch.name}" ${body.replace(/\n/g, ' / ')}`);
}

// ── PS Plus 월간 무료 게임 ────────────────────────────────────

interface MonthlyGame {
  slug: string;
  title: string;
}

// whats-new 페이지의 id="monthly-games" 섹션에서 이달의 무료 게임 목록 파싱.
// 카드 마크업이 게임 카탈로그 섹션과 동일해, 앵커 뒤 첫 게임 링크부터
// 링크 간격이 확 벌어지는 지점(다음 섹션)까지를 월간 게임 클러스터로 본다
const MONTHLY_CLUSTER_GAP = 8_000;

async function fetchMonthlyGames(): Promise<MonthlyGame[]> {
  return withRetry('PS Plus 무료 게임', async () => {
    const res = await fetch(PLUS_MONTHLY_URL, { headers: UA, signal: AbortSignal.timeout(TIMEOUT) });
    if (!res.ok) throw new Error(`playstation_http_${res.status}`);
    const html = await res.text();
    const anchor = html.indexOf('id="monthly-games"');
    if (anchor < 0) throw new Error('playstation_parse_monthly');
    const seg = html.slice(anchor, anchor + 60_000);

    const games: MonthlyGame[] = [];
    const seen = new Set<string>();
    let prevAt: number | null = null;
    for (const m of seg.matchAll(/href="\/ko-kr\/games\/([a-z0-9-]+)\/"/g)) {
      if (prevAt !== null && m.index - prevAt > MONTHLY_CLUSTER_GAP) break;
      prevAt = m.index;
      const slug = m[1]!;
      if (seen.has(slug)) continue;
      seen.add(slug);
      // 카드 제목은 링크 바로 앞 h2/h3
      const ctx = seg.slice(Math.max(0, m.index - 800), m.index);
      const titles = [...ctx.matchAll(/<h[23][^>]*>\s*([^<]+?)\s*<\/h[23]>/g)];
      games.push({ slug, title: titles.at(-1)?.[1] ?? slug.replace(/-/g, ' ') });
    }
    if (games.length === 0) throw new Error('playstation_parse_monthly');
    return games;
  });
}

// 무료 게임 목록 감시. '목록 변동 시에만 알림'(기본)은 직전 알림과 다를 때만 발송 (1회차는 무조건)
async function runMonthlyWatch(watch: WatchRow, log: (msg: string) => void): Promise<void> {
  const config = JSON.parse(watch.config) as PlaystationConfig;
  const games = await fetchMonthlyGames();
  const key = games.map((g) => g.slug).sort().join(',');

  const state = JSON.parse(watch.state ?? '{}') as PriceState;
  const changed = state.lastGames !== key;
  if (changed) {
    state.lastGames = key;
    watch.state = JSON.stringify(state);
    db.prepare('UPDATE watches SET state = ? WHERE id = ?').run(watch.state, watch.id);
  }
  if (config.onListChange !== false && !changed) {
    log(`PS Plus 무료 게임 변동 없음: ${games.length}종 - 알림 생략`);
    return;
  }

  const body = [`이달의 무료 게임 ${games.length}종`, ...games.map((g) => g.title)].join('\n');
  if (!watch.url) {
    db.prepare('UPDATE watches SET url = ? WHERE id = ? AND url IS NULL').run(
      PLUS_MONTHLY_URL,
      watch.id,
    );
  }
  enqueueNotification({
    source: 'playstation',
    title: notificationTitle('playstation', watch.name),
    body,
    url: watch.url ?? PLUS_MONTHLY_URL,
    watchId: watch.id,
  });
  log(`PS Plus 무료 게임 확인: ${body.replace(/\n/g, ' / ')}`);
}

export const playstationCrawler: Crawler = {
  async run(watches: WatchRow[], log: (msg: string) => void): Promise<void> {
    await Promise.allSettled(
      watches.map(async (watch) => {
        try {
          const mode = (JSON.parse(watch.config) as PlaystationConfig).mode;
          await (mode === 'plusMonthly' ? runMonthlyWatch(watch, log) : runWatch(watch, log));
          clearCrawlFailure(watch);
        } catch (err) {
          const message = err instanceof Error ? err.message : String(err);
          log(`PS 스토어 크롤링 실패 (watch ${watch.id}): ${message}`);
          reportCrawlFailure(watch, `PS 스토어 수집 실패: ${message}`);
        }
      }),
    );
  },

  validateConfig(config: Record<string, unknown>): string | null {
    const c = config as PlaystationConfig;
    // 무료 게임 목록 감시는 링크가 필요 없다
    if (c.mode === 'plusMonthly') return null;
    if (typeof c.productId !== 'string' || c.productId === '') {
      return '플레이스테이션 스토어 링크를 불러와 주세요';
    }
    if (typeof c.link !== 'string' || !c.link.startsWith(`https://${STORE_HOST}/`)) {
      return '플레이스테이션 스토어 링크를 불러와 주세요';
    }
    return null;
  },
};
