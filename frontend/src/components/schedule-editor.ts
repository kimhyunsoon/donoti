import { LitElement, html, css, nothing, type TemplateResult } from 'lit';
import { customElement, property } from 'lit/decorators.js';
import { tokens, ui } from '../style.js';
import { icon } from '../icons.js';
import { DAY_LABELS, defaultRule, nowHHMM, type ScheduleRule } from '../schedule.js';
import '../sheets/time-wheel-sheet.js';
import type { TimeWheelSheet } from '../sheets/time-wheel-sheet.js';

// iOS 미리알림처럼 규칙(요일 + 시각/간격)을 여러 개 조합하는 에디터.
// 시각은 하단 휠 시트(24시간제)로 고른다. 수정 시 'change' CustomEvent<ScheduleRule[]> 발행
@customElement('schedule-editor')
export class ScheduleEditor extends LitElement {
  static styles = [
    tokens,
    ui,
    css`
      :host { display: flex; flex-direction: column; gap: 10px; }
      .rule {
        background: var(--bg);
        border-radius: 14px;
        padding: 6px 14px 12px;
        display: flex;
        flex-direction: column;
      }
      .presets { display: flex; align-items: center; }
      .presets button { padding: 6px 9px; font-size: 0.82rem; }
      /* 현재 요일 선택과 일치하는 프리셋 하이라이트 */
      .presets .btn-ghost.on { color: var(--day-on); font-weight: 700; }
      .days { display: flex; gap: 5px; justify-content: space-between; padding: 2px 0 12px; }
      .days button {
        width: 34px;
        height: 34px;
        border-radius: 50%;
        background: var(--surface);
        color: var(--text-sub);
        font-size: 0.82rem;
        font-weight: 600;
        flex-shrink: 0;
      }
      .days button.on { background: var(--day-on); color: #fff; font-weight: 700; }
      /* 설정 화면의 테마 라디오와 동일한 세그먼티드 (ui 공통 스타일 그대로) */
      /* 방식 선택 - 설정 화면 테마 라디오처럼 행 우측에 고정폭 세그먼트 */
      .segmented { width: 190px; }
      /* 시각 버튼 - 탭하면 하단 휠 시트 */
      .time-btn {
        padding: 8px 14px;
        border-radius: 10px;
        background: var(--surface);
        font-weight: 600;
        font-size: 0.92rem;
        font-variant-numeric: tabular-nums;
        color: var(--text);
      }
      .time-btn:active { background: var(--surface-weak); }
      .btn-icon.small { width: 30px; height: 30px; color: var(--text-sub); margin-right: -6px; }
      .add {
        align-self: flex-start;
        display: flex;
        align-items: center;
        gap: 4px;
        font-size: 0.85rem;
        padding-left: 0;
      }
    `,
  ];

  @property({ attribute: false }) rules: ScheduleRule[] = [defaultRule()];

  private emit(): void {
    this.dispatchEvent(new CustomEvent<ScheduleRule[]>('change', { detail: this.rules }));
    this.requestUpdate();
  }

  private toggleDay(rule: ScheduleRule, day: number): void {
    rule.days = rule.days.includes(day)
      ? rule.days.filter((d) => d !== day)
      : [...rule.days, day];
    this.emit();
  }

  private setDays(rule: ScheduleRule, days: number[]): void {
    rule.days = days;
    this.emit();
  }

  // 현재 선택 요일이 프리셋과 정확히 일치하는지
  private daysEqual(days: number[], preset: number[]): boolean {
    return days.length === preset.length && preset.every((d) => days.includes(d));
  }

  private setMode(rule: ScheduleRule, mode: ScheduleRule['mode']): void {
    rule.mode = mode;
    // 특정 시각의 기본값은 현재 시각
    if (mode === 'times' && !rule.times?.length) rule.times = [nowHHMM()];
    if (mode === 'interval') {
      rule.start ??= '09:00';
      rule.end ??= '21:00';
      rule.every ??= 30;
    }
    this.emit();
  }

  // 하단 휠 시트로 시각 선택 후 반영 (취소 시 무시)
  private async pickTime(current: string, title: string, apply: (next: string) => void): Promise<void> {
    const wheel = this.renderRoot.querySelector('time-wheel-sheet') as TimeWheelSheet;
    const picked = await wheel.show(current, { title });
    if (picked !== null) {
      apply(picked);
      this.emit();
    }
  }

  private timeButton(value: string, title: string, apply: (next: string) => void): TemplateResult {
    return html`
      <button class="time-btn" @click=${(): void => void this.pickTime(value, title, apply)}>
        ${value}
      </button>
    `;
  }

