import './app-root.js';
import { applyTheme, currentTheme } from './theme.js';

// 저장된 테마 적용 (자동이면 시스템 설정 따름)
applyTheme(currentTheme());

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
