import { db } from '../db.js';
import { enqueueNotification } from '../notify.js';
import { withRetry, reportCrawlFailure, clearCrawlFailure } from './retry.js';
import type { Crawler, WatchRow } from './index.js';

// 디시인사이드 갤러리 새 글 감시 - 갤러리 링크 + 키워드(여러 개) 등록 방식
// 목록 페이지(gall.dcinside.com)는 정적 HTML이라 fetch + 정규식으로 충분하다.
// 글 번호(data-no)가 단조 증가하므로 직전 회차 최대 번호(state.lastNo) 이후 글만 새 글로 본다.

const HOST = 'gall.dcinside.com';
const UA = {
  'User-Agent':
    'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36',
  'Accept-Language': 'ko-KR,ko;q=0.9',
};
const TIMEOUT = 15_000;
// 새 글이 한 페이지를 넘길 때 더 넘겨보는 최대 페이지 수
const MAX_PAGES = 3;
// 한 회차에 watch 하나가 보내는 최대 알림 수 (키워드가 너무 넓을 때 폭주 방지)
const MAX_NOTIFY = 10;
const TITLE_SUFFIX = / - 커뮤니티 포털 디시인사이드\s*$/;

// 갤러리 종류: 정식(board) / 마이너(mgallery) / 미니(mini)
export type GalleryType = 'board' | 'mgallery' | 'mini';

// 등록 폼이 사용하는 링크 해석 결과
export interface DcinsideResolved {
  galleryId: string;
  galleryType: GalleryType;
  galleryName: string;
  link: string;
  // 현재 목록의 최신 글 번호 (참고용)
  latestNo: number;
}

interface DcinsideConfig {
  galleryId?: string;
  galleryType?: GalleryType;
  galleryName?: string;
  link?: string;
  keywords?: string[];
  // 개념글(추천글)만 감시
  recommendOnly?: boolean;
  // 링크 해석 시점의 최신 글 번호 - 등록 시 기준 번호(state.lastNo)가 되어 그 이후 글부터 알림
  latestNo?: number;
}

// watch.state에 기억하는 직전 회차 최대 글 번호 (retry.ts의 failing 키와 공존)
interface PostState {
  lastNo?: number;
}

interface Post {
  no: number;
  title: string;
  nick: string;
  // 'YYYY-MM-DD HH:MM:SS' (목록의 title 속성)
  at: string;
  link: string;
}

// 갤러리 종류별 목록 경로
function listPath(type: GalleryType): string {
  return type === 'board' ? '/board/lists/' : `/${type}/board/lists/`;
}

function viewPath(type: GalleryType): string {
  return type === 'board' ? '/board/view/' : `/${type}/board/view/`;
}

// 갤러리 목록 URL (recommendOnly면 개념글 탭)
function listUrl(type: GalleryType, id: string, page: number, recommendOnly: boolean): string {
  const params = new URLSearchParams({ id });
  if (recommendOnly) params.set('exception_mode', 'recommend');
  if (page > 1) params.set('page', String(page));
  return `https://${HOST}${listPath(type)}?${params.toString()}`;
}

