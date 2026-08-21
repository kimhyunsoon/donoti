import { css } from 'lit';

// 컴포넌트 공통 기반 - 테마 색상 변수는 index.html의 :root에서 상속받는다
export const tokens = css`
  :host {
    font-family: -apple-system, BlinkMacSystemFont, 'Apple SD Gothic Neo', sans-serif;
    color: var(--text);
    -webkit-tap-highlight-color: transparent;
  }
  * {
    box-sizing: border-box;
  }
`;

// 공용 컴포넌트 스타일 (버튼·입력·카드)
export const ui = css`
  button, .btn {
    font: inherit;
    border: none;
    cursor: pointer;
    -webkit-touch-callout: none;
    user-select: none;
  }
  .btn-primary {
    background: var(--accent);
    color: #fff;
    border-radius: 12px;
    padding: 13px 18px;
    font-size: 1rem;
    font-weight: 700;
    width: 100%;
  }
  .btn-soft {
    background: var(--accent-soft);
    color: var(--accent);
    border-radius: 10px;
    padding: 9px 14px;
    font-weight: 700;
    font-size: 0.9rem;
  }
  .btn-ghost {
    background: none;
    color: var(--text-sub);
    padding: 8px;
    font-size: 0.88rem;
  }
  input, select, textarea {
    font: inherit;
    color: var(--text);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 11px 12px;
    width: 100%;
  }
  input:focus, select:focus, textarea:focus {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
    border-color: transparent;
  }
  .card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 14px;
    padding: 14px 16px;
  }
  .sub {
    color: var(--text-sub);
    font-size: 0.85rem;
  }
  a {
    color: inherit;
    text-decoration: none;
  }
`;
