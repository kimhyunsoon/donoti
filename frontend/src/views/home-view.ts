import { LitElement, html, css, nothing, type TemplateResult } from 'lit';
import { customElement, state } from 'lit/decorators.js';
import { tokens, ui } from '../style.js';
import { api } from '../api.js';
import { icon } from '../icons.js';
import { setupPushOnce } from '../push.js';
import { CATALOG, findProvider, providerAvatar } from '../catalog.js';
import '../components/pull-refresh.js';
import { summarizeSchedule, nextRunAt, formatUntil, type ScheduleRule } from '../schedule.js';
import { formatEndsAt, relativeTime } from '../format.js';
import { toast } from '../ui.js';
import '../sheets/confirm-sheet.js';
import type { ConfirmSheet } from '../sheets/confirm-sheet.js';

interface WatchRow {
  id: number;
  category: string;
  provider: string;
  name: string;
  schedule: string;
  config: string;
  enabled: number;
  ends_at: string | null;
  // 가장 최근 알림 내용·시각 (없으면 null)
  last_body: string | null;
  last_at: string | null;
}

// 이마트 감시 지점 요약: '구로점·성남점 외 2곳'
function emartStoreLabel(config: string): string {
  try {
    const stores = (JSON.parse(config) as { stores?: { name: string }[] }).stores ?? [];
    const names = stores.map((s) => s.name.replace(/^이마트 /, ''));
    const head = names.slice(0, 2).join('·');
    return names.length > 2 ? `${head} 외 ${names.length - 2}곳` : head;
  } catch {
    return '';
  }
}

// 메인 - 설정한 감시 알림 목록
@customElement('home-view')
export class HomeView extends LitElement {
  static styles = [
    tokens,
    ui,
    css`
      :host {
        display: block;
        max-width: 480px;
        margin: 0 auto;
        padding: calc(14px + env(safe-area-inset-top)) 16px calc(24px + env(safe-area-inset-bottom));
      }
      header { display: flex; align-items: center; margin-bottom: 16px; }
      header img { height: 22px; margin-right: auto; }
      /* 안읽은 알림 표시 점 */
      .dot {
        position: absolute;
        top: 7px;
        right: 8px;
        width: 8px;
        height: 8px;
        border-radius: 50%;
        background: var(--accent);
      }
      .list { display: flex; flex-direction: column; gap: 8px; }
      /* 대메뉴(카테고리) 섹션 라벨 */
      .section-label {
        margin: 16px 4px 8px;
        font-size: 0.8rem;
        font-weight: 700;
        color: var(--text-sub);
      }
      .section-label:first-of-type { margin-top: 0; }
      .item {
        background: var(--surface);
        border-radius: 16px;
        padding: 14px 16px;
        display: flex;
        align-items: center;
        gap: 12px;
        cursor: pointer;
        /* 수평 스와이프는 삭제 제스처로 처리 (수직 스크롤은 브라우저에 맡김) */
        touch-action: pan-y;
      }
      /* 중지 중에는 스위치만 남기고 흐리게 */
      .item.off .body-wrap { opacity: 0.45; }
      .body-wrap { display: flex; align-items: center; gap: 12px; min-width: 0; flex: 1; }
      .names { display: flex; flex-direction: column; gap: 1px; min-width: 0; }
      /* 직계 자식만 - .last 안의 b(알림 내용)는 줄바꿈되어야 한다 */
      .names > b { font-size: 0.95rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
      .names > .sub { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 0.8rem; }
      .next { color: var(--accent); font-weight: 700; }
      /* 최근 알림 (있을 때만) - 잘리지 않게 전부 표시, 설정 정보와 구분선 */
      .last {
        font-size: 0.8rem;
        color: var(--text-sub);
        /* 알림 본문의 줄바꿈(회차 나열 등)을 그대로 보여준다 */
        white-space: pre-line;
        word-break: break-word;
        line-height: 1.45;
        margin-top: 6px;
        padding-top: 7px;
        border-top: 1px solid var(--border);
      }
      .last b { font-weight: 600; color: var(--text); font-size: 0.8rem; }
      .last .when { white-space: nowrap; }
      .ends {
        display: inline-flex;
        align-self: flex-start;
        margin-top: 3px;
        padding: 2px 8px;
        border-radius: 7px;
        background: var(--accent-soft);
        color: var(--accent);
        font-size: 0.74rem;
        font-weight: 700;
      }
      .switch { margin-left: auto; }
      .empty {
        background: var(--surface);
        border-radius: 20px;
        padding: 44px 20px;
        text-align: center;
        color: var(--text-sub);
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 14px;
      }
      .empty .btn-primary { width: auto; padding: 13px 22px; }
      .skeleton-item { height: 70px; border-radius: 16px; margin-bottom: 8px; }
    `,
  ];

