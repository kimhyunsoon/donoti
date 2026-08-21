// 감시 알림 스케줄 모델 - KST 24시간제 'HH:MM' (backend/src/watch-schedule.ts와 동기 유지)

export interface ScheduleRule {
  // 요일 0(일)~6(토)
  days: number[];
  mode: 'times' | 'interval';
  // mode=times: 실행 시각 목록
  times?: string[];
  // mode=interval: start~end 사이 every분마다
  start?: string;
  end?: string;
  every?: number;
}

export const DAY_LABELS = ['일', '월', '화', '수', '목', '금', '토'];

// 현재 시각 'HH:MM' (특정 시각 기본값용)
export function nowHHMM(): string {
  const t = new Date();
  return `${String(t.getHours()).padStart(2, '0')}:${String(t.getMinutes()).padStart(2, '0')}`;
}

// 새 알림의 기본 규칙
export function defaultRule(): ScheduleRule {
  return { days: [0, 1, 2, 3, 4, 5, 6], mode: 'interval', start: '09:00', end: '21:00', every: 30 };
}

// 요일 배열 → '매일' | '평일' | '주말' | '월·수·금'
export function daysLabel(days: number[]): string {
  const set = [...new Set(days)];
  if (set.length === 7) return '매일';
  const sorted = [...set].sort((a, b) => a - b);
  if (sorted.join() === '1,2,3,4,5') return '평일';
  if (sorted.join() === '0,6') return '주말';
  // 월화수목금토일 순으로 표기
  return [...set]
    .sort((a, b) => ((a + 6) % 7) - ((b + 6) % 7))
    .map((d) => DAY_LABELS[d])
    .join('·');
}

// 규칙 하나 → '평일 08:00~10:00 3분마다' | '월·수·금 16:00'
function summarizeRule(rule: ScheduleRule): string {
  const days = daysLabel(rule.days);
  if (rule.mode === 'times') return `${days} ${[...(rule.times ?? [])].sort().join(', ')}`;
  // 종일이면 시간 범위 생략 → '매일 1분마다'
  const range = rule.start === '00:00' && rule.end === '23:59' ? '' : `${rule.start}~${rule.end} `;
  return `${days} ${range}${rule.every}분마다`;
}

/** 스케줄 전체 요약 문자열 (목록 표시용) */
export function summarizeSchedule(rules: ScheduleRule[]): string {
  return rules.map(summarizeRule).join(' / ');
}

// 'HH:MM' → 자정 기준 분
function toMinutes(hhmm: string): number {
  return Number(hhmm.slice(0, 2)) * 60 + Number(hhmm.slice(3, 5));
}

/** 다음 실행 시각 (클라이언트 로컬=KST 기준). 규칙이 비정상이면 null */
export function nextRunAt(rules: ScheduleRule[], from = new Date()): Date | null {
  for (let offset = 0; offset <= 7; offset++) {
    const date = new Date(from);
    date.setDate(date.getDate() + offset);
    const day = date.getDay();
    // 오늘은 현재 분 이후만 (현재 분은 이미 실행된 회차)
    const nowMin = offset === 0 ? from.getHours() * 60 + from.getMinutes() : -1;
    let best: number | null = null;
    for (const rule of rules) {
      if (!rule.days.includes(day)) continue;
      if (rule.mode === 'times') {
        for (const t of rule.times ?? []) {
          const m = toMinutes(t);
          if (m > nowMin && (best === null || m < best)) best = m;
        }
      } else {
        const start = toMinutes(rule.start ?? '00:00');
        const end = toMinutes(rule.end ?? '23:59');
        const every = rule.every ?? 1;
        const k = Math.max(0, Math.ceil((nowMin + 1 - start) / every));
        const m = start + k * every;
        if (m <= end && (best === null || m < best)) best = m;
      }
    }
    if (best !== null) {
      date.setHours(Math.floor(best / 60), best % 60, 0, 0);
      return date;
    }
  }
  return null;
}

/** 남은 시간 간단 표기: 'n분 후' | 'n시간 후' | 'n일 후' */
export function formatUntil(target: Date, from = new Date()): string {
  const min = Math.max(1, Math.round((target.getTime() - from.getTime()) / 60_000));
  if (min < 60) return `${min}분 후`;
  const hour = Math.floor(min / 60);
  if (hour < 24) return `${hour}시간 후`;
  return `${Math.floor(hour / 24)}일 후`;
}
