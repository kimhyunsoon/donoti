import { LitElement, html, css, type TemplateResult } from 'lit';
import { customElement, state } from 'lit/decorators.js';
import { tokens } from '../style.js';
import { icon } from '../icons.js';

// 놓으면 새로고침되는 당김 거리
const THRESHOLD = 56;

// 당겨서 새로고침 래퍼 - 페이지 최상단에서 아래로 당기면 'refresh' 이벤트 발행
@customElement('pull-refresh')
export class PullRefresh extends LitElement {
  static styles = [
    tokens,
    css`
      :host { display: block; }
      .indicator {
        display: flex;
        align-items: flex-end;
        justify-content: center;
        overflow: hidden;
        height: 0;
        color: var(--text-sub);
        transition: height 0.15s ease-out;
      }
      .indicator.dragging { transition: none; }
      .indicator .spin { padding-bottom: 10px; }
      .indicator.refreshing .spin { animation: spin 0.8s linear infinite; }
      @keyframes spin {
        to { transform: rotate(360deg); }
      }
    `,
  ];

  @state() private pull = 0;
  @state() private refreshing = false;
  @state() private dragging = false;

  private startY = -1;

  connectedCallback(): void {
    super.connectedCallback();
    // preventDefault가 필요해 passive:false로 직접 등록
    this.addEventListener('touchstart', this.onTouchStart, { passive: true });
    this.addEventListener('touchmove', this.onTouchMove, { passive: false });
    this.addEventListener('touchend', this.onTouchEnd);
    this.addEventListener('touchcancel', this.onTouchEnd);
  }

  private onTouchStart = (e: TouchEvent): void => {
    if (this.refreshing || window.scrollY > 0) return;
    this.startY = e.touches[0]?.clientY ?? -1;
  };

  private onTouchMove = (e: TouchEvent): void => {
    if (this.startY < 0 || this.refreshing) return;
    const dy = (e.touches[0]?.clientY ?? 0) - this.startY;
    if (dy <= 0 || window.scrollY > 0) {
      this.pull = 0;
      this.dragging = false;
      return;
    }
    e.preventDefault();
    this.dragging = true;
    // 손가락 이동의 절반만 따라와 저항감 부여
    this.pull = Math.min(80, dy * 0.5);
  };

  private onTouchEnd = (): void => {
    if (this.startY < 0) return;
    this.startY = -1;
    this.dragging = false;
    if (this.pull >= THRESHOLD) {
      this.refreshing = true;
      this.pull = THRESHOLD;
      this.dispatchEvent(new CustomEvent('refresh'));
      // 로드는 짧으므로 잠시 스피너를 보여주고 접는다
      setTimeout(() => {
        this.refreshing = false;
        this.pull = 0;
      }, 900);
    } else {
      this.pull = 0;
    }
  };

  render(): TemplateResult {
    return html`
      <div
        class="indicator ${this.dragging ? 'dragging' : ''} ${this.refreshing ? 'refreshing' : ''}"
        style="height:${this.pull}px"
      >
        <span class="spin" style="opacity:${Math.min(1, this.pull / THRESHOLD)}">
          ${icon('refresh', 20)}
        </span>
      </div>
      <slot></slot>
    `;
  }
}
