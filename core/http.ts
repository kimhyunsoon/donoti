// 크롤링용 HTTP 헬퍼 (내장 fetch 기반)

const DEFAULT_HEADERS: Record<string, string> = {
  "User-Agent":
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0 Safari/537.36",
  "Accept-Language": "ko-KR,ko;q=0.9,en;q=0.8",
};

export interface FetchOptions {
  headers?: Record<string, string>;
  // 타임아웃 (ms, 기본 15초)
  timeoutMs?: number;
}

async function doFetch(url: string, options: FetchOptions): Promise<Response> {
  const res = await fetch(url, {
    headers: { ...DEFAULT_HEADERS, ...options.headers },
    signal: AbortSignal.timeout(options.timeoutMs ?? 15_000),
  });
  if (!res.ok) {
    throw new Error(`HTTP ${res.status} ${res.statusText}: ${url}`);
  }
  return res;
}

// HTML/텍스트 페이지 가져오기
export async function fetchText(url: string, options: FetchOptions = {}): Promise<string> {
  const res = await doFetch(url, options);
  return res.text();
}

/**
 * JSON API 가져오기
 * @param url 요청 URL
 * @param options 헤더·타임아웃 옵션
 */
export async function fetchJson<T = unknown>(url: string, options: FetchOptions = {}): Promise<T> {
  const res = await doFetch(url, options);
  return (await res.json()) as T;
}
