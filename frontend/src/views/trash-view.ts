import { LitElement, html, css, type TemplateResult } from 'lit';
import { customElement, state } from 'lit/decorators.js';
import { tokens, ui } from '../style.js';
import { api } from '../api.js';
import { icon } from '../icons.js';
import { toast } from '../ui.js';
import { findProvider, providerAvatar } from '../catalog.js';
import { relativeTime, formatEndsAt } from '../format.js';

interface WatchRow {
  id: number;
  category: string;
  provider: string;
  name: string;
  ends_at: string | null;
  deleted_at: string;
  created_at: string;
}

// 종료·삭제된 알림 - 등록 기준 최근 100개, 복구하면 상시 알림으로 되살아난다
@customElement('trash-view')
export class TrashView extends LitElement {
  static styles = [
    tokens,
    ui,
    css`
      :host {
        display: block;
        max-width: 480px;
        margin: 0 auto;
        min-height: 100dvh;
        padding-bottom: calc(24px + env(safe-area-inset-bottom));
      }
      .list { margin: 4px 16px; display: flex; flex-direction: column; gap: 8px; }
      .item {
        background: var(--surface);
        border-radius: 16px;
        padding: 14px 16px;
        display: flex;
        align-items: center;
        gap: 12px;
      }
      .item .names { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
      .item .names b { font-size: 0.94rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
      .item button { margin-left: auto; display: flex; align-items: center; gap: 4px; flex-shrink: 0; }
      .empty {
        text-align: center;
        color: var(--text-sub);
        padding: 80px 20px;
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 12px;
      }
      .skeleton-item { height: 66px; margin: 0 16px 8px; border-radius: 16px; }
    `,
  ];

  @state() private items: WatchRow[] | null = null;

  connectedCallback(): void {
    super.connectedCallback();
    void this.load();
  }

  private async load(): Promise<void> {
    this.items = await api<WatchRow[]>('/api/watches/deleted');
  }

  private async restore(id: number): Promise<void> {
    await api(`/api/watches/${id}/restore`, { method: 'POST' });
    toast('상시 알림으로 복구했어요');
    await this.load();
  }

  private renderItem(w: WatchRow): TemplateResult {
    const info = findProvider(w.category, w.provider);
    const detail = w.ends_at
      ? `${formatEndsAt(w.ends_at)} 종료됨`
      : `${relativeTime(w.deleted_at)} 삭제됨`;
    return html`
      <div class="item">
        ${info ? providerAvatar(info.provider, 40) : html``}
        <span class="names">
          <b>${w.name}</b>
          <span class="sub">${info?.provider.label ?? w.provider} · ${detail}</span>
        </span>
        <button class="btn-soft" @click=${(): void => void this.restore(w.id)}>
          ${icon('restore', 14)} 복구
        </button>
      </div>
    `;
  }

  render(): TemplateResult {
    return html`
      <div class="top">
        <button class="btn-icon" aria-label="뒤로" @click=${(): void => { location.hash = '#/settings'; }}>
          ${icon('chevron-left', 22)}
        </button>
        <h1>종료된 알림</h1>
      </div>

      ${this.items === null
        ? html`${[1, 2, 3].map(() => html`<div class="skeleton skeleton-item"></div>`)}`
        : this.items.length === 0
          ? html`
              <div class="empty">
                ${icon('trash', 36)}
                <span>종료되거나 삭제된 알림이 없어요</span>
              </div>
            `
          : html`<div class="list">${this.items.map((w) => this.renderItem(w))}</div>`}
    `;
  }
}