  private renderRule(rule: ScheduleRule, index: number): TemplateResult {
    return html`
      <div class="rule">
        <div class="frow">
          <span class="lbl">요일</span>
          <div class="presets">
            <button class="btn-ghost ${this.daysEqual(rule.days, [0, 1, 2, 3, 4, 5, 6]) ? 'on' : ''}"
              @click=${(): void => this.setDays(rule, [0, 1, 2, 3, 4, 5, 6])}>매일</button>
            <button class="btn-ghost ${this.daysEqual(rule.days, [1, 2, 3, 4, 5]) ? 'on' : ''}"
              @click=${(): void => this.setDays(rule, [1, 2, 3, 4, 5])}>평일</button>
            <button class="btn-ghost ${this.daysEqual(rule.days, [0, 6]) ? 'on' : ''}"
              @click=${(): void => this.setDays(rule, [0, 6])}>주말</button>
            ${this.rules.length > 1
              ? html`
                  <button class="btn-icon small" aria-label="조건 삭제"
                    @click=${(): void => { this.rules.splice(index, 1); this.emit(); }}>
                    ${icon('x', 16)}
                  </button>
                `
              : nothing}
          </div>
        </div>

        <div class="days">
          ${DAY_LABELS.map(
            (label, day) => html`
              <button class=${rule.days.includes(day) ? 'on' : ''}
                @click=${(): void => this.toggleDay(rule, day)}>${label}</button>
            `,
          )}
        </div>

        <div class="frow">
          <span class="lbl">방식</span>
          <div class="segmented">
            <button class=${rule.mode === 'interval' ? 'on' : ''}
              @click=${(): void => this.setMode(rule, 'interval')}>간격 반복</button>
            <button class=${rule.mode === 'times' ? 'on' : ''}
              @click=${(): void => this.setMode(rule, 'times')}>특정 시각</button>
          </div>
        </div>

        ${rule.mode === 'interval' ? this.renderInterval(rule) : this.renderTimes(rule)}
      </div>
    `;
  }

  private renderInterval(rule: ScheduleRule): TemplateResult {
    return html`
      <div class="frow">
        <span class="lbl">시간대</span>
        <span class="ctl">
          ${this.timeButton(rule.start ?? '09:00', '시작 시각', (next): void => { rule.start = next; })}
          <span style="color:var(--text-sub)">~</span>
          ${this.timeButton(rule.end ?? '21:00', '종료 시각', (next): void => { rule.end = next; })}
        </span>
      </div>
      <div class="frow">
        <span class="lbl">간격</span>
        <span class="ctl">
          <button class="time-btn" @click=${(): void => void this.pickEvery(rule)}>
            ${rule.every ?? 30}분마다
          </button>
        </span>
      </div>
    `;
  }

  // 반복 간격 선택 - 5분~120분, 5분 단위 휠
  private async pickEvery(rule: ScheduleRule): Promise<void> {
    const wheel = this.renderRoot.querySelector('time-wheel-sheet') as TimeWheelSheet;
    const items = Array.from({ length: 24 }, (_, i) => `${(i + 1) * 5}분`);
    const picked = await wheel.showList(`${rule.every ?? 30}분`, items, '반복 간격');
    if (picked !== null) {
      rule.every = parseInt(picked, 10);
      this.emit();
    }
  }

  private renderTimes(rule: ScheduleRule): TemplateResult {
    const times = rule.times ?? [];
    return html`
      ${times.map(
        (time, i) => html`
          <div class="frow">
            <span class="lbl">${i === 0 ? '시각' : ''}</span>
            <span class="ctl">
              ${this.timeButton(time, '알림 시각', (next): void => { times[i] = next; })}
              ${times.length > 1
                ? html`
                    <button class="btn-icon small" aria-label="시각 삭제"
                      @click=${(): void => { times.splice(i, 1); this.emit(); }}>
                      ${icon('x', 16)}
                    </button>
                  `
                : nothing}
            </span>
          </div>
        `,
      )}
      <button class="btn-ghost add"
        @click=${(): void => { times.push(nowHHMM()); rule.times = times; this.emit(); }}>
        ${icon('plus', 14)} 시각 추가
      </button>
    `;
  }

  render(): TemplateResult {
    return html`
      ${this.rules.map((rule, i) => this.renderRule(rule, i))}
      <button class="btn-ghost add"
        @click=${(): void => { this.rules.push(defaultRule()); this.emit(); }}>
        ${icon('plus', 14)} 조건 추가
      </button>
      <time-wheel-sheet></time-wheel-sheet>
    `;
  }
}
