import { css } from 'lit';

// 컴포넌트 공통 기반 - 테마 색상 변수는 index.html의 :root에서 상속받는다
export const tokens = css`
  :host {
    font-family: -apple-system, BlinkMacSystemFont, 'Apple SD Gothic Neo', 'Pretendard', sans-serif;
    color: var(--text);
    -webkit-tap-highlight-color: transparent;
  }
  * {
    box-sizing: border-box;
  }
`;

// 공용 컴포넌트 스타일 - 토스 스타일 (무테두리 흰 카드, 큰 라운드, 굵은 타이포)
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
    border-radius: 14px;
    padding: 15px 18px;
    font-size: 1rem;
    font-weight: 700;
    width: 100%;
    transition: transform 0.08s, filter 0.08s;
  }
  .btn-primary:active { transform: scale(0.98); filter: brightness(0.95); }
  .btn-primary:disabled { opacity: 0.5; }
  .btn-soft {
    background: var(--accent-soft);
    color: var(--accent);
    border-radius: 12px;
    padding: 11px 16px;
    font-weight: 700;
    font-size: 0.92rem;
    transition: transform 0.08s;
  }
  .btn-soft:active { transform: scale(0.97); }
  .btn-ghost {
    background: none;
    color: var(--text-sub);
    padding: 8px;
    font-size: 0.88rem;
    font-weight: 600;
  }
  /* 헤더 등 원형 아이콘 버튼 */
  .btn-icon {
    background: none;
    color: var(--text);
    width: 38px;
    height: 38px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    position: relative;
    transition: background 0.12s;
  }
  .btn-icon:active { background: var(--surface-weak); }
  input, select, textarea {
    font: inherit;
    color: var(--text);
    background: var(--surface);
    border: none;
    border-radius: 14px;
    padding: 14px 16px;
    width: 100%;
  }
  input::placeholder { color: var(--text-sub); }
  input:focus, select:focus, textarea:focus {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .card {
    background: var(--surface);
    border-radius: 20px;
    padding: 18px 20px;
  }
  .sub {
    color: var(--text-sub);
    font-size: 0.85rem;
  }
  a {
    color: inherit;
    text-decoration: none;
  }
  /* 카드 안 리스트 행 */
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 9px 0;
    font-weight: 500;
  }
  /* 애플 스타일 세그먼티드 컨트롤 */
  .segmented {
    display: flex;
    background: var(--bg);
    border-radius: 12px;
    padding: 3px;
    gap: 2px;
  }
  .segmented button {
    flex: 1;
    padding: 7px 0;
    border-radius: 9px;
    background: none;
    color: var(--text-sub);
    font-size: 0.86rem;
    font-weight: 600;
  }
  .segmented button.on {
    background: var(--surface);
    color: var(--text);
    font-weight: 700;
    box-shadow: 0 1px 4px rgba(0, 0, 0, 0.1);
  }
  /* 토글 스위치 */
  .switch { position: relative; width: 48px; height: 28px; flex-shrink: 0; }
  .switch input { opacity: 0; width: 100%; height: 100%; margin: 0; position: absolute; z-index: 1; padding: 0; }
  .switch .knob {
    position: absolute; inset: 0; border-radius: 999px; background: var(--border); transition: 0.15s;
  }
  .switch .knob::after {
    content: ''; position: absolute; top: 3px; left: 3px;
    width: 22px; height: 22px; border-radius: 50%; background: #fff;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.15); transition: 0.15s;
  }
  .switch input:checked + .knob { background: var(--switch-on); }
  .switch input:checked + .knob::after { transform: translateX(20px); }
  /* 설정 폼 행 (iOS 스타일: 좌측 라벨 + 우측 컨트롤) */
  .frow {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    min-height: 40px;
  }
  .frow .lbl {
    color: var(--text-sub);
    font-size: 0.86rem;
    font-weight: 600;
    flex-shrink: 0;
  }
  .frow .ctl {
    display: flex;
    align-items: center;
    gap: 6px;
    font-weight: 600;
    font-size: 0.92rem;
  }
  /* 상단 바 (뒤로가기 + 제목) */
  .top {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: calc(10px + env(safe-area-inset-top)) 10px 10px;
    position: sticky;
    top: 0;
    background: var(--bg);
    z-index: 5;
  }
  .top h1 { font-size: 1.2rem; font-weight: 700; margin: 0; }
  /* 로딩 스켈레톤 */
  .skeleton {
    background: linear-gradient(90deg, var(--surface-weak) 25%, var(--border) 50%, var(--surface-weak) 75%);
    background-size: 200% 100%;
    animation: shimmer 1.2s infinite;
    border-radius: 10px;
  }
  @keyframes shimmer {
    from { background-position: 200% 0; }
    to { background-position: -200% 0; }
  }
`;

// 바텀시트 공통 스타일 (sheets/sheet-base.ts 기반 컴포넌트용)
export const sheet = css`
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.4);
    z-index: 100;
    display: flex;
    align-items: flex-end;
    animation: fade-in 0.18s;
    transition: opacity 0.18s;
  }
  .overlay.closing { opacity: 0; }
  .panel {
    background: var(--bg);
    width: 100%;
    max-width: 480px;
    margin: 0 auto;
    max-height: 88dvh;
    border-radius: 18px 18px 0 0;
    display: flex;
    flex-direction: column;
    animation: slide-up 0.22s cubic-bezier(0.2, 0.9, 0.3, 1);
    transition: transform 0.18s ease-out;
  }
  .overlay.closing .panel { transform: translateY(105%); transition: transform 0.18s ease-in; }
  .drag-zone {
    touch-action: none;
    flex-shrink: 0;
    cursor: grab;
  }
  .grabber {
    width: 38px;
    height: 5px;
    border-radius: 999px;
    /* --border는 라이트에서 너무 옅어 안 보인다 - 반투명 text-sub로 살짝 진하게 */
    background: color-mix(in srgb, var(--text-sub) 45%, transparent);
    margin: 8px auto 2px;
  }
  .panel-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 16px;
  }
  .panel-head h2 { font-size: 1.05rem; margin: 0; }
  .panel-body {
    overflow-y: auto;
    /* 하단은 홈 인디케이터(safe-area)만큼만 - 여백을 더 주면 완료 버튼이 떠 보인다 */
    padding: 0 16px max(10px, env(safe-area-inset-bottom, 10px));
    -webkit-overflow-scrolling: touch;
  }
  @keyframes slide-up { from { transform: translateY(45%); opacity: 0.6; } }
  @keyframes fade-in { from { opacity: 0; } }
`;
