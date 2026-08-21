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

// 알림 권한 확인 후 이 기기를 푸시 구독시킨다. 성공 여부 반환
export async function ensureSubscribed(): Promise<boolean> {
  if (!('serviceWorker' in navigator) || !('PushManager' in window) || !('Notification' in window)) {
    return false;
  }
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
