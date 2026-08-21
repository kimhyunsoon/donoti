// 알림 종류 2뎁스 카탈로그: 카테고리 > 제공자 (frontend/src/catalog.ts와 동기 유지)
export const CATALOG: Readonly<Record<string, readonly string[]>> = {
  inventory: ['emart'],
  movie: ['cgv', 'lotte', 'megabox'],
  price: ['fx', 'stock'],
};

// 카테고리·제공자 조합이 카탈로그에 있는지
export function isValidTarget(category: string, provider: string): boolean {
  return CATALOG[category]?.includes(provider) ?? false;
}

// 알림 제목 앞에 붙는 provider별 이모지 (영화 3사는 동일, 쇼핑은 마트 아이콘)
const PROVIDER_EMOJI: Record<string, string> = {
  emart: '🛒',
  cgv: '🎬',
  lotte: '🎬',
  megabox: '🎬',
  fx: '💱',
  stock: '📈',
};

// 영화·쇼핑은 어느 브랜드인지 제목에 표기 (시세는 이름만으로 충분)
const PROVIDER_LABELS: Record<string, string> = {
  emart: '이마트',
  cgv: 'CGV',
  lotte: '롯데시네마',
  megabox: '메가박스',
};

/** 알림 제목: '🎬 CGV · 오디세이'처럼 이모지·브랜드 프리픽스를 붙인다 */
export function notificationTitle(provider: string, name: string): string {
  const emoji = PROVIDER_EMOJI[provider];
  const label = PROVIDER_LABELS[provider];
  const text = label ? `${label} · ${name}` : name;
  return emoji ? `${emoji} ${text}` : text;
}
