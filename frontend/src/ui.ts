// 라이트 DOM 공용 UI 헬퍼 - 모달 히스토리 스택, 배경 스크롤 잠금, 토스트

// ── 배경 스크롤 잠금 (iOS 대응: body를 fixed로 고정) ─────────────
let lockCount = 0;
let savedScrollY = 0;

function lockScroll(): void {
  if (++lockCount > 1) return;
  savedScrollY = window.scrollY;
  document.body.style.position = 'fixed';
  document.body.style.top = `-${savedScrollY}px`;
  document.body.style.width = '100%';
}

function unlockScroll(): void {
  if (lockCount === 0) return;
  if (--lockCount > 0) return;
  document.body.style.position = '';
  document.body.style.top = '';
  document.body.style.width = '';
  window.scrollTo(0, savedScrollY);
}

// ── 모달 히스토리 스택 ──────────────────────────────────────────
// 모든 모달 닫기는 history.back()을 경유한다.
// → iOS 엣지 스와이프(뒤로가기) = popstate와 완전히 같은 경로로 닫혀 상태가 꼬이지 않음.
interface ModalEntry {
  close: () => void;
}
const modalStack: ModalEntry[] = [];

window.addEventListener('popstate', () => {
  const entry = modalStack.pop();
  if (entry) {
    unlockScroll();
    entry.close();
  }
});

// 안전망: 모달이 열린 채 라우트가 강제로 바뀌면(예: 세션 만료 → 로그인 전환)
// 모달 요소는 뷰와 함께 사라지므로 스택과 스크롤 잠금만 정리한다
window.addEventListener('hashchange', () => {
  while (modalStack.length > 0) {
    modalStack.pop();
    unlockScroll();
  }
});

/**
 * 모달을 히스토리에 등록한다. 열려있는 동안 배경 스크롤이 잠긴다.
 * @param onClose 닫힐 때(뒤로가기·스와이프·requestClose) 한 번 실행
 * @returns requestClose - 호출하면 history.back()으로 닫힌다
 */
export function pushModal(onClose: () => void): () => void {
  const entry: ModalEntry = { close: onClose };
  modalStack.push(entry);
  lockScroll();
  history.pushState({ modal: modalStack.length }, '');
  return (): void => {
    if (modalStack.includes(entry)) history.back();
  };
}

// 전역 토스트 (Shadow DOM 밖 body에 부착 - 어느 뷰에서든 사용)
let activeToast: HTMLElement | null = null;

const TOAST_STYLE =
  'position:fixed;left:50%;transform:translateX(-50%);bottom:calc(28px + env(safe-area-inset-bottom));' +
  'background:rgba(25,31,40,0.92);color:#fff;padding:12px 20px;border-radius:14px;' +
  'font-size:0.92rem;font-weight:600;z-index:1000;max-width:86vw;display:flex;gap:12px;align-items:center;' +
  "font-family:-apple-system,BlinkMacSystemFont,'Apple SD Gothic Neo',sans-serif;";

export function toast(message: string, duration = 4000): void {
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
