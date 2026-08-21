import { html, css, type TemplateResult } from 'lit';
import { customElement } from 'lit/decorators.js';
import { tokens, ui, sheet } from '../style.js';
import { SheetBase } from './sheet-base.js';

interface ConfirmOptions {
  title: string;
  message: string;
  confirmLabel: string;
}

// 범용 확인 바텀시트. show(...) → true(확인) | false(닫기·취소)
@customElement('confirm-sheet')
export class ConfirmSheet extends SheetBase {
  static styles = [
    tokens,
    ui,
    sheet,
    css`
      .message {
        margin: 2px 4px 18px;
        font-size: 0.95rem;
        color: var(--text-sub);
        word-break: break-all;
      }
    `,
  ];

  protected get sheetTitle(): string {
    return this.options.title;
  }

  private options: ConfirmOptions = { title: '', message: '', confirmLabel: '확인' };
  private ok = false;
  private resolveFn?: (value: boolean) => void;

  show(options: ConfirmOptions): Promise<boolean> {
    this.options = options;
    this.ok = false;
    this.openSheet();
    return new Promise((resolve) => {
      this.resolveFn = resolve;
    });
  }

  protected onClosed(): void {
    this.resolveFn?.(this.ok);
  }

  private confirm(): void {
    this.ok = true;
    this.requestClose();
  }

  protected renderBody(): TemplateResult {
    return html`
      <p class="message">${this.options.message}</p>
      <button class="btn-primary" @click=${(): void => this.confirm()}>
        ${this.options.confirmLabel}
      </button>
    `;
  }
}