// 목록에 쓰이는 HTML 엔티티 복원
function decodeEntities(text: string): string {
  return text
    .replace(/&#(\d+);/g, (_, n: string) => String.fromCodePoint(Number(n)))
    .replace(/&#x([0-9a-f]+);/gi, (_, n: string) => String.fromCodePoint(parseInt(n, 16)))
    .replaceAll('&quot;', '"')
    .replaceAll('&#039;', "'")
    .replaceAll('&lt;', '<')
    .replaceAll('&gt;', '>')
    .replaceAll('&nbsp;', ' ')
    .replaceAll('&amp;', '&');
}

// 목록 HTML → 일반 글 목록 (공지·광고·설문 행은 us-post 클래스가 없어 제외됨)
function parsePosts(html: string, type: GalleryType, id: string): Post[] {
  const posts: Post[] = [];
  const rowRe = /<tr class="ub-content us-post"[^>]*data-no="(\d+)"[\s\S]*?<\/tr>/g;
  for (const match of html.matchAll(rowRe)) {
    const row = match[0];
    const no = Number(match[1]);
    const titleHtml = row.match(/<td class="gall_tit[^"]*">[\s\S]*?<a\s+href="[^"]*"[^>]*>([\s\S]*?)<\/a>/)?.[1];
    if (!titleHtml) continue;
    const title = decodeEntities(titleHtml.replace(/<[^>]+>/g, '')).replace(/\s+/g, ' ').trim();
    const nick = decodeEntities(row.match(/data-nick="([^"]*)"/)?.[1] ?? '');
    const at = row.match(/<td class="gall_date" title="([^"]*)"/)?.[1] ?? '';
    posts.push({ no, title, nick, at, link: `https://${HOST}${viewPath(type)}?id=${id}&no=${no}` });
  }
  return posts;
}

// 목록 페이지 1장 조회 → 갤러리 이름 + 글 목록
async function fetchList(
  type: GalleryType,
  id: string,
  page: number,
  recommendOnly: boolean,
): Promise<{ galleryName: string; posts: Post[] }> {
  const url = listUrl(type, id, page, recommendOnly);
  const html = await withRetry(`디시 목록 ${id} p${page}`, async () => {
    const res = await fetch(url, { headers: UA, signal: AbortSignal.timeout(TIMEOUT) });
    if (!res.ok) throw new Error(`dcinside_http_${res.status}`);
    return res.text();
  });
  // 갤러리 종류(정식/마이너/미니)가 다르면 본문 없이 JS 리다이렉트만 온다 → 올바른 종류를 알려준다
  const redirect = html.match(/location\.replace\("https:\/\/gall\.dcinside\.com\/(?:(mgallery|mini)\/)?board\/lists/);
  if (redirect) throw new Error(`dcinside_redirect_${redirect[1] ?? 'board'}`);
  const rawTitle = html.match(/<title>([^<]*)<\/title>/)?.[1] ?? '';
  if (!TITLE_SUFFIX.test(rawTitle)) throw new Error('dcinside_parse');
  const galleryName = decodeEntities(rawTitle.replace(TITLE_SUFFIX, '').trim());
  return { galleryName, posts: parsePosts(html, type, id) };
}

/**
 * 갤러리 링크(PC·모바일·글 링크 모두 허용)를 해석해 갤러리 정보를 돌려준다. 실패 시 한국어 메시지로 throw
 */
export async function resolveDcinsideLink(input: string): Promise<DcinsideResolved> {
  const extracted = input.match(/https?:\/\/\S+/)?.[0] ?? '';
  let url: URL;
  try {
    url = new URL(extracted);
  } catch {
    throw new Error('올바른 링크가 아니에요');
  }
  let type: GalleryType | null = null;
  let id: string | null = null;
  if (url.hostname === HOST) {
    // /board/lists/?id=x, /mgallery/board/view/?id=x&no=1, /mini/board/lists/?id=x
    const seg = url.pathname.split('/')[1] ?? '';
    type = seg === 'mgallery' || seg === 'mini' ? seg : seg === 'board' ? 'board' : null;
    id = url.searchParams.get('id');
  } else if (url.hostname === 'm.dcinside.com') {
    // 모바일: /board/<id>, /mini/<id>, /board/<id>/<no> - 마이너 여부는 접근해서 판별
    const [seg, mid] = url.pathname.split('/').filter(Boolean);
    if (seg === 'mini') type = 'mini';
    else if (seg === 'board') type = 'board';
    id = mid ?? null;
  }
  if (!type || !id || !/^[\w-]+$/.test(id)) {
    throw new Error('디시인사이드 갤러리 링크(gall.dcinside.com)만 등록할 수 있어요');
  }

  // 종류가 다르면(모바일 링크는 정식/마이너 구분이 없음) 리다이렉트 안내를 따라 한 번 더 시도
  let galleryType: GalleryType = type;
  for (let attempt = 0; attempt < 2; attempt++) {
    try {
      const { galleryName, posts } = await fetchList(galleryType, id, 1, false);
      return {
        galleryId: id,
        galleryType,
        galleryName,
        link: listUrl(galleryType, id, 1, false),
        latestNo: posts[0]?.no ?? 0,
      };
    } catch (err) {
      const redirected = (err instanceof Error ? err.message : '').match(/^dcinside_redirect_(board|mgallery|mini)$/)?.[1];
      if (!redirected || redirected === galleryType) break;
      galleryType = redirected as GalleryType;
    }
  }
  throw new Error('갤러리를 불러오지 못했어요. 갤러리 주소를 확인해 주세요');
}

// 키워드 정규화 (대소문자·공백 무시)
function norm(text: string): string {
  return text.toLowerCase().replace(/\s+/g, '');
}

// 같은 회차 안에서 (갤러리, 페이지, 개념글 여부) 조회를 한 번만 하기 위한 캐시
type ListCache = Map<string, Promise<{ galleryName: string; posts: Post[] }>>;

function cachedList(
  cache: ListCache,
  type: GalleryType,
  id: string,
  page: number,
  recommendOnly: boolean,
): Promise<{ galleryName: string; posts: Post[] }> {
  const key = `${type}/${id}/${page}/${recommendOnly ? 'r' : 'a'}`;
  let pending = cache.get(key);
  if (!pending) {
    pending = fetchList(type, id, page, recommendOnly);
    cache.set(key, pending);
  }
  return pending;
}

// watch 하나 크롤링. 기준 번호(state.lastNo)는 등록 시 initialState가 잡아두므로 첫 회차부터 알림이 간다.
// 기준이 없으면(구버전 등록분) 이번 회차 최대 번호만 기록하고 알림은 보내지 않는다
async function runWatch(watch: WatchRow, cache: ListCache, log: (msg: string) => void): Promise<void> {
  const config = JSON.parse(watch.config) as DcinsideConfig;
  const type = config.galleryType;
  const id = config.galleryId;
  const keywords = (config.keywords ?? []).map(norm).filter((k) => k !== '');
  if (!type || !id || keywords.length === 0) {
    log(`디시 알림 설정 없음: watch ${watch.id} "${watch.name}" - 건너뜀`);
    return;
  }
  const recommendOnly = config.recommendOnly === true;
  const state = JSON.parse(watch.state ?? '{}') as PostState;
  const lastNo = typeof state.lastNo === 'number' ? state.lastNo : null;

  // 직전 번호보다 새 글만 모은다 - 한 페이지를 넘기면 다음 페이지까지 (최대 MAX_PAGES)
  const fresh: Post[] = [];
  let galleryName = config.galleryName ?? id;
  let maxNo = lastNo ?? 0;
  for (let page = 1; page <= MAX_PAGES; page++) {
    const list = await cachedList(cache, type, id, page, recommendOnly);
    galleryName = list.galleryName;
    if (list.posts.length === 0) break;
    maxNo = Math.max(maxNo, ...list.posts.map((p) => p.no));
    if (lastNo === null) break;
    const newer = list.posts.filter((p) => p.no > lastNo);
    fresh.push(...newer);
    // 페이지 끝까지 전부 새 글이면 다음 페이지에도 새 글이 있을 수 있다
    if (newer.length < list.posts.length) break;
  }

  if (maxNo !== (lastNo ?? -1)) {
    state.lastNo = maxNo;
    watch.state = JSON.stringify(state);
    db.prepare('UPDATE watches SET state = ? WHERE id = ?').run(watch.state, watch.id);
  }
  if (!watch.url && config.link) {
    db.prepare('UPDATE watches SET url = ? WHERE id = ? AND url IS NULL').run(config.link, watch.id);
  }
  if (lastNo === null) {
    log(`디시 기준 번호 기록: "${watch.name}" ${galleryName} #${maxNo} - 이후 새 글부터 알림`);
    return;
  }

  // 오래된 글부터 순서대로, 키워드 일치 글만
  const matched = fresh
    .filter((p) => keywords.some((k) => norm(p.title).includes(k)))
    .sort((a, b) => a.no - b.no);
  if (matched.length === 0) {
    log(`디시 새 글 ${fresh.length}건 중 키워드 일치 없음: "${watch.name}"`);
    return;
  }
  const overflow = matched.length - MAX_NOTIFY;
  for (const post of matched.slice(-MAX_NOTIFY)) {
    const time = post.at.slice(11, 16);
    // 제목은 글 제목 그대로, 본문에 갤러리·글쓴이·시각
    enqueueNotification({
      source: 'dcinside',
      title: post.title,
      body: `${galleryName} · ${post.nick}${time ? ` · ${time}` : ''}`,
      url: post.link,
      watchId: watch.id,
    });
  }
  if (overflow > 0) {
    enqueueNotification({
      source: 'dcinside',
      title: `${galleryName} 새 글 ${overflow}건 더 있어요`,
      body: `키워드 일치 글이 많아 최근 ${MAX_NOTIFY}건만 알렸어요`,
      url: config.link,
      watchId: watch.id,
    });
  }
  log(`디시 새 글 알림: "${watch.name}" ${matched.length}건 (새 글 ${fresh.length}건 중)`);
}

export const dcinsideCrawler: Crawler = {
  async run(watches: WatchRow[], log: (msg: string) => void): Promise<void> {
    const cache: ListCache = new Map();
    await Promise.allSettled(
      watches.map(async (watch) => {
        try {
          await runWatch(watch, cache, log);
          clearCrawlFailure(watch);
        } catch (err) {
          const message = err instanceof Error ? err.message : String(err);
          log(`디시 크롤링 실패 (watch ${watch.id}): ${message}`);
          reportCrawlFailure(watch, `디시인사이드 수집 실패: ${message}`);
        }
      }),
    );
  },

  validateConfig(config: Record<string, unknown>): string | null {
    const c = config as DcinsideConfig;
    if (typeof c.galleryId !== 'string' || c.galleryId === '') return '갤러리 링크를 불러와 주세요';
    if (c.galleryType !== 'board' && c.galleryType !== 'mgallery' && c.galleryType !== 'mini') {
      return '갤러리 링크를 불러와 주세요';
    }
    const keywords = Array.isArray(c.keywords) ? c.keywords : [];
    if (keywords.some((k) => typeof k !== 'string')) return '키워드가 올바르지 않아요';
    if (keywords.filter((k) => k.trim() !== '').length === 0) return '키워드를 1개 이상 입력해 주세요';
    if (keywords.length > 20) return '키워드는 20개까지 등록할 수 있어요';
    if (typeof c.latestNo !== 'number') return '갤러리 링크를 다시 불러와 주세요';
    return null;
  },

  // 등록 시점의 최신 글 번호를 기준으로. 수정 시 갤러리가 그대로면 기존 기준 유지 (키워드만 바꿔도 재알림 없음)
  initialState(config: Record<string, unknown>, previous: Record<string, unknown> | null): Record<string, unknown> | null {
    const c = config as DcinsideConfig;
    const p = previous as DcinsideConfig | null;
    if (p && p.galleryId === c.galleryId && p.galleryType === c.galleryType) return null;
    return { lastNo: c.latestNo };
  },
};
