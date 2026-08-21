import { db } from '../db.js';
import { enqueueNotification } from '../notify.js';
import { notificationTitle } from '../catalog.js';
import { kstNow } from '../watch-schedule.js';
import { withRetry, reportCrawlFailure, clearCrawlFailure } from './retry.js';
import type { Crawler, WatchRow } from './index.js';

// 롯데시네마 크롤러 - CGV와 동일한 방식 (영화 × 지점 N × 날짜 N × 시간범위, 지점당 1알림)
// API: LCWS/Ticketing/TicketingData.aspx (multipart form 'paramList' JSON, 쿠키 불필요)
// BookingSeatCount = 잔여석 (UI '잔여석 144 / 164'와 대조 확인)

const API = 'https://www.lottecinema.co.kr/LCWS/Ticketing/TicketingData.aspx';
const BOOKING_URL = 'https://www.lottecinema.co.kr/NLCHS/Ticketing';
const DAY_LABELS = ['일', '월', '화', '수', '목', '금', '토'];

interface LotteConfig {
  movieNo?: string; // RepresentationMovieCode
  movieName?: string;
  stores?: { code: string; name: string }[]; // code = 'divisionCode|detailDivisionCode|cinemaID'
  dates?: string[]; // 'YYYY-MM-DD'
  start?: string; // 'HH:MM' (상영 시작시간 기준)
  end?: string;
}

async function callLotte<T>(label: string, params: Record<string, string>): Promise<T> {
  // 최대 3회 재시도
  return withRetry(label, async () => {
    const form = new FormData();
    form.append(
      'paramList',
      JSON.stringify({
        channelType: 'HO',
        osType: 'W',
        osVersion: 'Mozilla/5.0',
        multiLanguageID: 'KR',
        memberOnNo: '0',
        ...params,
      }),
    );
    const res = await fetch(API, {
      method: 'POST',
      body: form,
      headers: { 'User-Agent': 'Mozilla/5.0', Referer: BOOKING_URL },
      signal: AbortSignal.timeout(12_000),
    });
    if (!res.ok) throw new Error(`lotte_http_${res.status}`);
    return res.json() as Promise<T>;
  });
}

// ── 등록 폼용 목록 (영화·지점을 1콜로) ─────────────────────────

interface TicketingPage {
  Movies: { Movies: { Items: { RepresentationMovieCode: string; MovieNameKR: string }[] } };
  Cinemas: {
    Cinemas: {
      Items: { DivisionCode: number; DetailDivisionCode: string; CinemaID: number; CinemaNameKR: string }[];
    };
  };
  CinemaDivison: {
    AreaDivisions: { Items: { DivisionCode: number; DetailDivisionCode: string; GroupNameKR: string }[] };
  };
}

async function fetchTicketingPage(): Promise<TicketingPage> {
  return callLotte<TicketingPage>('롯데시네마 예매목록', { MethodName: 'GetTicketingPageTOBE' });
}

/** 현재 예매 가능한 영화 목록 */
export async function fetchLotteMovies(): Promise<{ code: string; name: string }[]> {
  const page = await fetchTicketingPage();
  return page.Movies.Movies.Items.map((m) => ({ code: m.RepresentationMovieCode, name: m.MovieNameKR }));
}

/** 전체 지점 목록 (지역명 포함). code는 'divisionCode|detailDivisionCode|cinemaID' */
export async function fetchLotteTheaters(): Promise<{ code: string; name: string; area: string }[]> {
  const page = await fetchTicketingPage();
  const areaNames = new Map(
    page.CinemaDivison.AreaDivisions.Items.map((a) => [`${a.DivisionCode}|${a.DetailDivisionCode}`, a.GroupNameKR]),
  );
  // DivisionCode 1 = 지역별 극장 (2는 특별관 그룹으로 중복)
  return page.Cinemas.Cinemas.Items.filter((c) => c.DivisionCode === 1).map((c) => ({
    code: `${c.DivisionCode}|${c.DetailDivisionCode}|${c.CinemaID}`,
    name: c.CinemaNameKR,
    area: areaNames.get(`${c.DivisionCode}|${c.DetailDivisionCode}`) ?? '',
  }));
}

// ── 상영표 ────────────────────────────────────────────────────

interface PlaySeqRow {
  RepresentationMovieCode: string;
  ScreenNameKR: string; // '1관'
  ScreenDivisionNameKR: string; // '일반' | '수퍼플렉스' 등
  FilmNameKR: string; // '2D' | '3D' 등
  StartTime: string; // '16:45'
  BookingSeatCount: number; // 잔여석
}

