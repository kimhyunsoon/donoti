import { LitElement, html, css, type TemplateResult } from 'lit';
import { customElement, state } from 'lit/decorators.js';
import { keyed } from 'lit/directives/keyed.js';
import { tokens } from './style.js';
import './views/login-view.js';
import './views/home-view.js';
import './views/settings-view.js';
import './views/notifications-view.js';
import './views/watch-form-view.js';
import './views/trash-view.js';

// 해시 라우터: #/login #/settings #/notifications #/watch/* #/trash, 그 외 전부 → 홈
@customElement('app-root')
export class AppRoot extends LitElement {
  static styles = [
    tokens,
    css`
      :host {
        display: block;
        min-height: 100dvh;
        background: var(--bg);
      }
      .view {
        animation: view-in 0.22s ease-out;
      }
      @keyframes view-in {
        from { opacity: 0; transform: translateY(10px); }
        to { opacity: 1; transform: none; }
      }
    `,
  ];

  @state() private route = location.hash || '#/home';

  connectedCallback(): void {
    super.connectedCallback();
    window.addEventListener('hashchange', this.onHashChange);
  }

  disconnectedCallback(): void {
    window.removeEventListener('hashchange', this.onHashChange);
    super.disconnectedCallback();
  }

  private onHashChange = (): void => {
    this.route = location.hash || '#/home';
  };

  private renderView(path: string): TemplateResult {
    if (path.startsWith('#/login')) return html`<login-view></login-view>`;
    if (path.startsWith('#/settings')) return html`<settings-view></settings-view>`;
    if (path.startsWith('#/notifications')) return html`<notifications-view></notifications-view>`;
    if (path.startsWith('#/watch/')) return html`<watch-form-view></watch-form-view>`;
    if (path.startsWith('#/trash')) return html`<trash-view></trash-view>`;
    return html`<home-view></home-view>`;
  }

  render(): TemplateResult {
    // 쿼리 제외한 경로만 keyed 키로 사용
    const path = this.route.split('?')[0] ?? '#/home';
    return html`${keyed(path, html`<div class="view">${this.renderView(path)}</div>`)}`;
  }
}
