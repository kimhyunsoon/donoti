import { LitElement, html, css, nothing, type TemplateResult } from 'lit';
import { customElement, state } from 'lit/decorators.js';
import { tokens, ui } from '../style.js';
import { api } from '../api.js';
import { icon } from '../icons.js';
import { setupPushOnce } from '../push.js';

interface Me {
  id: number;
  username: string;
}

// 로그인 후 홈 - 감시 설정 등 실제 기능은 다음 단계에서 이 화면 위에 얹는다
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
      .card h2 { font-size: 1.02rem; font-weight: 700; margin: 0 0 4px; }
      .card p { margin: 0; }
    `,
  ];

  @state() private me: Me | null = null;
  @state() private unread = 0;

  connectedCallback(): void {
    super.connectedCallback();
    void api<Me>('/api/auth/me').then((me) => {
      this.me = me;
      // 첫 진입 1회: 알림 권한 확인 후 이 기기 구독 (거부해도 설정에서 다시 켤 수 있음)
      void setupPushOnce();
    });
    void api<{ count: number }>('/api/notifications/unread-count').then(({ count }) => {
      this.unread = count;
    });
  }

  render(): TemplateResult {
    return html`
      <header>
        <img src="/logo-text.png" alt="두노티">
        <button class="btn-icon" aria-label="알림센터" @click=${(): void => { location.hash = '#/notifications'; }}>
          ${icon('bell', 21)}
          ${this.unread > 0 ? html`<span class="dot"></span>` : nothing}
        </button>
        <button class="btn-icon" aria-label="설정" @click=${(): void => { location.hash = '#/settings'; }}>
          ${icon('settings', 21)}
        </button>
      </header>

      <div class="card">
        <h2>${this.me ? `${this.me.username}님, 반갑습니다` : html`&nbsp;`}</h2>
        <p class="sub">감시 대상·주기 설정은 준비 중입니다.</p>
      </div>
    `;
  }
}