  @state() private watches: WatchRow[] | null = null;
  @state() private unread = 0;

  // '다음 알림 n분 후' 갱신용
  private ticker: ReturnType<typeof setInterval> | null = null;

  // 새 푸시 도착 시(서비스워커 postMessage) 목록·안읽음 즉시 갱신
  private onSwMessage = (e: MessageEvent): void => {
    if ((e.data as { type?: string })?.type === 'push') this.refresh();
  };

  private refresh(): void {
    void api<WatchRow[]>('/api/watches').then((watches) => {
      this.watches = watches;
    });
    void api<{ count: number }>('/api/notifications/unread-count').then(({ count }) => {
      this.unread = count;
    });
  }

  connectedCallback(): void {
    super.connectedCallback();
    this.refresh();
    // 첫 진입 1회: 알림 권한 확인 후 이 기기 구독 (거부해도 설정에서 다시 켤 수 있음)
    void setupPushOnce();
    this.ticker = setInterval(() => this.requestUpdate(), 30_000);
    navigator.serviceWorker?.addEventListener('message', this.onSwMessage);
  }

  disconnectedCallback(): void {
    if (this.ticker) clearInterval(this.ticker);
    navigator.serviceWorker?.removeEventListener('message', this.onSwMessage);
    super.disconnectedCallback();
  }

  // 우측 스와이프(삭제) 추적
  private swipeStartX = -1;
  private swipeStartY = -1;
  private swipeEl: HTMLElement | null = null;
  private swipeMoved = false;

  private onSwipeStart(e: PointerEvent): void {
    this.swipeMoved = false;
    // 스위치 위에서 시작한 드래그는 무시
    if ((e.target as HTMLElement).closest('.switch')) return;
    this.swipeStartX = e.clientX;
    this.swipeStartY = e.clientY;
    this.swipeEl = e.currentTarget as HTMLElement;
  }

  private onSwipeMove(e: PointerEvent): void {
    if (!this.swipeEl) return;
    const dx = e.clientX - this.swipeStartX;
    const dy = e.clientY - this.swipeStartY;
    if (!this.swipeMoved) {
      if (Math.abs(dx) < 8 || Math.abs(dx) < Math.abs(dy)) return;
      this.swipeMoved = true;
      this.swipeEl.setPointerCapture(e.pointerId);
    }
    this.swipeEl.style.transition = 'none';
    this.swipeEl.style.transform = `translateX(${Math.max(0, dx)}px)`;
  }

  private onSwipeEnd(e: PointerEvent, w: WatchRow): void {
    const el = this.swipeEl;
    this.swipeEl = null;
    if (!el || !this.swipeMoved) return;
    el.style.transition = 'transform 0.18s ease-out';
    el.style.transform = 'translateX(0)';
    if (e.clientX - this.swipeStartX > 80) void this.confirmRemove(w);
  }

  // 스와이프 삭제 - 확인 모달 후 soft delete (종료된 알림에서 복구 가능)
  private async confirmRemove(w: WatchRow): Promise<void> {
    const sheet = this.renderRoot.querySelector('confirm-sheet') as ConfirmSheet;
    const ok = await sheet.show({
      title: '알림을 삭제할까요?',
      message: `${w.name} · 종료된 알림에서 복구할 수 있어요`,
      confirmLabel: '삭제',
    });
    if (!ok) return;
    await api(`/api/watches/${w.id}`, { method: 'DELETE' });
    this.watches = (this.watches ?? []).filter((v) => v.id !== w.id);
    toast('삭제했어요');
  }

  // 임시 중지·재개 (행 클릭과 분리)
  private async toggle(w: WatchRow, e: Event): Promise<void> {
    e.stopPropagation();
    const enabled = (e.target as HTMLInputElement).checked;
    w.enabled = enabled ? 1 : 0;
    this.requestUpdate();
    await api(`/api/watches/${w.id}/toggle`, {
      method: 'POST',
      body: JSON.stringify({ enabled }),
    }).catch(() => {
      w.enabled = enabled ? 0 : 1;
      this.requestUpdate();
    });
  }

