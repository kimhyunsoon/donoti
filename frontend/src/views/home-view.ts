import { LitElement, html, css, nothing, type TemplateResult } from 'lit';
import { customElement, state } from 'lit/decorators.js';
import { tokens, ui } from '../style.js';
import { api } from '../api.js';
import { ensureSubscribed } from '../push.js';

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
        padding: calc(24px + env(safe-area-inset-top)) 16px calc(24px + env(safe-area-inset-bottom));
      }
      header { display: flex; align-items: center; gap: 10px; margin-bottom: 20px; }
      header img { width: 34px; border-radius: 9px; }
      header h1 { font-size: 1.15rem; margin: 0; flex: 1; }
      .card { display: flex; flex-direction: column; gap: 10px; margin-bottom: 12px; }
      .card h2 { font-size: 0.95rem; margin: 0; }
      .toast { color: var(--ok); font-size: 0.85rem; margin: 0; }
      .toast.fail { color: var(--danger); }
    `,
  ];

  @state() private me: Me | null = null;
  @state() private pushMessage = '';
  @state() private pushFailed = false;

  connectedCallback(): void {
    super.connectedCallback();
    void api<Me>('/api/auth/me').then((me) => {
      this.me = me;
    });
  }

  private async onEnablePush(): Promise<void> {
    const ok = await ensureSubscribed().catch(() => false);
    this.pushFailed = !ok;
    this.pushMessage = ok
      ? '이 기기가 알림을 받습니다'
      : '알림 권한이 필요합니다 (브라우저 설정 확인)';
  }

  // 발송 파이프라인 점검: 큐에 넣으면 워커가 몇 초 내 이 기기로 푸시를 보낸다
  private async onTestNotification(): Promise<void> {
    await api('/api/notifications', {
      method: 'POST',
      body: JSON.stringify({ title: '🔔 donoti 테스트', body: '알림 파이프라인이 동작합니다' }),
    });
    this.pushFailed = false;
    this.pushMessage = '테스트 알림을 큐에 넣었습니다 - 곧 도착합니다';
  }

  private async onLogout(): Promise<void> {
    await api('/api/auth/logout', { method: 'POST' });
    location.hash = '#/login';
  }

  render(): TemplateResult {
    return html`
      <header>
        <img src="/icons/icon-192.png" alt="">
        <h1>donoti</h1>
        <button class="btn-ghost" @click=${this.onLogout}>로그아웃</button>
      </header>

      <div class="card">
        <h2>${this.me ? `${this.me.username}님, 반갑습니다` : ''}</h2>
        <p class="sub">감시 대상·주기 설정은 준비 중입니다.</p>
      </div>

      <div class="card">
        <h2>알림</h2>
        <button class="btn-primary" @click=${this.onEnablePush}>이 기기에서 알림 받기</button>
        <button class="btn-soft" @click=${this.onTestNotification}>테스트 알림 보내기</button>
        ${this.pushMessage
          ? html`<p class="toast ${this.pushFailed ? 'fail' : ''}">${this.pushMessage}</p>`
          : nothing}
      </div>
    `;
  }
}
