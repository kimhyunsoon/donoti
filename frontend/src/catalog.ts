import { html, type TemplateResult } from 'lit';
import { icon } from './icons.js';

// 알림 종류 2뎁스 카탈로그 (backend/src/catalog.ts와 동기 유지)

export interface ProviderInfo {
  id: string;
  label: string;
  // 브랜드 이미지 경로 (public/brands) - 없으면 icon+color로 렌더
  image?: string;
  icon?: string;
  color?: string;
}

export interface CategoryInfo {
  id: string;
  label: string;
  providers: ProviderInfo[];
}

export const CATALOG: CategoryInfo[] = [
  {
    id: 'inventory',
    label: '쇼핑',
    providers: [{ id: 'emart', label: '이마트', image: '/brands/emart.jpg' }],
  },
  {
    id: 'movie',
    label: '영화',
    providers: [
      { id: 'cgv', label: 'CGV', image: '/brands/cgv.jpg' },
      { id: 'lotte', label: '롯데시네마', image: '/brands/lotte.jpg' },
      { id: 'megabox', label: '메가박스', image: '/brands/megabox.jpg' },
    ],
  },
  {
    id: 'price',
    label: '시세',
    providers: [
      { id: 'fx', label: '환율', icon: 'dollar', color: '#1f8ce6' },
      { id: 'stock', label: '주식', icon: 'trending-up', color: '#e6455a' },
    ],
  },
];

// 환율 지원 통화 (backend/src/crawlers/fx.ts CURRENCIES와 동기 유지)
export const FX_CURRENCIES: { code: string; label: string }[] = [
  { code: 'USD', label: '달러' },
  { code: 'JPY', label: '엔' },
  { code: 'EUR', label: '유로' },
  { code: 'CNY', label: '위안' },
  { code: 'IDR', label: '루피아' },
  { code: 'TWD', label: '대만달러' },
  { code: 'HKD', label: '홍콩달러' },
];

// 주식 종목 분류 라벨 (backend stock_symbols.kind와 동기 유지)
export const STOCK_KIND_LABELS: Record<string, string> = {
  kr: '한국주식',
  us: '미국주식',
  index: '지수',
  etf: 'ETF',
};

/** 카탈로그에서 category·provider 조합 찾기 */
export function findProvider(
  categoryId: string,
  providerId: string,
): { category: CategoryInfo; provider: ProviderInfo } | null {
  const category = CATALOG.find((c) => c.id === categoryId);
  const provider = category?.providers.find((p) => p.id === providerId);
  return category && provider ? { category, provider } : null;
}

/** provider id만으로 찾기 (id는 카탈로그 전체에서 유일 - 라우터 경로용) */
export function findProviderById(
  providerId: string,
): { category: CategoryInfo; provider: ProviderInfo } | null {
  for (const category of CATALOG) {
    const provider = category.providers.find((p) => p.id === providerId);
    if (provider) return { category, provider };
  }
  return null;
}

/**
 * 브랜드 아바타 (토스 스타일 라운드 사각). 브랜드 이미지(공식 앱 아이콘)는 전체 채움,
 * 없으면 브랜드 컬러 배경 위에 SVG 아이콘으로 렌더
 */
export function providerAvatar(p: ProviderInfo, size = 40): TemplateResult {
  const base =
    `width:${size}px;height:${size}px;border-radius:${Math.round(size * 0.28)}px;` +
    'display:grid;place-items:center;flex-shrink:0;overflow:hidden;';
  if (p.image) {
    return html`
      <img src=${p.image} alt=${p.label} style="${base}object-fit:cover">
    `;
  }
  return html`
    <span style="${base}background:${p.color ?? 'var(--accent)'};color:#fff">
      ${icon(p.icon ?? 'bell', Math.round(size * 0.52))}
    </span>
  `;
}
