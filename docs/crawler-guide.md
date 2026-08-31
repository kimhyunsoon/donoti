# 크롤러 작성 가이드 (에이전트용)

감시(크롤링) 기능을 구현·확장할 때의 작업 절차와 이전 세대(emart-stock)에서 배운 것들.

## 알림 파이프라인

```
생산자 (크롤러·API)  ──►  notifications 테이블 (pending)  ──►  워커 (5초 폴링)
                                                             │
                                          ┌──────────────────┤
                                          │ 성공 → sent      └ Web Push 브로드캐스트
                                          └ 실패 → 백오프 재시도 (10초→1분→5분→15분, 최대 5회)
                                                   → 소진 시 failed (수동 재시도: POST /api/notifications/:id/retry)
```

- 발송 지점은 backend 프로세스 내장 워커(`notify.ts`) 하나뿐. 생산자는 `enqueueNotification()`만 호출한다.
- 만료된 푸시 구독(404/410)은 발송 시 자동 정리된다.
- 크롤러 주기는 `watches.schedule`(JSON), provider별 폼 데이터는 `watches.config`(JSON)에 저장한다. 크롤러 전용 런타임 상태는 `watches.state`. 새 크롤러는 `crawlers/<provider>.ts`에 `Crawler{run, validateConfig}` 구현 후 `crawlers/index.ts` 레지스트리에 등록 - 전체 구조는 architecture.md 참고.

## 크롤링 앱 작성 시 디버깅 순서

셀렉터를 코드에 넣기 전에 반드시 실제 응답에서 확인할 것.

1. **정적 fetch 먼저**: `curl -sL -A "<모바일 UA>" <URL>` 로 받아서 목표 셀렉터가 초기 HTML 에 있는지 `grep`. 있으면 fetch + cheerio 로 충분.
2. **단축링크·브릿지 페이지 주의**: 예컨대 `emart.kr/*` 단축링크는 앱 유도용 브릿지 페이지라 본문이 없다. HTML 안의 리다이렉트 스크립트에서 실제 URL 을 찾아 그 URL 을 크롤링 대상으로 쓴다.
3. **SPA 면 headless chromium**: 셀렉터가 초기 HTML 에 없으면 (Vue/React 마운트 지점 `<div id="webapp">` 등만 있으면) JS 렌더링이 필요하다:
   ```bash
   chromium --headless=new --disable-gpu --no-sandbox \
     --user-data-dir=/tmp/앱이름-chrome --virtual-time-budget=15000 \
     --user-agent="<모바일 UA>" --dump-dom "<URL>" > rendered.html
   ```
   렌더링된 DOM 에서 셀렉터와 실제 텍스트를 확인한 뒤 코드를 작성한다 (execFile + cheerio 패턴, 실측 1~2초/회라 1분 주기도 문제없다).
4. **파싱 실패 대비**: 페이지 구조가 바뀌어 셀렉터를 못 찾으면 조용히 실패하지 말고 알림으로 1회 경고를 보낸다 (상태 플래그로 중복 방지).

## 테스트 발송 요령

실제 알림 조건을 인위적으로 만들기 어려울 때 (예: 품절 상품의 재고 알림), 조건을 임시로 반전시켜 알림이 큐를 거쳐 기기까지 도착하는지 확인한 뒤 원복한다. 반전 코드가 배포된 동안은 매 주기 알림이 오는 것이 정상이므로, 확인 후 바로 원복할 것.

## 자주 만나는 문제

| 증상 | 원인 |
|---|---|
| 큐에 쌓이는데 발송 안 됨 | backend 미가동 또는 푸시 구독 0개 (`push_subscriptions` 확인) |
| 알림이 특정 기기에만 안 옴 | 구독 만료 → 홈의 "이 기기에서 알림 받기" 재실행 |
| 로컬(http)에서 푸시 구독 실패 | Web Push 는 https 또는 localhost 에서만 동작 |
| 재배포 후 구독 전부 무효 | 데이터 디렉토리의 `.vapid.json` 유실 (볼륨 확인) |
