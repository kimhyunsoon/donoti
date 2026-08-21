import { LitElement, html, css, nothing, type TemplateResult } from 'lit';
import { customElement, state } from 'lit/decorators.js';
import { tokens, ui } from '../style.js';
import { api } from '../api.js';

@customElement('login-view')
export class LoginView extends LitElement {
  static styles = [
    tokens,
    ui,
    css`
      :host { display: grid; place-items: center; min-height: 100dvh; background: var(--bg); }
      form { display: flex; flex-direction: column; gap: 12px; width: min(320px, 82vw); }
      .logo { text-align: center; margin-bottom: 14px; }
      .logo img { width: 170px; }
      .error { color: var(--danger); font-size: 0.88rem; text-align: center; margin: 0; }
    `,
  ];

  @state() private error = '';
  @state() private busy = false;

  // 이미 로그인된 상태면 홈으로 (라우팅 가드)
  connectedCallback(): void {
    super.connectedCallback();
    void fetch('/api/auth/me', { credentials: 'same-origin' }).then((res) => {
      if (res.ok) location.hash = '#/home';
    });
  }

  private async onSubmit(e: SubmitEvent): Promise<void> {
    e.preventDefault();
    if (this.busy) return;
    this.busy = true;
    this.error = '';
    const form = new FormData(e.target as HTMLFormElement);
    try {
      await api('/api/auth/login', {
        method: 'POST',
        body: JSON.stringify({ username: form.get('username'), password: form.get('password') }),
      });
      location.hash = '#/home';
    } catch {
      this.error = '아이디 또는 비밀번호가 올바르지 않습니다';
    } finally {
      this.busy = false;
    }
  }

  render(): TemplateResult {
    return html`
      <form @submit=${this.onSubmit}>
        <div class="logo"><img src="/logo-text.png" alt="두노티"></div>
        <input name="username" placeholder="아이디" autocomplete="username" required>
        <input name="password" type="password" placeholder="비밀번호" autocomplete="current-password" required>
        <button type="submit" class="btn-primary" ?disabled=${this.busy}>로그인</button>
        ${this.error ? html`<p class="error">${this.error}</p>` : nothing}
      </form>
    `;
  }
}
