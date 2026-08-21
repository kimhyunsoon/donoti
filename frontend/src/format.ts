// 날짜·시간 표시 유틸

/** SQLite UTC datetime('YYYY-MM-DD HH:MM:SS') → 상대 시간 문자열 */
export function relativeTime(utc: string): string {
  const diff = Date.now() - new Date(`${utc.replace(' ', 'T')}Z`).getTime();
  const min = Math.floor(diff / 60_000);
  if (min < 1) return '방금 전';
  if (min < 60) return `${min}분 전`;
  const hour = Math.floor(min / 60);
  if (hour < 24) return `${hour}시간 전`;
  const day = Math.floor(hour / 24);
  if (day < 7) return `${day}일 전`;
  return new Date(`${utc.replace(' ', 'T')}Z`).toLocaleDateString('ko-KR', {
    month: 'long',
    day: 'numeric',
  });
}

/** KST 종료 시점('YYYY-MM-DD HH:MM') → '8월 30일 14시까지' */
export function formatEndsAt(kst: string): string {
  const month = Number(kst.slice(5, 7));
  const date = Number(kst.slice(8, 10));
  const hour = Number(kst.slice(11, 13));
  return `${month}월 ${date}일 ${hour}시까지`;
}
