import { db } from '../db.js';
import { enqueueNotification } from '../notify.js';
import { notificationTitle } from '../catalog.js';
import { withRetry, reportCrawlFailure, clearCrawlFailure } from './retry.js';
import type { Crawler, WatchRow } from './index.js';

// 이마트 크롤러 - 이마트 앱 공유 링크 기반 재고·구매가능 감시
// 지원 유형: digital(디지털그랩 픽업) / product(일반 매장 상품)
// 오더픽·와인그랩 등 나머지는 등록 시 거부 (앱 전용이라 크롤링 불가)

const API = 'https://api-eapp.emart.com';
const UA = { 'User-Agent': 'Mozilla/5.0' };
const TIMEOUT = 10_000;

// 지역 코드 → 지역명 (digital-grab store 응답 기준)
const AREA_NAMES: Record<string, string> = {
  A: '서울', B: '부산', C: '인천', D: '대구', E: '광주', F: '대전', G: '울산', H: '강원',
  I: '경기', J: '경남', K: '경북', L: '전남', M: '전북', N: '충남', O: '충북', P: '제주', Q: '세종',
};

export interface EmartStore {
  code: string;
  name: string;
  area: string;
}

// 등록 폼이 사용하는 링크 해석 결과
export interface EmartResolved {
  type: 'digital' | 'product';
  sku: string;
  name: string;
  price: number;
  link: string;
  linkedStoreCode: string;
  stores: EmartStore[];
}

interface EmartConfig {
  type?: 'digital' | 'product';
  sku?: string;
  name?: string;
  link?: string;
  stores?: { code: string; name: string }[];
  endOnStock?: boolean;
  // 재고 있을 때만 알림 (기본 true, false면 품절 상태도 매회 알림)
  onlyInStock?: boolean;
}

async function getJson<T>(label: string, url: string, init?: RequestInit): Promise<T> {
  // 최대 3회 재시도 (4xx는 미취급 등 정상 흐름이라 withRetry가 즉시 중단)
  return withRetry(label, async () => {
    const res = await fetch(url, {
      headers: { ...UA, 'Content-Type': 'application/json' },
      signal: AbortSignal.timeout(TIMEOUT),
      ...init,
    });
    if (!res.ok) throw new Error(`emart_http_${res.status}`);
    return res.json() as Promise<T>;
  });
}

// ── 디지털그랩 (픽업) ─────────────────────────────────────────

interface DigitalItemRes {
  status: number;
  data: {
    name: string;
    salePrice: number;
    buyLimit: { maxBuyUnlimited: boolean; maxBuyAvailableCount: number } | null;
  } | null;
}

/** 지점별 픽업 상품 조회. 미취급·조회 불가 지점은 null */
async function fetchDigitalItem(
  sku: string,
  storeCode: string,
): Promise<{ name: string; price: number; available: boolean } | null> {
  try {
    const res = await getJson<DigitalItemRes>(
      `이마트 픽업 ${sku}@${storeCode}`,
      `${API}/panda/api/v1/digital-grab/item?skuCode=${sku}&storeCode=${storeCode}`,
    );
    if (res.status !== 200 || !res.data) return null;
    const limit = res.data.buyLimit;
    return {
      name: res.data.name,
      price: res.data.salePrice,
      available: limit != null && (limit.maxBuyUnlimited || limit.maxBuyAvailableCount > 0),
    };
  } catch {
    return null;
  }
}

interface DigitalStoreRow {
  storeCode: string;
  storeName: string;
  areaName: string;
}

// 픽업 상품 취급 지점 목록 (이마트 + 트레이더스)
async function fetchDigitalStores(sku: string): Promise<EmartStore[]> {
  const res = await getJson<{ data: { emart: DigitalStoreRow[] | null; traders: DigitalStoreRow[] | null } }>(
    `이마트 픽업 지점목록 ${sku}`,
    `${API}/panda/api/v1/digital-grab/store?skuCode=${sku}`,
  );
  return [...(res.data.emart ?? []), ...(res.data.traders ?? [])].map((s) => ({
    code: s.storeCode,
    name: s.storeName,
    area: s.areaName,
  }));
}

// ── 일반 매장 상품 ────────────────────────────────────────────

interface ProductInfoRes {
  status: number;
  data: {
    name: string;
    price: { sellPrice: number } | null;
    inventory: { inventory: number; outOfStock: boolean } | null;
  } | null;
}

async function fetchProductInfo(
  sku: string,
  storeId: string,
): Promise<{ name: string; price: number } | null> {
  try {
    const res = await getJson<ProductInfoRes>(
      `이마트 상품 ${sku}@${storeId}`,
      `${API}/product/api/v1/info/1100/${sku}?storeId=${storeId}`,
      { method: 'POST', body: '{}' },
    );
    if (res.status !== 200 || !res.data) return null;
    return { name: res.data.name, price: res.data.price?.sellPrice ?? 0 };
  } catch {
    return null;
  }
}

