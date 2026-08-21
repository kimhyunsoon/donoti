import { LitElement, html, css, nothing, type TemplateResult } from 'lit';
import { customElement, state } from 'lit/decorators.js';
import { tokens, ui } from '../style.js';
import { api } from '../api.js';
import { icon } from '../icons.js';
import { toast } from '../ui.js';

interface NotificationRow {
  id: number;
  source: string;
  title: string;
  body: string;
  url: string | null;
  status: 'pending' | 'sending' | 'sent' | 'failed';
  last_error: string | null;
  read_at: string | null;
  created_at: string; // UTC 'YYYY-MM-DD HH:MM:SS'
}

// SQLite UTC datetime → 상대 시간 문자열
function relativeTime(utc: string): string {
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

@customElement('notifications-view')
export class NotificationsView extends LitElement {
  static styles = [
    tokens,
    ui,
    css`
      :host { display: block; max-width: 480px; margin: 0 auto; min-height: 100dvh; padding-bottom: calc(24px + env(safe-area-inset-bottom)); }
      .list { margin: 4px 16px; display: flex; flex-direction: column; gap: 8px; }
      .item {
        background: var(--surface);
        border-radius: 16px;
        padding: 14px 16px;
        display: flex;
        flex-direction: column;
        gap: 3px;
        position: relative;
      }
      .item.unread::before {
        content: '';
        position: absolute;
        top: 18px;
        right: 16px;
        width: 7px;
        height: 7px;
        border-radius: 50%;
        background: var(--accent);
      }
      .item .title { font-weight: 700; font-size: 0.96rem; padding-right: 16px; }
      .item .body { font-size: 0.88rem; color: var(--text-sub); white-space: pre-wrap; word-break: break-word; }
      .item .meta { display: flex; align-items: center; gap: 8px; font-size: 0.78rem; color: var(--text-sub); margin-top: 3px; }
      .item .meta .fail { color: var(--danger); font-weight: 600; }
      .item .meta button { margin-left: auto; display: flex; align-items: center; gap: 4px; padding: 5px 10px; font-size: 0.8rem; }
      .empty {
        text-align: center;
        color: var(--text-sub);
        padding: 80px 20px;
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 12px;
      }
      .skeleton-item { height: 74px; margin: 0 16px 8px; border-radius: 16px; }
    `,
  ];

  @state() private items: NotificationRow[] | null = null;
  // 진입 시점의 안읽음 id - read-all 이후에도 이번 방문 동안은 표시 유지
  @state() private unreadIds = new Set<number>();

  connectedCallback(): void {
    super.connectedCallback();
    void this.load();
  }

  private async load(): Promise<void> {
    const items = await api<NotificationRow[]>('/api/notifications?limit=50');
    this.unreadIds = new Set(
      items.filter((n) => n.status === 'sent' && n.read_at === null).map((n) => n.id),
    );
    this.items = items;
    // 진입 = 전체 읽음 처리 + 앱 아이콘 배지 제거
    await api('/api/notifications/read-all', { method: 'POST' });
    const nav = navigator as Navigator & { clearAppBadge?: () => Promise<void> };
    if (nav.clearAppBadge) void nav.clearAppBadge().catch(() => {});
  }

  private async retry(id: number): Promise<void> {
    await api(`/api/notifications/${id}/retry`, { method: 'POST' });
    toast('재발송 큐에 넣었어요');
    this.items = await api<NotificationRow[]>('/api/notifications?limit=50');
  }

  private renderItem(n: NotificationRow): TemplateResult {
    const unread = this.unreadIds.has(n.id);
    return html`
      <div class="item ${unread ? 'unread' : ''}">
        <span class="title">${n.title}</span>
        ${n.body ? html`<span class="body">${n.body}</span>` : nothing}
        <span class="meta">
          <span>${relativeTime(n.created_at)}</span>
          <span>·</span>
          <span>${n.source}</span>
          ${n.status === 'failed'
            ? html`
                <span class="fail">발송 실패</span>
                <button class="btn-soft" @click=${(): void => void this.retry(n.id)}>
                  ${icon('refresh', 13)} 재시도
                </button>
              `
            : nothing}
          ${n.status === 'pending' || n.status === 'sending'
            ? html`<span>· 발송 중</span>`
            : nothing}
        </span>
      </div>
    `;
  }

  render(): TemplateResult {
    return html`
      <div class="top">
        <button class="btn-icon" aria-label="뒤로" @click=${(): void => { location.hash = '#/home'; }}>
          ${icon('chevron-left', 22)}
        </button>
        <h1>알림</h1>
      </div>

      ${this.items === null
        ? html`${[1, 2, 3].map(() => html`<div class="skeleton skeleton-item"></div>`)}`
        : this.items.length === 0
          ? html`
              <div class="empty">
                ${icon('bell', 36)}
                <span>아직 받은 알림이 없어요</span>
              </div>
            `
          : html`<div class="list">${this.items.map((n) => this.renderItem(n))}</div>`}
    `;
  }
}
