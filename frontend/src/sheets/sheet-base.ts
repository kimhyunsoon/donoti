import { LitElement, html, nothing, type TemplateResult } from 'lit';
import { state } from 'lit/decorators.js';
import { pushModal } from '../ui.js';

// 바텀시트 공통 베이스 (dogroo 패턴)
// - 열기: openSheet() → 히스토리 등록(pushModal) + 배경 스크롤 잠금
// - 닫기: 닫기 버튼·배경 탭·핸들 아래로 드래그·뒤로가기(스와이프) 전부 같은 경로
// - 닫힐 때 슬라이드다운 애니메이션 후 onClosed() 호출
export abstract class SheetBase extends LitElement {
  @state() protected open = false;
  @state() protected closing = false;
  @state() private dragY = 0;

  private closeFn?: () => void;
  private dragStartY = -1;

  protected abstract get sheetTitle(): string;
  protected abstract renderBody(): TemplateResult;
  // 닫힘 애니메이션까지 끝난 뒤 호출 (Promise resolve 지점)
  protected onClosed(): void {}

  protected openSheet(): void {
    this.open = true;
    this.closing = false;
    this.dragY = 0;
    this.closeFn = pushModal(() => void this.animateOut());
  }

  protected requestClose(): void {
    this.closeFn?.();
  }

  private async animateOut(): Promise<void> {
    this.closing = true;
    await new Promise((resolve) => setTimeout(resolve, 190));
    this.open = false;
    this.closing = false;
    this.dragY = 0;
    this.onClosed();
  }

  // 핸들(그래버·헤더) 아래로 드래그하면 닫힘 - 버튼 위에서는 드래그 시작하지 않음(클릭 보장)
  private onDragStart = (e: PointerEvent): void => {
    if ((e.target as HTMLElement).closest('button')) return;
    this.dragStartY = e.clientY;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  };

  private onDragMove = (e: PointerEvent): void => {
    if (this.dragStartY < 0) return;
    this.dragY = Math.max(0, e.clientY - this.dragStartY);
  };

  private onDragEnd = (): void => {
    if (this.dragStartY < 0) return;
    this.dragStartY = -1;
    if (this.dragY > 90) this.requestClose();
    else this.dragY = 0;
  };

  private onOverlayClick = (e: Event): void => {
    if ((e.target as HTMLElement).classList.contains('overlay')) this.requestClose();
  };

  render(): TemplateResult | typeof nothing {
    if (!this.open) return nothing;
    const style = this.dragY > 0 && !this.closing ? `transform:translateY(${this.dragY}px);transition:none;` : '';
    return html`
      <div class="overlay ${this.closing ? 'closing' : ''}" @click=${this.onOverlayClick}>
        <div class="panel" style=${style}>
          <div
            class="drag-zone"
            @pointerdown=${this.onDragStart}
            @pointermove=${this.onDragMove}
            @pointerup=${this.onDragEnd}
            @pointercancel=${this.onDragEnd}
          >
            <div class="grabber"></div>
            <div class="panel-head">
              <h2>${this.sheetTitle}</h2>
              <button class="btn-ghost" @click=${(): void => this.requestClose()}>닫기</button>
            </div>
          </div>
          <div class="panel-body">${this.renderBody()}</div>
        </div>
      </div>
    `;
  }
}
