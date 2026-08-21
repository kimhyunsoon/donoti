import { db } from '../db.js';
import { enqueueNotification } from '../notify.js';
import { notificationTitle } from '../catalog.js';
import { kstNow } from '../watch-schedule.js';
import { withRetry, reportCrawlFailure, clearCrawlFailure } from './retry.js';
import type { Crawler, WatchRow } from './index.js';

// CGV 크롤러 - 영화 1편 × 지점 N × 날짜 N × 시간범위의 상영·잔여석 감시
// 알림은 지점당 1건 (매칭 상영들의 관·시간·잔여석 나열)
// 예매 가능(잔여석 있는 상영)이 있을 때만 알림

const API = 'https://cgv.co.kr/api/v1';
const CO_CD = 'A420';
const HEADERS = {
  'User-Agent':
    'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/148.0.0.0 Safari/537.36',
  Accept: 'application/json',
  'Accept-Language': 'ko-KR',
  Referer: 'https://cgv.co.kr/cnm/movieBook/cinema',
};

const BOOKING_URL = 'https://cgv.co.kr/cnm/movieBook/cinema';
const DAY_LABELS = ['일', '월', '화', '수', '목', '금', '토'];

interface CgvConfig {
  movieNo?: string;
  movieName?: string;
  stores?: { code: string; name: string }[];
  dates?: string[]; // 'YYYY-MM-DD'
  start?: string; // 'HH:MM' (상영 시작시간 기준)
  end?: string;
  // IMAX 상영만 알림
  imaxOnly?: boolean;
}

// CGV는 쿠키(__cf_bm 등) 없는 반복 호출을 간헐적으로 HTML 에러 페이지로 차단한다.
// 예매 페이지에서 쿠키를 받아 유지하고, 차단 감지 시 쿠키 갱신 후 1회 재시도.
let cookie = '';

async function refreshCookie(): Promise<void> {
  const res = await fetch(BOOKING_URL, {
    headers: { 'User-Agent': HEADERS['User-Agent'], 'Accept-Language': 'ko-KR' },
    signal: AbortSignal.timeout(10_000),
  });
  const setCookies = res.headers.getSetCookie();
  if (setCookies.length > 0) {
    cookie = setCookies.map((c) => c.split(';')[0]).join('; ');
  }
}

async function fetchJson<T>(label: string, url: string): Promise<T> {
  // 최대 3회 재시도 - 2회차부터는 쿠키를 새로 받아 차단을 우회한다
  return withRetry(label, async (attempt) => {
    if (attempt > 0) await refreshCookie();
    const res = await fetch(url, {
      headers: { ...HEADERS, ...(cookie !== '' ? { Cookie: cookie } : {}) },
      signal: AbortSignal.timeout(10_000),
    });
    if (!res.ok) throw new Error(`cgv_http_${res.status}`);
    const text = await res.text();
    // 차단 시 200이어도 HTML 에러 페이지가 온다
    if (!text.trimStart().startsWith('{')) throw new Error('cgv_blocked');
    return JSON.parse(text) as T;
  });
}

// ── 등록 폼용 목록 ────────────────────────────────────────────

interface MovieRow {
  movNo: string;
  movNm: string;
}

/** 현재 예매 가능한 상영작 목록 */
export async function fetchCgvMovies(): Promise<{ code: string; name: string }[]> {
  const res = await fetchJson<{ data: MovieRow[] }>(
    'CGV 상영작',
    `${API}/booking/searchAtktTopPostrList?coCd=${CO_CD}&movNm=&div=&attrCd=`,
  );
  return (res.data ?? []).map((m) => ({ code: m.movNo, name: m.movNm }));
}

interface RegionRow {
  comCdval: string;
  comCdvalNm: string;
}

interface SiteRow {
  regnGrpCd: string;
  siteNo: string;
  siteNm: string;
}

/** 전체 지점 목록 (지역명 포함) */
export async function fetchCgvTheaters(): Promise<{ code: string; name: string; area: string }[]> {
  const res = await fetchJson<{ data: { regionInfo: RegionRow[]; siteInfo: SiteRow[] } }>(
    'CGV 지점목록',
    `${API}/content/site/searchAllRegionAndSite?coCd=${CO_CD}`,
  );
  const areaNames = new Map((res.data.regionInfo ?? []).map((r) => [r.comCdval, r.comCdvalNm]));
  return (res.data.siteInfo ?? []).map((s) => ({
    code: s.siteNo,
    name: s.siteNm,
    area: areaNames.get(s.regnGrpCd) ?? '',
  }));
}

// ── 상영표 ────────────────────────────────────────────────────

interface ScnRow {
  movNo: string;
  expoScnsNm: string; // '1관 (Laser)' | 'IMAX관' 등
  movkndDsplNm: string; // '2D' | 'IMAX' | '4DX' 등
  scnYmd: string; // '20260821'
  scnsrtTm: string; // '1705'
  frSeatCnt: string; // 잔여석
}

// 지점·날짜별 전체 상영표
async function fetchScreenings(siteNo: string, ymd8: string): Promise<ScnRow[]> {
  const res = await fetchJson<{ data: ScnRow[] | null }>(
    `CGV 상영표 ${siteNo} ${ymd8}`,
    `${API}/booking/searchMovScnInfo?coCd=${CO_CD}&siteNo=${siteNo}&scnYmd=${ymd8}&rtctlScopCd=08`,
  );
  return res.data ?? [];
}

// 'HH:MM' → 'HHMM' (상영표의 scnsrtTm 형식)
function toHm4(hhmm: string): string {
  return hhmm.replace(':', '');
}

