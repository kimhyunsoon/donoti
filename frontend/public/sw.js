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
      // ids: 같은 종류가 합쳐진 알림 - 클릭 시 전부 읽음 처리
      data: { id: data.id ?? null, ids: data.ids ?? null },
    }),
  ];
  // 앱 아이콘 배지 (iOS 16.4+ 설치형 PWA·안드로이드 크롬 지원) - 안읽은 수는 서버가 계산해 페이로드에 포함
  if ('setAppBadge' in self.navigator && typeof data.unread === 'number') {
    jobs.push(self.navigator.setAppBadge(data.unread).catch(() => {}));
  }
  // 열려 있는 앱 화면(홈·알림센터)에 새 알림 도착을 전달해 즉시 갱신되게 한다
  jobs.push(
    self.clients
      .matchAll({ type: 'window', includeUncontrolled: true })
      .then((windows) => windows.forEach((client) => client.postMessage({ type: 'push' })))
      .catch(() => {}),
  );
  event.waitUntil(Promise.all(jobs));
});

// OS 알림 클릭 → 해당 알림 읽음 처리 + 앱 배지 갱신 + 알림센터로 이동
self.addEventListener('notificationclick', (event) => {
  event.notification.close();
  const data = event.notification.data ?? {};
  const ids = Array.isArray(data.ids) ? data.ids : data.id ? [data.id] : [];
  event.waitUntil(
    (async () => {
      let unread = null;
      for (const id of ids) {
        try {
          const res = await fetch(`/api/notifications/${id}/read`, {
            method: 'POST',
            credentials: 'include',
          });
          const body = await res.json();
          if (typeof body.unread === 'number') unread = body.unread;
        } catch {
          // 읽음 처리 실패는 무시 - 알림센터 이동은 계속
        }
      }
      if ('setAppBadge' in self.navigator && typeof unread === 'number') {
        await (unread > 0
          ? self.navigator.setAppBadge(unread)
          : self.navigator.clearAppBadge()
        ).catch(() => {});
      }
      const windows = await self.clients.matchAll({ type: 'window', includeUncontrolled: true });
      const client = windows[0];
      if (client) {
        // navigate()는 iOS PWA에서 실패하므로 앱에 메시지를 보내 해시 라우팅으로 이동시킨다
        try { await client.focus(); } catch (e) { /* 포커스 실패 무시 */ }
        client.postMessage({ type: 'open-notifications' });
      } else {
        await self.clients.openWindow('/#/notifications');
      }
    })(),
  );
});
