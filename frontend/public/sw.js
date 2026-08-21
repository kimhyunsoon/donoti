// 최소 서비스워커 — 설치 가능 조건 충족 + 푸시 수신 + 앱 아이콘 배지
self.addEventListener('install', () => {
  self.skipWaiting();
});

self.addEventListener('activate', (event) => {
  event.waitUntil(self.clients.claim());
});

self.addEventListener('push', (event) => {
  const data = event.data ? event.data.json() : {};
  const jobs = [
    self.registration.showNotification(data.title ?? '두노티', {
      body: data.body ?? '',
      icon: '/icons/icon-192.png',
      data: { url: data.url ?? '/' },
    }),
  ];
  // 앱 아이콘 배지 (iOS 16.4+ 설치형 PWA·안드로이드 크롬 지원) - 안읽은 수는 서버가 계산해 페이로드에 포함
  if ('setAppBadge' in self.navigator && typeof data.unread === 'number') {
    jobs.push(self.navigator.setAppBadge(data.unread).catch(() => {}));
  }
  event.waitUntil(Promise.all(jobs));
});

self.addEventListener('notificationclick', (event) => {
  event.notification.close();
  event.waitUntil(self.clients.openWindow(event.notification.data?.url ?? '/'));
});
