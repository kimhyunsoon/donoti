// fetch 래퍼 - 세션 쿠키 포함, 401이면 로그인 화면으로
export async function api<T>(path: string, init?: RequestInit): Promise<T> {
  // Content-Type은 JSON 문자열 body가 있을 때만 지정 -
  // body 없는 POST/DELETE에 붙이면 Fastify가 400(빈 JSON body)을 반환하고,
  // FormData는 브라우저가 boundary 포함 헤더를 직접 설정해야 한다
  const jsonBody = typeof init?.body === 'string';
  const res = await fetch(path, {
    credentials: 'same-origin',
    ...init,
    headers: jsonBody ? { 'Content-Type': 'application/json' } : undefined,
  });
  if (res.status === 401 && !path.startsWith('/api/auth/login')) {
    // 로그인 화면으로 전환 - 뷰가 교체되므로 이 호출은 조용히 중단 (unhandled rejection 방지)
    location.hash = '#/login';
    return new Promise<never>(() => {});
  }
  if (!res.ok) throw new Error(`api_error_${res.status}`);
  return res.json() as Promise<T>;
}
