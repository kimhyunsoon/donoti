import { html, css, nothing, type TemplateResult } from 'lit';
import { customElement } from 'lit/decorators.js';
import { tokens, ui, sheet } from '../style.js';
import { SheetBase } from './sheet-base.js';

const ITEM_H = 40;
const HOURS = Array.from({ length: 24 }, (_, i) => i);
const MINUTES = Array.from({ length: 60 }, (_, i) => i);

interface TimeWheelOptions {
  title?: string;
  // true면 분 휠 없이 시(時)만 선택 (결과 분은 00)
  hourOnly?: boolean;
}

// 애플 피커 스타일 24시간제 시/분 휠. show('08:30') → 'HH:MM' | null(취소)
@customElement('time-wheel-sheet')
export class TimeWheelSheet extends SheetBase {
  static styles = [
    tokens,
    ui,
    sheet,
    css`
      .wheels {
        position: relative;
        display: flex;
        justify-content: center;
        gap: 8px;
        padding: 6px 0 14px;
      }
      .wheel {
        height: ${ITEM_H * 5}px;
        width: 84px;
        overflow-y: auto;
        scroll-snap-type: y mandatory;
        scrollbar-width: none;
        /* 인디케이터(absolute)보다 위에 그려 선택 값이 가려지지 않게 */
        position: relative;
        z-index: 1;
        cursor: grab;
      }
      .wheel::-webkit-scrollbar { display: none; }
      /* 마우스 드래그 중에는 스냅을 끄고 자유 스크롤 (놓으면 JS가 스냅) */
      .wheel.dragging { scroll-snap-type: none; cursor: grabbing; }
      .wheel .pad { height: ${ITEM_H * 2}px; }
      .wheel .item {
        height: ${ITEM_H}px;
        scroll-snap-align: center;
        display: grid;
        place-items: center;
        font-size: 1.25rem;
        font-variant-numeric: tabular-nums;
      }
      .indicator {
        position: absolute;
        left: 50%;
        top: ${6 + ITEM_H * 2}px;
        transform: translateX(-50%);
        width: 200px;
        height: ${ITEM_H}px;
        border-radius: 10px;
        /* 반투명 - 불투명 배경은 선택 값을 덮어버린다 */
        background: color-mix(in srgb, var(--accent) 13%, transparent);
        pointer-events: none;
      }
      .colon {
        align-self: center;
        font-size: 1.3rem;
        font-weight: 700;
        padding-bottom: 8px;
      }
      /* 시(時) 단일 휠의 단위 라벨 - 휠 중앙(선택 줄)과 세로 정렬 */
      .unit {
        align-self: center;
        font-size: 1rem;
        font-weight: 600;
        color: var(--text-sub);
        line-height: 1;
      }
    `,
  ];

  protected get sheetTitle(): string {
    return this.options.title ?? '시각 선택';
  }

  private initial = '09:00';
  private options: TimeWheelOptions = {};
  private picked: string | null = null;
  private resolveFn?: (value: string | null) => void;
  // 리스트 모드: 시/분 대신 임의 항목 하나를 고르는 단일 휠 (반복 간격 등)
  private listItems: string[] | null = null;

  show(current: string, options: TimeWheelOptions = {}): Promise<string | null> {
    this.listItems = null;
    this.initial = current;
    this.options = options;
    this.picked = null;
    this.openSheet();
    return new Promise((resolve) => {
      this.resolveFn = resolve;
    });
  }

  /** 임의 항목 목록에서 하나 선택 (예: '5분'~'120분'). 취소 시 null */
  showList(current: string, items: string[], title: string): Promise<string | null> {
    this.listItems = items;
    this.initial = current;
    this.options = { title };
    this.picked = null;
    this.openSheet();
    return new Promise((resolve) => {
      this.resolveFn = resolve;
    });
  }

  protected onClosed(): void {
    this.resolveFn?.(this.picked);
  }

