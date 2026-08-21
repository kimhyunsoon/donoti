import { db } from '../db.js';
import { enqueueNotification } from '../notify.js';
import { notificationTitle } from '../catalog.js';
import { kstNow } from '../watch-schedule.js';
import { withRetry, reportCrawlFailure, clearCrawlFailure } from './retry.js';
import type { Crawler, WatchRow } from './index.js';

// 메가박스 크롤러 - CGV·롯데와 동일한 방식 (영화 × 지점 N × 날짜 N × 시간범위, 지점당 1건)
// API: on/oh/ohb/SimpleBooking/selectBokdList.do (JSON POST + X-Requested-With, 쿠키 불필요)
// restSeatCnt = 잔여석. 상영표 조회 시 brchNo와 함께 areaCd가 필요해 지점 code는 'brchNo|areaCd'

const API = 'https://megabox.co.kr/on/oh/ohb/SimpleBooking/selectBokdList.do';
const BOOKING_URL = 'https://megabox.co.kr/booking';
const DAY_LABELS = ['일', '월', '화', '수', '목', '금', '토'];

interface MegaboxConfig {
  movieNo?: string; // rpstMovieNo
  movieName?: string;
  stores?: { code: string; name: string }[]; // code = 'brchNo|areaCd'
  dates?: string[]; // 'YYYY-MM-DD'
  start?: string; // 'HH:MM' (상영 시작시간 기준)
  end?: string;
}

async function callMegabox<T>(label: string, body: Record<string, unknown>): Promise<T> {
  // 최대 3회 재시도
  return withRetry(label, async () => {
    const res = await fetch(API, {
      method: 'POST',
      body: JSON.stringify(body),
      headers: {
        'User-Agent': 'Mozilla/5.0',
        'Content-Type': 'application/json; charset=UTF-8',
        'X-Requested-With': 'XMLHttpRequest',
        Referer: BOOKING_URL,
      },
      signal: AbortSignal.timeout(12_000),
    });
    if (!res.ok) throw new Error(`megabox_http_${res.status}`);
    return res.json() as Promise<T>;
  });
}

// ── 등록 폼용 목록 (영화·지점을 1콜로) ─────────────────────────

interface BokdListRes {
  movieList: { movieNo: string; movieNm: string }[] | null;
  areaBrchList: { brchNo: string; brchNm: string; areaCd: string; areaCdNm: string }[] | null;
  movieFormList: FormRow[] | null;
}

async function fetchBookingPage(): Promise<BokdListRes> {
  const today = kstNow().stamp.slice(0, 10).replaceAll('-', '');
  return callMegabox<BokdListRes>('메가박스 예매목록', {
    playDe: today,
    incomeMovieNo: '',
    onLoad: 'Y',
    sellChnlCd: '',
    incomeTheabKindCd: '',
    incomeBrchNo1: '',
    incomePlayDe: '',
  });
}

/** 현재 예매 가능한 영화 목록 */
export async function fetchMegaboxMovies(): Promise<{ code: string; name: string }[]> {
  const page = await fetchBookingPage();
  return (page.movieList ?? []).map((m) => ({ code: m.movieNo, name: m.movieNm }));
}

/** 전체 지점 목록 (지역명 포함). code는 'brchNo|areaCd' */
export async function fetchMegaboxTheaters(): Promise<{ code: string; name: string; area: string }[]> {
  const page = await fetchBookingPage();
  return (page.areaBrchList ?? []).map((b) => ({
    code: `${b.brchNo}|${b.areaCd}`,
    name: b.brchNm,
    area: b.areaCdNm,
  }));
}

// ── 상영표 ────────────────────────────────────────────────────

interface FormRow {
  rpstMovieNo: string;
  theabExpoNm: string; // '9층 르 리클라이너 1관[Laser]'
  playKindNm: string; // '2D&#40;자막&#41;' 등 (HTML 엔티티 포함)
  playStartTime: string; // '16:50'
  restSeatCnt: number; // 잔여석
}

async function fetchScreenings(movieNo: string, storeCode: string, ymd8: string): Promise<FormRow[]> {
  const [brchNo, areaCd] = storeCode.split('|');
  const res = await callMegabox<BokdListRes>(`메가박스 상영표 ${storeCode} ${ymd8}`, {
    arrMovieNo: movieNo,
    playDe: ymd8,
    brchNoListCnt: 1,
    brchNo1: brchNo,
    areaCd1: areaCd,
    spclbYn1: 'N',
    theabKindCd1: '10',
    movieNo1: movieNo,
    sellChnlCd: '',
  });
  return res.movieFormList ?? [];
}

// 'YYYY-MM-DD' → '8/21 (금)'
function dateHeader(date: string): string {
  const d = new Date(`${date}T00:00`);
  return `${d.getMonth() + 1}/${d.getDate()} (${DAY_LABELS[d.getDay()]})`;
}