interface BranchRow {
  storeId: string;
  storeNm: string;
  areaCode: string;
  jijumStatus: string;
  stockQty: number;
}

// 이마트 전 지점 목록
async function fetchProductBranches(): Promise<EmartStore[]> {
  const res = await getJson<{ data: BranchRow[] }>(
    '이마트 지점목록',
    `${API}/search/api/v1/store/branch?storeType=E`,
  );
  return res.data
    .filter((b) => b.jijumStatus === 'OPEN')
    .map((b) => ({ code: b.storeId, name: b.storeNm, area: AREA_NAMES[b.areaCode] ?? '' }));
}

// 지점별 재고 수량 (1콜로 전 지점)
async function fetchStockMap(sku: string): Promise<Map<string, number>> {
  const res = await getJson<{ data: BranchRow[] }>(
    `이마트 재고 ${sku}`,
    `${API}/search/api/v1/store/stock/branch?storeType=E&skuCode=${sku}`,
  );
  return new Map(res.data.map((b) => [b.storeId, b.stockQty]));
}

// ── 공유 링크 해석 (등록 폼용) ─────────────────────────────────

const UNSUPPORTED_MESSAGE =
  '지원하지 않는 링크예요. 이마트 앱의 픽업(디지털그랩)·매장 상품 공유 링크만 등록할 수 있어요';

/**
 * 이마트 공유 링크(emart.kr 단축 또는 eapp.emart.com 직접)를 해석해
 * 상품 정보와 선택 가능한 지점 목록을 돌려준다. 실패 시 한국어 메시지로 throw
 */
export async function resolveEmartLink(input: string): Promise<EmartResolved> {
  // 공유 텍스트에 상품명 등이 섞여 있어도 URL만 뽑아 쓴다
  const extracted = input.match(/https?:\/\/\S+/)?.[0] ?? '';
  let url: URL;
  try {
    url = new URL(extracted);
  } catch {
    throw new Error('올바른 링크가 아니에요');
  }

  // 단축링크는 302 Location의 appLink.do?link=... 에 실제 경로가 들어있다
  if (url.hostname === 'emart.kr') {
    const res = await fetch(url.href, {
      redirect: 'manual',
      headers: UA,
      signal: AbortSignal.timeout(TIMEOUT),
    });
    const location = res.headers.get('location') ?? '';
    const idx = location.indexOf('link=');
    if (idx < 0) throw new Error(UNSUPPORTED_MESSAGE);
    // link= 뒤는 인코딩 없이 ?·&가 그대로 섞여 있어 통째로 다시 파싱한다
    url = new URL(location.slice(idx + 5), 'https://eapp.emart.com');
  }
  if (!url.hostname.endsWith('emart.com')) throw new Error(UNSUPPORTED_MESSAGE);

  if (url.pathname.startsWith('/webapp/digital/view/')) {
    const sku = url.pathname.split('/').pop() ?? '';
    const storeCode = url.searchParams.get('storeCode') ?? '';
    if (!/^\d+$/.test(sku) || storeCode === '') throw new Error(UNSUPPORTED_MESSAGE);
    const [item, stores] = await Promise.all([fetchDigitalItem(sku, storeCode), fetchDigitalStores(sku)]);
    if (!item) throw new Error('상품 정보를 불러오지 못했어요. 잠시 후 다시 시도해 주세요');
    return {
      type: 'digital',
      sku,
      name: item.name,
      price: item.price,
      link: extracted,
      linkedStoreCode: storeCode,
      stores,
    };
  }

  if (url.pathname.startsWith('/webapp/product/view')) {
    const sku = url.searchParams.get('sku_code') ?? '';
    const storeId = url.searchParams.get('storeId') ?? '';
    if (!/^\d+$/.test(sku) || storeId === '') throw new Error(UNSUPPORTED_MESSAGE);
    const [info, stores] = await Promise.all([fetchProductInfo(sku, storeId), fetchProductBranches()]);
    if (!info) throw new Error('상품 정보를 불러오지 못했어요. 잠시 후 다시 시도해 주세요');
    return {
      type: 'product',
      sku,
      name: info.name,
      price: info.price,
      link: extracted,
      linkedStoreCode: storeId,
      stores,
    };
  }

  // 오더픽·와인그랩 등 나머지 전부 (앱 전용이라 크롤링 불가)
  throw new Error(UNSUPPORTED_MESSAGE);
}