async function fetchPlaySequence(cinemaCode: string, date: string): Promise<PlaySeqRow[]> {
  const res = await callLotte<{ PlaySeqs: { Items: PlaySeqRow[] | null } }>(
    `롯데시네마 상영표 ${cinemaCode} ${date}`,
    {
      MethodName: 'GetPlaySequence',
      playDate: date,
      cinemaID: cinemaCode,
      representationMovieCode: '',
    },
  );
  return res.PlaySeqs.Items ?? [];
}

// 'YYYY-MM-DD' → '8/21 (금)'
function dateHeader(date: string): string {
  const d = new Date(`${date}T00:00`);
  return `${d.getMonth() + 1}/${d.getDate()} (${DAY_LABELS[d.getDay()]})`;
}

// 관 표기: '1관' / '수퍼플렉스관' 등 특별관·특수 포맷은 함께
function screenLabel(row: PlaySeqRow): string {
  let label = row.ScreenNameKR;
  if (row.ScreenDivisionNameKR && row.ScreenDivisionNameKR !== '일반') label += ` (${row.ScreenDivisionNameKR})`;
  if (row.FilmNameKR && row.FilmNameKR !== '2D') label += ` ${row.FilmNameKR}`;
  return label;
}

// watch 하나 크롤링. 지점당 1건 알림 (CGV와 동일 형식)
async function runWatch(
  watch: WatchRow,
  getRows: (cinemaCode: string, date: string) => Promise<PlaySeqRow[]>,
  log: (msg: string) => void,
): Promise<void> {
  const config = JSON.parse(watch.config) as LotteConfig;
  const { movieNo, stores, dates } = config;
  if (!movieNo || !stores?.length || !dates?.length) {
    log(`롯데시네마 알림 설정 없음: watch ${watch.id} "${watch.name}" - 건너뜀`);
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
      let rows: PlaySeqRow[];
      try {
        rows = await getRows(store.code, date);
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        log(`롯데시네마 상영표 조회 실패 (${store.name} ${date}): ${message}`);
        fetchFailed = true;
        reportCrawlFailure(watch, `롯데시네마 상영표 조회 실패 (${store.name} ${date}): ${message}`);
        continue;
      }
      const matched = rows.filter(
        (r) =>
          r.RepresentationMovieCode === movieNo &&
          r.StartTime >= start &&
          (noUpper || r.StartTime <= end),
      );
      for (const r of matched) {
        if (r.BookingSeatCount <= 0) continue; // 매진 회차는 나열하지 않음
        hits.push({ date, tm: r.StartTime, text: `${r.StartTime} ${screenLabel(r)} · ${r.BookingSeatCount}석` });
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
      source: 'lotte',
      title: notificationTitle('lotte', `${watch.name} · ${store.name}`),
      body: parts.join('\n'),
      url: watch.url ?? BOOKING_URL,
      watchId: watch.id,
    });
    log(`롯데시네마 상영 확인: "${watch.name}" ${store.name} (${hits.length}회)`);
  }
  if (!fetchFailed) clearCrawlFailure(watch);
}

export const lotteCrawler: Crawler = {
  async run(watches: WatchRow[], log: (msg: string) => void): Promise<void> {
    // 같은 회차의 (지점,날짜) 상영표는 1회만 조회
    const cache = new Map<string, Promise<PlaySeqRow[]>>();
    const getRows = (cinemaCode: string, date: string): Promise<PlaySeqRow[]> => {
      const key = `${cinemaCode}:${date}`;
      let entry = cache.get(key);
      if (!entry) {
        entry = fetchPlaySequence(cinemaCode, date);
        cache.set(key, entry);
      }
      return entry;
    };

    const results = await Promise.allSettled(watches.map((watch) => runWatch(watch, getRows, log)));
    const failed = results.filter((r) => r.status === 'rejected');
    for (const f of failed) {
      log(`롯데시네마 크롤링 실패: ${(f as PromiseRejectedResult).reason}`);
    }
    if (failed.length > 0 && failed.length === results.length) throw new Error('lotte_all_failed');
  },

  validateConfig(config: Record<string, unknown>): string | null {
    const c = config as LotteConfig;
    if (typeof c.movieNo !== 'string' || c.movieNo === '') return '영화를 선택해 주세요';
    if (!Array.isArray(c.stores) || c.stores.length === 0) return '지점을 1곳 이상 선택해 주세요';
    if (!Array.isArray(c.dates) || c.dates.length === 0) return '날짜를 1개 이상 선택해 주세요';
    if (typeof c.start === 'string' && typeof c.end === 'string' && c.start > c.end) {
      return '시작 시각이 종료 시각보다 늦어요';
    }
    return null;
  },
};