  // 열릴 때 현재 값 위치로 스크롤
  protected updated(): void {
    if (!this.open) return;
    const hourEl = this.renderRoot.querySelector<HTMLElement>('.wheel.hours');
    const minEl = this.renderRoot.querySelector<HTMLElement>('.wheel.minutes');
    if (hourEl && hourEl.dataset.scrolled !== '1') {
      hourEl.dataset.scrolled = '1';
      if (this.listItems) {
        hourEl.scrollTop = Math.max(0, this.listItems.indexOf(this.initial)) * ITEM_H;
        return;
      }
      const parts = this.initial.split(':');
      hourEl.scrollTop = Number(parts[0] ?? 9) * ITEM_H;
      if (minEl) minEl.scrollTop = Number(parts[1] ?? 0) * ITEM_H;
    }
  }

  // 데스크톱 마우스 드래그로 휠 돌리기 (터치는 브라우저 네이티브 스크롤)
  private dragWheel: HTMLElement | null = null;
  private dragFromY = 0;
  private dragFromScroll = 0;

  private onWheelDown = (e: PointerEvent): void => {
    if (e.pointerType !== 'mouse') return;
    const wheel = e.currentTarget as HTMLElement;
    this.dragWheel = wheel;
    this.dragFromY = e.clientY;
    this.dragFromScroll = wheel.scrollTop;
    wheel.classList.add('dragging');
    wheel.setPointerCapture(e.pointerId);
    e.preventDefault();
  };

  private onWheelMove = (e: PointerEvent): void => {
    if (!this.dragWheel) return;
    this.dragWheel.scrollTop = this.dragFromScroll - (e.clientY - this.dragFromY);
  };

  private onWheelUp = (): void => {
    const wheel = this.dragWheel;
    if (!wheel) return;
    this.dragWheel = null;
    wheel.classList.remove('dragging');
    wheel.scrollTo({ top: Math.round(wheel.scrollTop / ITEM_H) * ITEM_H, behavior: 'smooth' });
  };

  private confirm(): void {
    const hourEl = this.renderRoot.querySelector<HTMLElement>('.wheel.hours');
    const minEl = this.renderRoot.querySelector<HTMLElement>('.wheel.minutes');
    if (this.listItems) {
      const idx = Math.min(
        this.listItems.length - 1,
        Math.max(0, Math.round((hourEl?.scrollTop ?? 0) / ITEM_H)),
      );
      this.picked = this.listItems[idx] ?? null;
      this.requestClose();
      return;
    }
    const h = Math.min(23, Math.max(0, Math.round((hourEl?.scrollTop ?? 0) / ITEM_H)));
    const m = this.options.hourOnly
      ? 0
      : Math.min(59, Math.max(0, Math.round((minEl?.scrollTop ?? 0) / ITEM_H)));
    this.picked = `${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}`;
    this.requestClose();
  }

  protected renderBody(): TemplateResult {
    if (this.listItems) {
      return html`
        <div class="wheels">
          <div class="indicator"></div>
          <div class="wheel hours"
            @pointerdown=${this.onWheelDown} @pointermove=${this.onWheelMove}
            @pointerup=${this.onWheelUp} @pointercancel=${this.onWheelUp}>
            <div class="pad"></div>
            ${this.listItems.map((item) => html`<div class="item">${item}</div>`)}
            <div class="pad"></div>
          </div>
        </div>
        <button class="btn-primary" @click=${this.confirm}>완료</button>
      `;
    }
    return html`
      <div class="wheels">
        <div class="indicator"></div>
        <div class="wheel hours"
          @pointerdown=${this.onWheelDown} @pointermove=${this.onWheelMove}
          @pointerup=${this.onWheelUp} @pointercancel=${this.onWheelUp}>
          <div class="pad"></div>
          ${HOURS.map((h) => html`<div class="item">${String(h).padStart(2, '0')}</div>`)}
          <div class="pad"></div>
        </div>
        ${this.options.hourOnly
          ? html`<div class="unit">시</div>`
          : html`
              <div class="colon">:</div>
              <div class="wheel minutes"
                @pointerdown=${this.onWheelDown} @pointermove=${this.onWheelMove}
                @pointerup=${this.onWheelUp} @pointercancel=${this.onWheelUp}>
                <div class="pad"></div>
                ${MINUTES.map((m) => html`<div class="item">${String(m).padStart(2, '0')}</div>`)}
                <div class="pad"></div>
              </div>
            `}
        ${nothing}
      </div>
      <button class="btn-primary" @click=${this.confirm}>완료</button>
    `;
  }
}
