import { LitElement, html, css, type TemplateResult } from 'lit';
import { customElement, state } from 'lit/decorators.js';
import { tokens, ui } from '../style.js';
import { api } from '../api.js';
import { icon } from '../icons.js';
import { toast } from '../ui.js';
import { applyTheme, currentTheme, type ThemeMode } from '../theme.js';
import { ensureSubscribed, unsubscribe, isSubscribed } from '../push.js';

const THEME_LABEL: Record<ThemeMode, string> = { auto: '자동', light: '라이트', dark: '다크' };

@customElement('settings-view')
export class SettingsView extends LitElement {
  static styles = [
    tokens,
    ui,
    css`
      :host { display: block; max-width: 480px; margin: 0 auto; min-height: 100dvh; padding-bottom: calc(24px + env(safe-area-inset-bottom)); }
      .block { margin: 12px 16px; }
    `,
  ];

  @state() private username = '';
  @state() private subscribed = false;

  connectedCallback(): void {
    super.connectedCallback();
    void api<{ username: string }>('/api/auth/me').then((me) => {
      this.username = me.username;
    });
    void isSubscribed().then((on) => {
      this.subscribed = on;
    });
  }

  // 알림 켜기 = 이 기기 푸시 구독, 끄기 = 구독 해지
  private async toggleNotify(e: Event): Promise<void> {
    const on = (e.target as HTMLInputElement).checked;
    if (on) {
      const ok = await ensureSubscribed().catch(() => false);
      if (!ok) toast('브라우저 알림 권한이 필요해요');
      this.subscribed = ok;
      this.requestUpdate(); // 실패 시 스위치 원복
    } else {
      await unsubscribe().catch(() => {});
      this.subscribed = false;
    }
  }

  private setTheme(mode: ThemeMode): void {
    applyTheme(mode);
    this.requestUpdate();
  }

  private async logout(): Promise<void> {
    await api('/api/auth/logout', { method: 'POST' });
    location.hash = '#/login';
    toast('로그아웃했어요');
  }

  render(): TemplateResult {
    const theme = currentTheme();
    return html`
      <div class="top">
        <button class="btn-icon" aria-label="뒤로" @click=${(): void => { location.hash = '#/home'; }}>
          ${icon('chevron-left', 22)}
        </button>
        <h1>설정</h1>
      </div>

      <div class="block card">
        <div class="row">
          <span>테마</span>
          <div class="segmented" style="width: 210px">
            ${(['auto', 'light', 'dark'] as ThemeMode[]).map(
              (mode) => html`
                <button class=${theme === mode ? 'on' : ''} @click=${(): void => this.setTheme(mode)}>
                  ${THEME_LABEL[mode]}
                </button>
              `,
            )}
          </div>
        </div>
      </div>

      <div class="block card">
        <div class="row">
          <span>이 기기 알림</span>
          <label class="switch">
            <input
              type="checkbox"
              ?checked=${this.subscribed}
              @change=${(e: Event): void => void this.toggleNotify(e)}
            >
            <span class="knob"></span>
          </label>
        </div>
      </div>

      <div class="block card">
        <div class="row" style="cursor:pointer" @click=${(): void => { location.hash = '#/trash'; }}>
          <span>종료된 알림</span>
          <span style="color:var(--text-sub);display:flex">${icon('chevron-right', 18)}</span>
        </div>
      </div>

      <div class="block card">
        <div class="row">
          <span>${this.username}</span>
          <button class="btn-ghost" style="display:flex;align-items:center;gap:5px" @click=${(): void => void this.logout()}>
            ${icon('log-out', 15)} 로그아웃
          </button>
        </div>
      </div>
    `;
  }
}
