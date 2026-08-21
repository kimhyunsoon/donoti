import { html, css, nothing, type TemplateResult } from 'lit';
import { customElement, state } from 'lit/decorators.js';
import { tokens, ui, sheet } from '../style.js';
import { icon } from '../icons.js';
import { SheetBase } from './sheet-base.js';

// 월요일 시작 요일 헤더
const DOW_LABELS = ['월', '화', '수', '목', '금', '토', '일'];

function ymd(year: number, month: number, date: number): string {
  return `${year}-${String(month + 1).padStart(2, '0')}-${String(date).padStart(2, '0')}`;
}

function todayYmd(): string {
  const t = new Date();
  return ymd(t.getFullYear(), t.getMonth(), t.getDate());
}

interface CalendarOptions {
  title?: string;
  // 이 날짜 이전은 선택 불가 ('YYYY-MM-DD')
  min?: string;
}

// 애플 스타일 월 달력 (오늘 표기, 날짜 탭 즉시 선택). show('2026-08-30') → 'YYYY-MM-DD' | null(취소)
@customElement('calendar-sheet')
export class CalendarSheet extends SheetBase {
  static styles = [
    tokens,
    ui,
    sheet,
    css`
      .cal-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 2px 4px 10px;
      }
      .cal-head b { font-size: 1.02rem; }
      .grid {
        display: grid;
        grid-template-columns: repeat(7, 1fr);
        row-gap: 4px;
        padding-bottom: 14px;
      }
      .dow {
        text-align: center;
        font-size: 0.78rem;
        font-weight: 700;
        color: var(--text-sub);
        padding: 6px 0;
      }
      /* 주말 색: 토=파랑, 일=빨강 (요일 헤더·날짜 공통) */
      .sat { color: var(--day-on); }
      .sun { color: var(--danger); }
      .day {
        position: relative;
        /* 셀 폭과 무관한 고정 원 (1fr 셀에 꽉 채우면 타원이 된다) */
        place-self: center;
        width: 42px;
        height: 42px;
        display: grid;
        place-items: center;
        background: none;
        border-radius: 50%;
        font-size: 0.95rem;
        font-weight: 600;
        color: var(--text);
        font-variant-numeric: tabular-nums;
      }
      /* 지난 날짜 - --border는 라이트에서 안 보일 만큼 옅어 반투명 text-sub 사용 */
      .day:disabled { color: color-mix(in srgb, var(--text-sub) 55%, transparent); }
      /* 오늘 표기: 숫자 아래 점만 (숫자 색은 그대로) */
      .day.today::after {
        content: '';
        position: absolute;
        bottom: 5px;
        left: 50%;
        transform: translateX(-50%);
        width: 4px;
        height: 4px;
        border-radius: 50%;
        background: var(--accent);
      }
      .day.selected {
        background: var(--accent);
        color: #fff;
      }
      .day.selected.today::after { background: #fff; }
    `,
  ];

  protected get sheetTitle(): string {
    return this.options.title ?? '날짜 선택';
  }

  @state() private viewYear = 2026;
  @state() private viewMonth = 0; // 0~11
  @state() private multiSelected = new Set<string>();
  private multi = false;
  private selected = '';
  private options: CalendarOptions = {};
  private picked: string | null = null;
  private multiPicked: string[] | null = null;
  private resolveFn?: (value: string | null) => void;
  private multiResolveFn?: (value: string[] | null) => void;

  show(current: string, options: CalendarOptions = {}): Promise<string | null> {
    this.multi = false;
    this.options = options;
    this.selected = current;
    const base = current !== '' ? new Date(`${current}T00:00`) : new Date();
    this.viewYear = base.getFullYear();
    this.viewMonth = base.getMonth();
    this.picked = null;
    this.openSheet();
    return new Promise((resolve) => {
      this.resolveFn = resolve;
    });
  }

  /** 여러 날짜 선택 (탭 토글 + 완료). 취소 시 null */
  showMulti(current: string[], options: CalendarOptions = {}): Promise<string[] | null> {
    this.multi = true;
    this.options = options;
    this.multiSelected = new Set(current);
    const first = current[0];
    const base = first ? new Date(`${first}T00:00`) : new Date();
    this.viewYear = base.getFullYear();
    this.viewMonth = base.getMonth();
    this.multiPicked = null;
    this.openSheet();
    return new Promise((resolve) => {
      this.multiResolveFn = resolve;
    });
  }

  protected onClosed(): void {
    if (this.multi) this.multiResolveFn?.(this.multiPicked);
    else this.resolveFn?.(this.picked);
  }

  private moveMonth(delta: number): void {
    const next = new Date(this.viewYear, this.viewMonth + delta, 1);
    this.viewYear = next.getFullYear();
    this.viewMonth = next.getMonth();
  }

  private pick(date: number): void {
    const value = ymd(this.viewYear, this.viewMonth, date);
    if (this.multi) {
      // 토글 후 완료 버튼으로 확정
      if (this.multiSelected.has(value)) this.multiSelected.delete(value);
      else this.multiSelected.add(value);
      this.multiSelected = new Set(this.multiSelected);
      return;
    }
    this.picked = value;
    this.requestClose();
  }

  private confirmMulti(): void {
    this.multiPicked = [...this.multiSelected].sort();
    this.requestClose();
  }

  protected renderBody(): TemplateResult {
    const first = new Date(this.viewYear, this.viewMonth, 1);
    const daysInMonth = new Date(this.viewYear, this.viewMonth + 1, 0).getDate();
    const today = todayYmd();
    const min = this.options.min ?? '';
    return html`
      <div class="cal-head">
        <button class="btn-icon" aria-label="이전 달" @click=${(): void => this.moveMonth(-1)}>
          ${icon('chevron-left', 20)}
        </button>
        <b>${this.viewYear}년 ${this.viewMonth + 1}월</b>
        <button class="btn-icon" aria-label="다음 달" @click=${(): void => this.moveMonth(1)}>
          ${icon('chevron-right', 20)}
        </button>
      </div>
      <div class="grid">
        ${DOW_LABELS.map(
          (d, i) => html`<span class="dow ${i === 5 ? 'sat' : ''} ${i === 6 ? 'sun' : ''}">${d}</span>`,
        )}
        ${Array.from({ length: (first.getDay() + 6) % 7 }, () => html`<span></span>`)}
        ${Array.from({ length: daysInMonth }, (_, i) => {
          const date = i + 1;
          const value = ymd(this.viewYear, this.viewMonth, date);
          const disabled = min !== '' && value < min;
          const on = this.multi ? this.multiSelected.has(value) : value === this.selected;
          const dow = new Date(this.viewYear, this.viewMonth, date).getDay();
          return html`
            <button
              class="day ${value === today ? 'today' : ''} ${on ? 'selected' : ''} ${dow === 6 ? 'sat' : ''} ${dow === 0 ? 'sun' : ''}"
              ?disabled=${disabled}
              @click=${(): void => this.pick(date)}
            >${date}</button>
          `;
        })}
      </div>
      ${this.multi
        ? html`
            <button class="btn-primary" @click=${(): void => this.confirmMulti()}>
              완료${this.multiSelected.size > 0 ? ` (${this.multiSelected.size}일)` : ''}
            </button>
          `
        : nothing}
    `;
  }
}