// ── 크롤러 ────────────────────────────────────────────────────

// '이마트 ' 접두는 중복이라 제거
function shortStoreName(name: string): string {
  return name.replace(/^이마트 /, '');
}

// watch 하나 크롤링. 구매 가능 지점이 하나라도 있으면 매회 알림 (직전 상태와 무관)
async function runWatch(watch: WatchRow, log: (msg: string) => void): Promise<void> {
  const config = JSON.parse(watch.config) as EmartConfig;
  const { type, sku, stores } = config;
  if (!type || !sku || !stores?.length) {
    log(`이마트 알림 설정 없음: watch ${watch.id} "${watch.name}" - 건너뜀`);
    return;
  }

  // 재고 있을 때만 알림 (기본 on) - 끄면 품절 상태도 매회 알림
  const notifyOutOfStock = config.onlyInStock === false;
  let price = 0;
  // 구매 가능 지점이 있었는지
  let inStock = true;
  // 지점별 한 줄 ('구로점' / '은평점 · 2개')
  let storeLines: string[] = [];
  if (type === 'digital') {
    const quotes = await Promise.all(stores.map((s) => fetchDigitalItem(sku, s.code)));
    const available = stores.filter((_, i) => quotes[i]?.available);
    if (available.length > 0) {
      price = quotes.find((q) => q?.available)?.price ?? 0;
      storeLines = available.map((s) => shortStoreName(s.name));
    } else if (notifyOutOfStock) {
      inStock = false;
      price = quotes.find((q) => q !== null)?.price ?? 0;
    } else {
      return;
    }
  } else {
    const stockMap = await fetchStockMap(sku);
    const available = stores
      .map((s) => ({ ...s, qty: stockMap.get(s.code) ?? 0 }))
      .filter((s) => s.qty > 0);
    if (available.length > 0) {
      const info = await fetchProductInfo(sku, available[0]!.code);
      price = info?.price ?? 0;
      storeLines = available.map((s) => `${shortStoreName(s.name)} · ${s.qty}개`);
    } else if (notifyOutOfStock) {
      inStock = false;
      const info = await fetchProductInfo(sku, stores[0]!.code);
      price = info?.price ?? 0;
    } else {
      return;
    }
  }

  // 첫 줄 상태·가격, 이후 지점별 한 줄씩
  const priceText = price > 0 ? ` · ${price.toLocaleString('ko-KR')}원` : '';
  const head = inStock
    ? `구매 가능${priceText}`
    : `${type === 'digital' ? '품절' : '재고 없음'}${priceText}`;
  const shownStores = storeLines.slice(0, 6);
  if (storeLines.length > 6) shownStores.push(`외 ${storeLines.length - 6}곳`);
  let body = [head, ...shownStores].join('\n');

  // 재고 확인되면 종료 옵션 - 알림 후 목록에서 제거 (종료된 알림에서 복구 가능)
  if (config.endOnStock && inStock) {
    db.prepare(
      `UPDATE watches SET deleted_at = datetime('now'), updated_at = datetime('now')
       WHERE id = ? AND deleted_at IS NULL`,
    ).run(watch.id);
    body += '\n알림 종료';
    log(`재고 확인으로 알림 종료: watch ${watch.id} "${watch.name}"`);
  }

  if (!watch.url && config.link) {
    db.prepare('UPDATE watches SET url = ? WHERE id = ? AND url IS NULL').run(config.link, watch.id);
  }
  enqueueNotification({
    source: 'emart',
    title: notificationTitle('emart', watch.name),
    body,
    url: watch.url ?? config.link,
    watchId: watch.id,
  });
  log(`이마트 재고 확인: "${watch.name}" ${body}`);
}

export const emartCrawler: Crawler = {
  async run(watches: WatchRow[], log: (msg: string) => void): Promise<void> {
    await Promise.allSettled(
      watches.map(async (watch) => {
        try {
          await runWatch(watch, log);
          clearCrawlFailure(watch);
        } catch (err) {
          const message = err instanceof Error ? err.message : String(err);
          log(`이마트 크롤링 실패 (watch ${watch.id}): ${message}`);
          reportCrawlFailure(watch, `이마트 수집 실패: ${message}`);
        }
      }),
    );
  },

  validateConfig(config: Record<string, unknown>): string | null {
    const c = config as EmartConfig;
    if (c.type !== 'digital' && c.type !== 'product') return '이마트 공유 링크를 불러와 주세요';
    if (typeof c.sku !== 'string' || c.sku === '') return '이마트 공유 링크를 불러와 주세요';
    if (!Array.isArray(c.stores) || c.stores.length === 0) return '지점을 1곳 이상 선택해 주세요';
    return null;
  },
};
