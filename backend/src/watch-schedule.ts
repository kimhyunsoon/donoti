// 감시 알림 스케줄 모델 - 모든 시각은 KST 24시간제 'HH:MM' (frontend/src/schedule.ts와 동기 유지)

// 실행 규칙 하나 - 여러 규칙을 배열로 조합해 iOS 미리알림처럼 자유롭게 설정
export interface ScheduleRule {
  // 요일 0(일)~6(토), 최소 1개
  days: number[];
  mode: 'times' | 'interval';
  // mode=times: 실행 시각 목록
  times?: string[];
  // mode=interval: start~end 사이 every분마다 실행 (start 시각부터 기산)
  start?: string;
  end?: string;
  every?: number;
}

const TIME_RE = /^([01]\d|2[0-3]):[0-5]\d$/;

// 'HH:MM' → 자정 기준 분
function toMinutes(hhmm: string): number {
  return Number(hhmm.slice(0, 2)) * 60 + Number(hhmm.slice(3, 5));
}

/** 현재 KST 시각. KST는 UTC+9 고정(서머타임 없음) */
export function kstNow(): { day: number; hhmm: string; stamp: string } {
  const t = new Date(Date.now() + 9 * 3_600_000);
  const pad = (n: number): string => String(n).padStart(2, '0');
  const hhmm = `${pad(t.getUTCHours())}:${pad(t.getUTCMinutes())}`;
  return {
    day: t.getUTCDay(),
    hhmm,
    // ends_at('YYYY-MM-DD HH:MM')과 문자열 비교 가능한 형식
    stamp: `${t.getUTCFullYear()}-${pad(t.getUTCMonth() + 1)}-${pad(t.getUTCDate())} ${hhmm}`,
  };
}

/**
 * 스케줄 JSON 검증
 * @returns 문제 없으면 null, 있으면 오류 메시지
 */
export function validateSchedule(value: unknown): string | null {
  if (!Array.isArray(value) || value.length === 0) return '실행 조건이 최소 1개 필요합니다';
  for (const rule of value as ScheduleRule[]) {
    if (typeof rule !== 'object' || rule === null) return '잘못된 조건 형식입니다';
    if (
      !Array.isArray(rule.days) ||
      rule.days.length === 0 ||
      rule.days.some((d) => !Number.isInteger(d) || d < 0 || d > 6)
    ) {
      return '요일을 1개 이상 선택해야 합니다';
    }
    if (rule.mode === 'times') {
      if (
        !Array.isArray(rule.times) ||
        rule.times.length === 0 ||
        rule.times.some((t) => typeof t !== 'string' || !TIME_RE.test(t))
      ) {
        return '실행 시각을 1개 이상 지정해야 합니다';
      }
    } else if (rule.mode === 'interval') {
      if (
        typeof rule.start !== 'string' ||
        !TIME_RE.test(rule.start) ||
        typeof rule.end !== 'string' ||
        !TIME_RE.test(rule.end)
      ) {
        return '시작·종료 시각이 올바르지 않습니다';
      }
      if (toMinutes(rule.start) > toMinutes(rule.end)) return '시작 시각이 종료 시각보다 늦습니다';
      // 간격은 5분~120분, 5분 단위
      if (
        !Number.isInteger(rule.every) ||
        rule.every! < 5 ||
        rule.every! > 120 ||
        rule.every! % 5 !== 0
      ) {
        return '실행 간격은 5분~120분 사이 5분 단위여야 합니다';
      }
    } else {
      return '잘못된 실행 방식입니다';
    }
  }
  return null;
}

/** 현재 KST 시각(요일·'HH:MM')이 규칙 중 하나와 일치하는지 - 일치 시점이 크롤링 시작 타이밍 */
export function matchesSchedule(rules: ScheduleRule[], day: number, hhmm: string): boolean {
  const now = toMinutes(hhmm);
  return rules.some((rule) => {
    if (!rule.days.includes(day)) return false;
    if (rule.mode === 'times') return rule.times!.includes(hhmm);
    const start = toMinutes(rule.start!);
    if (now < start || now > toMinutes(rule.end!)) return false;
    return (now - start) % rule.every! === 0;
  });
}