// '20260821' → '8/21 (금)'
function dateHeader(ymd8: string): string {
  const date = new Date(`${ymd8.slice(0, 4)}-${ymd8.slice(4, 6)}-${ymd8.slice(6, 8)}T00:00`);
  return `${Number(ymd8.slice(4, 6))}/${Number(ymd8.slice(6, 8))} (${DAY_LABELS[date.getDay()]})`;
}

// watch 하나 크롤링. 지점당 1건 알림
async function runWatch(
  watch: WatchRow,
  getRows: (siteNo: string, ymd8: string) => Promise<ScnRow[]>,
  log: (msg: string) => void,
): Promise<void> {
  const config = JSON.parse(watch.config) as CgvConfig;
  const { movieNo, stores, dates } = config;
  if (!movieNo || !stores?.length || !dates?.length) {
    log(`CGV 알림 설정 없음: watch ${watch.id} "${watch.name}" - 건너뜀`);
    return;
  }
  const startTm = toHm4(config.start ?? '00:00');
  const endTm = toHm4(config.end ?? '23:59');
  // 종료가 23:59(끝까지)면 상한 없음 - 심야 회차는 '24:30'처럼 표기되므로 그대로 포함된다
  const noUpper = endTm === '2359';

  // 오늘(KST) 이후 날짜만 조회
  const today = kstNow().stamp.slice(0, 10);
  const validDates = dates.filter((d) => d >= today).map((d) => d.replaceAll('-', ''));

  let fetchFailed = false;
  for (const store of stores) {
    // 잔여석 있는 회차 수집 후 시간순 정렬, 날짜 헤더로 그룹핑
    const hits: { ymd8: string; tm4: string; text: string }[] = [];
    for (const ymd8 of validDates) {
      let rows: ScnRow[];
      try {
        rows = await getRows(store.code, ymd8);
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        log(`CGV 상영표 조회 실패 (${store.name} ${ymd8}): ${message}`);
        fetchFailed = true;
        reportCrawlFailure(watch, `CGV 상영표 조회 실패 (${store.name} ${ymd8}): ${message}`);
        continue;
      }
      const matched = rows.filter(
        (r) =>
          r.movNo === movieNo &&
          r.scnsrtTm >= startTm &&
          (noUpper || r.scnsrtTm <= endTm) &&
          (!config.imaxOnly ||
            r.movkndDsplNm.includes('IMAX') ||
            r.expoScnsNm.includes('IMAX')),
      );
      for (const r of matched) {
        const seats = Number(r.frSeatCnt);
        if (seats <= 0) continue; // 매진 회차는 나열하지 않음
        hits.push({
          ymd8,
          tm4: r.scnsrtTm,
          text: `${r.scnsrtTm.slice(0, 2)}:${r.scnsrtTm.slice(2, 4)} ${r.expoScnsNm} · ${seats}석`,
        });
      }
    }
    // 예매 가능한 회차가 없으면 알림 없음
    if (hits.length === 0) continue;
    hits.sort((a, b) => (a.ymd8 + a.tm4).localeCompare(b.ymd8 + b.tm4));

    const parts = [] as string[];
    let lastYmd = '';
    for (const hit of hits.slice(0, 8)) {
      if (hit.ymd8 !== lastYmd) {
        lastYmd = hit.ymd8;
        parts.push(dateHeader(hit.ymd8));
      }
      parts.push(hit.text);
    }
    if (hits.length > 8) parts.push(`외 ${hits.length - 8}회`);

    if (!watch.url) {
      db.prepare('UPDATE watches SET url = ? WHERE id = ? AND url IS NULL').run(BOOKING_URL, watch.id);
    }
    enqueueNotification({
      source: 'cgv',
      title: notificationTitle('cgv', `${watch.name} · ${store.name}`),
      body: parts.join('\n'),
      url: watch.url ?? BOOKING_URL,
      watchId: watch.id,
    });
    log(`CGV 상영 확인: "${watch.name}" ${store.name} (${hits.length}회)`);
  }
  if (!fetchFailed) clearCrawlFailure(watch);
}

export const cgvCrawler: Crawler = {
  async run(watches: WatchRow[], log: (msg: string) => void): Promise<void> {
    // 같은 회차의 (지점,날짜) 상영표는 1회만 조회
    const cache = new Map<string, Promise<ScnRow[]>>();
    const getRows = (siteNo: string, ymd8: string): Promise<ScnRow[]> => {
      const key = `${siteNo}:${ymd8}`;
      let entry = cache.get(key);
      if (!entry) {
        entry = fetchScreenings(siteNo, ymd8);
        cache.set(key, entry);
      }
      return entry;
    };

    const results = await Promise.allSettled(watches.map((watch) => runWatch(watch, getRows, log)));
    const failed = results.filter((r) => r.status === 'rejected');
    for (const f of failed) {
      log(`CGV 크롤링 실패: ${(f as PromiseRejectedResult).reason}`);
    }
    if (failed.length > 0 && failed.length === results.length) throw new Error('cgv_all_failed');
  },

  validateConfig(config: Record<string, unknown>): string | null {
    const c = config as CgvConfig;
    if (typeof c.movieNo !== 'string' || c.movieNo === '') return '영화를 선택해 주세요';
    if (!Array.isArray(c.stores) || c.stores.length === 0) return '지점을 1곳 이상 선택해 주세요';
    if (!Array.isArray(c.dates) || c.dates.length === 0) return '날짜를 1개 이상 선택해 주세요';
    if (typeof c.start === 'string' && typeof c.end === 'string' && c.start > c.end) {
      return '시작 시각이 종료 시각보다 늦어요';
    }
    return null;
  },
};
