import { api } from './api.js';

// VAPID 공개키(base64url) → PushManager가 요구하는 BufferSource (ArrayBuffer 기반이어야 함)
function vapidKeyToUint8(key: string): Uint8Array<ArrayBuffer> {
  const padding = '='.repeat((4 - (key.length % 4)) % 4);
  const base64 = (key + padding).replace(/-/g, '+').replace(/_/g, '/');
  const raw = atob(base64);
  const bytes = new Uint8Array(new ArrayBuffer(raw.length));
  for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);
  return bytes;
}

function pushSupported(): boolean {
  return 'serviceWorker' in navigator && 'PushManager' in window && 'Notification' in window;
}

// 알림 권한 확인 후 이 기기를 푸시 구독시킨다. 성공 여부 반환
export async function ensureSubscribed(): Promise<boolean> {
  if (!pushSupported()) return false;
  if (Notification.permission === 'denied') return false;
  const permission =
    Notification.permission === 'granted' ? 'granted' : await Notification.requestPermission();
  if (permission !== 'granted') return false;
  const { key } = await api<{ key: string }>('/api/push/vapid-public-key');
  const reg = await navigator.serviceWorker.ready;
  const sub = await reg.pushManager.subscribe({
    userVisibleOnly: true,
    applicationServerKey: vapidKeyToUint8(key),
  });
  await api('/api/push/subscriptions', { method: 'POST', body: JSON.stringify(sub.toJSON()) });
  return true;
}

// 알림을 켰을 때 확인용 브라우저 알림 (서버 왕복 없이 이 기기에만 표시)
export async function showEnabledNotification(): Promise<void> {
  if (!pushSupported() || Notification.permission !== 'granted') return;
  const reg = await navigator.serviceWorker.ready;
  await reg.showNotification('두노티', {
    body: '알림이 켜졌어요. 이 기기로 알림을 보내드려요',
    icon: '/icons/icon-192.png',
  });
}

// 이 기기의 구독을 해지한다 (설정 스위치 끔)
export async function unsubscribe(): Promise<void> {
  if (!pushSupported()) return;
  const reg = await navigator.serviceWorker.ready;
  const sub = await reg.pushManager.getSubscription();
  if (!sub) return;
  await api('/api/push/subscriptions', {
    method: 'DELETE',
    body: JSON.stringify({ endpoint: sub.endpoint }),
  });
  await sub.unsubscribe();
}

// 이 기기가 현재 구독 중인지 (설정 스위치 초기 상태)
export async function isSubscribed(): Promise<boolean> {
  if (!pushSupported() || Notification.permission !== 'granted') return false;
  const reg = await navigator.serviceWorker.ready;
  return (await reg.pushManager.getSubscription()) !== null;
}

// 첫 진입 시 1회: 알림 권한을 요청해 이 기기를 구독시킨다
// (iOS는 사용자 제스처 없이는 권한 요청이 안 되므로, 그 경우 설정의 스위치가 같은 역할)
export async function setupPushOnce(): Promise<void> {
  if (localStorage.getItem('push-setup') === '1') return;
  try {
    await ensureSubscribed();
    localStorage.setItem('push-setup', '1');
  } catch {
    // 미지원·거부는 무시 - 설정 스위치로 언제든 다시 켤 수 있음
  }
}
