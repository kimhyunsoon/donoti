import './app-root.js';
import { applyTheme, currentTheme } from './theme.js';

// 저장된 테마 적용 (자동이면 시스템 설정 따름)
applyTheme(currentTheme());

// 서비스워커 등록 (푸시·홈화면 설치용)
if ('serviceWorker' in navigator) {
  navigator.serviceWorker.register('/sw.js');
}
