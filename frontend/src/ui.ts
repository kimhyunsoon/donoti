// 전역 토스트 (Shadow DOM 밖 body에 부착 - 어느 뷰에서든 사용)
let activeToast: HTMLElement | null = null;

const TOAST_STYLE =
  'position:fixed;left:50%;transform:translateX(-50%);bottom:calc(28px + env(safe-area-inset-bottom));' +
  'background:rgba(25,31,40,0.92);color:#fff;padding:12px 20px;border-radius:14px;' +
  'font-size:0.92rem;font-weight:600;z-index:1000;max-width:86vw;display:flex;gap:12px;align-items:center;' +
  "font-family:-apple-system,BlinkMacSystemFont,'Apple SD Gothic Neo',sans-serif;";

export function toast(message: string, duration = 2200): void {
  activeToast?.remove();
  const el = document.createElement('div');
  el.style.cssText = TOAST_STYLE;
  el.textContent = message;
  document.body.appendChild(el);
  activeToast = el;
  el.animate(
    [
      { opacity: 0, transform: 'translateX(-50%) translateY(12px)' },
      { opacity: 1, transform: 'translateX(-50%)' },
    ],
    { duration: 200, easing: 'ease-out' },
  );
  setTimeout(() => {
    el.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 180, fill: 'forwards' }).finished.then(
      () => el.remove(),
    );
  }, duration);
}