  private renderItem(w: WatchRow): TemplateResult {
    const info = findProvider(w.category, w.provider);
    let summary = '';
    let next: Date | null = null;
    try {
      const rules = JSON.parse(w.schedule) as ScheduleRule[];
      summary = summarizeSchedule(rules);
      next = nextRunAt(rules);
    } catch {
      summary = '';
    }
    const on = w.enabled === 1;
    const stores = w.provider === 'emart' ? emartStoreLabel(w.config) : '';
    // 이마트: '재고 있을 때만'(기본)일 때만 표기, 매번 알림은 표기 없음
    let stockNote = '';
    if (w.provider === 'emart') {
      try {
        const cfg = JSON.parse(w.config) as { onlyInStock?: boolean };
        if (cfg.onlyInStock !== false) stockNote = ' · 재고 있을 때만';
      } catch {
        stockNote = '';
      }
    }
    return html`
      <div class="item ${on ? '' : 'off'}"
        @click=${(): void => {
          if (this.swipeMoved) { this.swipeMoved = false; return; }
          location.hash = `#/watch/edit?id=${w.id}`;
        }}
        @pointerdown=${(e: PointerEvent): void => this.onSwipeStart(e)}
        @pointermove=${(e: PointerEvent): void => this.onSwipeMove(e)}
        @pointerup=${(e: PointerEvent): void => this.onSwipeEnd(e, w)}
        @pointercancel=${(e: PointerEvent): void => this.onSwipeEnd(e, w)}
      >
        <div class="body-wrap">
          ${info ? providerAvatar(info.provider, 42) : nothing}
          <span class="names">
            <b>${w.name}</b>
            <span class="sub">
              ${on && next ? html`<span class="next">${formatUntil(next)}</span> · ` : nothing}
              ${on ? '' : '중지됨 · '}${summary}${stockNote}
            </span>
            ${stores !== '' ? html`<span class="sub">${stores}</span>` : nothing}
            ${w.last_body && w.last_at
              ? html`<span class="last"><b>${w.last_body}</b> · <span class="when">${relativeTime(w.last_at)}</span></span>`
              : nothing}
            ${w.ends_at ? html`<span class="ends">${formatEndsAt(w.ends_at)}</span>` : nothing}
          </span>
        </div>
        <label class="switch" @click=${(e: Event): void => e.stopPropagation()}>
          <input type="checkbox" ?checked=${on} @change=${(e: Event): void => void this.toggle(w, e)}>
          <span class="knob"></span>
        </label>
      </div>
    `;
  }

  render(): TemplateResult {
    // 대메뉴(카테고리)별로 묶어 카탈로그 순서로 정렬
    const groups = CATALOG.map((c) => ({
      label: c.label,
      items: (this.watches ?? []).filter((w) => w.category === c.id),
    })).filter((g) => g.items.length > 0);
    return html`
      <pull-refresh @refresh=${(): void => this.refresh()}>
      <header>
        <img src="/logo-text.png" alt="두노티">
        <button class="btn-icon" aria-label="알림 만들기" @click=${(): void => { location.hash = '#/watch/new'; }}>
          ${icon('plus', 22)}
        </button>
        <button class="btn-icon" aria-label="알림센터" @click=${(): void => { location.hash = '#/notifications'; }}>
          ${icon('bell', 21)}
          ${this.unread > 0 ? html`<span class="dot"></span>` : nothing}
        </button>
        <button class="btn-icon" aria-label="설정" @click=${(): void => { location.hash = '#/settings'; }}>
          ${icon('settings', 21)}
        </button>
      </header>

      ${this.watches === null
        ? html`${[1, 2, 3].map(() => html`<div class="skeleton skeleton-item"></div>`)}`
        : this.watches.length === 0
          ? html`
              <div class="empty">
                ${icon('bell', 36)}
                <span>아직 등록한 알림이 없어요</span>
                <button class="btn-primary" @click=${(): void => { location.hash = '#/watch/new'; }}>
                  알림 만들기
                </button>
              </div>
            `
          : groups.map(
              (g) => html`
                <div class="section-label">${g.label}</div>
                <div class="list">${g.items.map((w) => this.renderItem(w))}</div>
              `,
            )}

      <confirm-sheet></confirm-sheet>
      </pull-refresh>
    `;
  }
}
