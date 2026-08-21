import './app-root.js';
import { applyTheme, currentTheme } from './theme.js';

// 저장된 테마 적용 (자동이면 시스템 설정 따름)
applyTheme(currentTheme());

// 가장자리 좌우 스와이프로 뒤로·앞으로 가기 (iOS PWA에는 네이티브 엣지 스와이프가 없다)
// 화면 안쪽에서 시작하는 스와이프는 무시 - 목록 아이템의 스와이프 제스처(삭제·읽음)와 충돌 방지
{
  const EDGE = 28; // 제스처를 인식하는 가장자리 폭
  const DISTANCE = 70; // 발동 이동 거리
  let edge: 'left' | 'right' | null = null;
  let startX = 0;
  let startY = 0;

  window.addEventListener(
    'touchstart',
    (e: TouchEvent) => {
      const touch = e.touches[0];
      if (!touch) return;
      startX = touch.clientX;
      startY = touch.clientY;
      edge = startX <= EDGE ? 'left' : startX >= window.innerWidth - EDGE ? 'right' : null;
    },
    { passive: true },
  );

  window.addEventListener(
    'touchend',
    (e: TouchEvent) => {
      if (edge === null) return;
      const touch = e.changedTouches[0];
      if (!touch) return;
      const dx = touch.clientX - startX;
      const dy = touch.clientY - startY;
      if (Math.abs(dx) < DISTANCE || Math.abs(dx) < Math.abs(dy) * 1.5) return;
      if (edge === 'left' && dx > 0) history.back();
      else if (edge === 'right' && dx < 0) history.forward();
      edge = null;
    },
    { passive: true },
  );
}

// 서비스워커 등록 (푸시·홈화면 설치용)
if ('serviceWorker' in navigator) {
  navigator.serviceWorker.register('/sw.js');
  // OS 알림 클릭 → 서비스워커가 보내는 메시지로 알림센터 이동
  navigator.serviceWorker.addEventListener('message', (e: MessageEvent) => {
    if ((e.data as { type?: string })?.type === 'open-notifications') {
      location.hash = '#/notifications';
    }
  });
}