// 관 표기: 상영관 이름 + 2D가 아닌 상영 포맷
function screenLabel(row: FormRow): string {
  const kind = row.playKindNm.replaceAll('&#40;', '(').replaceAll('&#41;', ')');
  return kind.startsWith('2D') ? row.theabExpoNm : `${row.theabExpoNm} ${kind}`;
}

// watch 하나 크롤링. 지점당 1건 알림 (CGV·롯데와 동일 형식)
async function runWatch(
  watch: WatchRow,
  getRows: (movieNo: string, storeCode: string, ymd8: string) => Promise<FormRow[]>,
  log: (msg: string) => void,
): Promise<void> {
  const config = JSON.parse(watch.config) as MegaboxConfig;
  const { movieNo, stores, dates } = config;
  if (!movieNo || !stores?.length || !dates?.length) {
    log(`메가박스 알림 설정 없음: watch ${watch.id} "${watch.name}" - 건너뜀`);
    return;
  }
  const start = config.start ?? '00:00';
  const end = config.end ?? '23:59';
  // 종료가 23:59(끝까지)면 상한 없음 - 심야 회차는 '24:30'처럼 표기되므로 그대로 포함된다
  const noUpper = end === '23:59';

  const today = kstNow().stamp.slice(0, 10);
  const validDates = dates.filter((d) => d >= today);

  let fetchFailed = false;
  for (const store of stores) {
    const hits: { date: string; tm: string; text: string }[] = [];
    for (const date of validDates) {
      let rows: FormRow[];
      try {
        rows = await getRows(movieNo, store.code, date.replaceAll('-', ''));
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        log(`메가박스 상영표 조회 실패 (${store.name} ${date}): ${message}`);
        fetchFailed = true;
        reportCrawlFailure(watch, `메가박스 상영표 조회 실패 (${store.name} ${date}): ${message}`);
        continue;
      }
      const matched = rows.filter(
        (r) =>
          r.rpstMovieNo === movieNo &&
          r.playStartTime >= start &&
          (noUpper || r.playStartTime <= end),
      );
      for (const r of matched) {
        if (r.restSeatCnt <= 0) continue; // 매진 회차는 나열하지 않음
        hits.push({
          date,
          tm: r.playStartTime,
          text: `${r.playStartTime} ${screenLabel(r)} · ${r.restSeatCnt}석`,
        });
      }
    }
    // 예매 가능한 회차가 없으면 알림 없음
    if (hits.length === 0) continue;
    hits.sort((a, b) => (a.date + a.tm).localeCompare(b.date + b.tm));

    const parts = [] as string[];
    let lastDate = '';
    for (const hit of hits.slice(0, 8)) {
      if (hit.date !== lastDate) {
        lastDate = hit.date;
        parts.push(dateHeader(hit.date));
      }
      parts.push(hit.text);
    }
    if (hits.length > 8) parts.push(`외 ${hits.length - 8}회`);

    if (!watch.url) {
      db.prepare('UPDATE watches SET url = ? WHERE id = ? AND url IS NULL').run(BOOKING_URL, watch.id);
    }
    enqueueNotification({
      source: 'megabox',
      title: notificationTitle('megabox', `${watch.name} · ${store.name}`),
      body: parts.join('\n'),
      url: watch.url ?? BOOKING_URL,
      watchId: watch.id,
    });
    log(`메가박스 상영 확인: "${watch.name}" ${store.name} (${hits.length}회)`);
  }
  if (!fetchFailed) clearCrawlFailure(watch);
}

export const megaboxCrawler: Crawler = {
  async run(watches: WatchRow[], log: (msg: string) => void): Promise<void> {
    // 같은 회차의 (영화,지점,날짜) 상영표는 1회만 조회
    const cache = new Map<string, Promise<FormRow[]>>();
    const getRows = (movieNo: string, storeCode: string, ymd8: string): Promise<FormRow[]> => {
      const key = `${movieNo}:${storeCode}:${ymd8}`;
      let entry = cache.get(key);
      if (!entry) {
        entry = fetchScreenings(movieNo, storeCode, ymd8);
        cache.set(key, entry);
      }
      return entry;
    };

    const results = await Promise.allSettled(watches.map((watch) => runWatch(watch, getRows, log)));
    const failed = results.filter((r) => r.status === 'rejected');
    for (const f of failed) {
      log(`메가박스 크롤링 실패: ${(f as PromiseRejectedResult).reason}`);
    }
    if (failed.length > 0 && failed.length === results.length) throw new Error('megabox_all_failed');
  },

  validateConfig(config: Record<string, unknown>): string | null {
    const c = config as MegaboxConfig;
    if (typeof c.movieNo !== 'string' || c.movieNo === '') return '영화를 선택해 주세요';
    if (!Array.isArray(c.stores) || c.stores.length === 0) return '지점을 1곳 이상 선택해 주세요';
    if (!Array.isArray(c.dates) || c.dates.length === 0) return '날짜를 1개 이상 선택해 주세요';
    if (typeof c.start === 'string' && typeof c.end === 'string' && c.start > c.end) {
      return '시작 시각이 종료 시각보다 늦어요';
    }
    return null;
  },
};
