import { LitElement, html, css, nothing, type TemplateResult } from 'lit';
import { customElement, state } from 'lit/decorators.js';
import { tokens, ui } from '../style.js';
import { api } from '../api.js';
import { icon } from '../icons.js';
import { toast } from '../ui.js';
import { relativeTime } from '../format.js';
import { findProviderById, providerAvatar } from '../catalog.js';
import '../sheets/confirm-sheet.js';
import type { ConfirmSheet } from '../sheets/confirm-sheet.js';

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

// 알림센터 - 방문만으로는 읽음 처리하지 않는다.
// 읽음 처리 경로: 우측 스와이프 / 링크 이동 확인 / OS 알림 클릭(서비스워커)
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
        gap: 12px;
        align-items: flex-start;
        position: relative;
        /* 수평 스와이프는 읽음 제스처로 처리 (수직 스크롤은 브라우저에 맡김) */
        touch-action: pan-y;
      }
      .item .content {
        display: flex;
        flex-direction: column;
        gap: 3px;
        min-width: 0;
        flex: 1;
      }
      .item.read { opacity: 0.55; }
      .item.linked { cursor: pointer; }
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

  // 우측 스와이프(읽음 처리) 추적
  private swipeStartX = -1;
  private swipeStartY = -1;
  private swipeEl: HTMLElement | null = null;
  private swipeMoved = false;

  // 새 푸시 도착 시(서비스워커 postMessage) 목록 즉시 갱신
  private onSwMessage = (e: MessageEvent): void => {
    if ((e.data as { type?: string })?.type === 'push') void this.load();
  };

  connectedCallback(): void {
    super.connectedCallback();
    void this.load();
    navigator.serviceWorker?.addEventListener('message', this.onSwMessage);
  }

  disconnectedCallback(): void {
    navigator.serviceWorker?.removeEventListener('message', this.onSwMessage);
    super.disconnectedCallback();
  }

  private async load(): Promise<void> {
    // 알림센터는 항상 최근 100개까지만. 안읽은 알림을 위로 (그 안에서는 최신순 유지)
    const items = await api<NotificationRow[]>('/api/notifications?limit=100');
    const isUnread = (n: NotificationRow): number => (n.status === 'sent' && n.read_at === null ? 0 : 1);
    this.items = items.sort((a, b) => isUnread(a) - isUnread(b) || b.id - a.id);
  }

  // 개별 읽음 처리 + 앱 아이콘 배지 갱신
  private async markRead(n: NotificationRow): Promise<void> {
    if (n.read_at) return;
    const { unread } = await api<{ unread: number }>(`/api/notifications/${n.id}/read`, {
      method: 'POST',
    });
    n.read_at = new Date().toISOString();
    this.requestUpdate();
    const nav = navigator as Navigator & {
      setAppBadge?: (count: number) => Promise<void>;
      clearAppBadge?: () => Promise<void>;
    };
    if (unread > 0) void nav.setAppBadge?.(unread).catch(() => {});
    else void nav.clearAppBadge?.().catch(() => {});
  }

  // 링크 있는 알림 클릭 → 이동 확인 모달 → 읽음 처리 후 링크로
  private async openLink(n: NotificationRow): Promise<void> {
    if (this.swipeMoved) {
      this.swipeMoved = false;
      return;
    }
    if (!n.url) return;
    const sheet = this.renderRoot.querySelector('confirm-sheet') as ConfirmSheet;
    const ok = await sheet.show({ title: '링크로 이동할까요?', message: n.url, confirmLabel: '이동' });
    if (!ok) return;
    void this.markRead(n);
    window.open(n.url, '_blank', 'noopener');
  }

  private onSwipeStart(e: PointerEvent, n: NotificationRow): void {
    this.swipeMoved = false;
    // 이미 읽은 알림은 스와이프 제스처 없음
    if (n.read_at) return;
    this.swipeStartX = e.clientX;
    this.swipeStartY = e.clientY;
    this.swipeEl = e.currentTarget as HTMLElement;
  }

  private onSwipeMove(e: PointerEvent): void {
    if (!this.swipeEl) return;
    const dx = e.clientX - this.swipeStartX;
    const dy = e.clientY - this.swipeStartY;
    if (!this.swipeMoved) {
      // 수평 의도가 분명할 때만 제스처 시작
      if (Math.abs(dx) < 8 || Math.abs(dx) < Math.abs(dy)) return;
      this.swipeMoved = true;
      this.swipeEl.setPointerCapture(e.pointerId);
    }
    this.swipeEl.style.transition = 'none';
    this.swipeEl.style.transform = `translateX(${Math.max(0, dx)}px)`;
  }

  private onSwipeEnd(e: PointerEvent, n: NotificationRow): void {
    const el = this.swipeEl;
    this.swipeEl = null;
    if (!el || !this.swipeMoved) return;
    el.style.transition = 'transform 0.18s ease-out';
    el.style.transform = 'translateX(0)';
    if (e.clientX - this.swipeStartX > 80) void this.markRead(n);
  }

  private async retry(n: NotificationRow, e: Event): Promise<void> {
    e.stopPropagation();
    await api(`/api/notifications/${n.id}/retry`, { method: 'POST' });
    toast('재발송 큐에 넣었어요');
    await this.load();
  }

  private renderItem(n: NotificationRow): TemplateResult {
    const unread = n.status === 'sent' && n.read_at === null;
    const info = findProviderById(n.source);
    return html`
      <div
        class="item ${unread ? 'unread' : ''} ${n.read_at ? 'read' : ''} ${n.url ? 'linked' : ''}"
        @click=${(): void => void this.openLink(n)}
        @pointerdown=${(e: PointerEvent): void => this.onSwipeStart(e, n)}
        @pointermove=${(e: PointerEvent): void => this.onSwipeMove(e)}
        @pointerup=${(e: PointerEvent): void => this.onSwipeEnd(e, n)}
        @pointercancel=${(e: PointerEvent): void => this.onSwipeEnd(e, n)}
      >
        ${info ? providerAvatar(info.provider, 36) : nothing}
        <div class="content">
        <span class="title">${n.title}</span>
        ${n.body ? html`<span class="body">${n.body}</span>` : nothing}
        <span class="meta">
          <span>${relativeTime(n.created_at)}</span>
          <span>·</span>
          <span>${n.source}</span>
          ${n.status === 'failed'
            ? html`
                <span class="fail">발송 실패</span>
                <button class="btn-soft" @click=${(e: Event): void => void this.retry(n, e)}>
                  ${icon('refresh', 13)} 재시도
                </button>
              `
            : nothing}
          ${n.status === 'pending' || n.status === 'sending'
            ? html`<span>· 발송 중</span>`
            : nothing}
        </span>
        </div>
      </div>
    `;
  }

  // 전부 읽음 처리 + 앱 배지 초기화
  private async readAll(): Promise<void> {
    await api('/api/notifications/read-all', { method: 'POST' });
    const now = new Date().toISOString();
    for (const n of this.items ?? []) {
      if (n.status === 'sent' && n.read_at === null) n.read_at = now;
    }
    this.requestUpdate();
    const nav = navigator as Navigator & { clearAppBadge?: () => Promise<void> };
    void nav.clearAppBadge?.().catch(() => {});
    toast('모두 읽음 처리했어요');
  }

  render(): TemplateResult {
    return html`
      <div class="top">
        <button class="btn-icon" aria-label="뒤로" @click=${(): void => { location.hash = '#/home'; }}>
          ${icon('chevron-left', 22)}
        </button>
        <h1>알림</h1>
        ${this.items?.some((n) => n.status === 'sent' && n.read_at === null)
          ? html`
              <button class="btn-ghost" style="margin-left:auto"
                @click=${(): void => void this.readAll()}>모두 읽기</button>
            `
          : nothing}
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

      <confirm-sheet></confirm-sheet>
    `;
  }
}
