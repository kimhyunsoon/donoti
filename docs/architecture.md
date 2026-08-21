# donoti 아키텍처

## 스택

- **backend**: TypeScript ESM + Fastify 5 + better-sqlite3(WAL). 포트 4646 (상수). 세션 쿠키 인증(argon2id + SQLite 세션 스토어), Web Push(VAPID).
- **frontend**: Lit 3 + Vite 6. dev 포트 4647, `/api` 프록시로 로컬·프로덕션 모두 동일 오리진 (CORS 불필요).
- **DB**: 단일 SQLite 파일 (`{DATA_DIR}/donoti.db`). 마이그레이션 도구 없음 - `schema.sql`(IF NOT EXISTS)을 기동 시마다 적용, 기존 DB 컬럼 추가는 `db.ts`의 `migrate()`에 `PRAGMA table_info` 분기로.
- **시크릿**: 세션 시크릿·VAPID 키쌍은 최초 기동 시 자동 생성해 데이터 디렉토리에 보관 (`.session-secret`, `.vapid.json`). env로 관리할 시크릿은 초기 계정뿐.

## 테이블

| 테이블 | 요점 |
|---|---|
| `users` | 초기 계정 1개 (env로 부트스트랩, 회원가입 없음) |
| `sessions` | session-store.ts가 자체 생성. 재배포에도 로그인 유지 |
| `notifications` | 알림 발송 큐. status(pending/sending/sent/failed) + attempts + next_attempt_at(epoch ms) |
| `push_subscriptions` | 기기별 Web Push 구독 (만료 시 자동 정리) |
| `watches` | 감시 알림 (2뎁스 category>provider). schedule(JSON, KST), config(provider별 폼), enabled(임시 중지), ends_at(종료 시점), deleted_at(soft delete·복구 가능) |
| `stock_symbols` | 주식 종목 마스터 (유명 종목만). `crawlers/stock-symbols.ts` 시드가 기동 시마다 upsert. kind(kr/us/index/etf) + market(domestic/world, 네이버 API 라우팅) |
| `settings` | 범용 key-value. 스키마 변경 없는 확장용 |

## 감시 알림 & 크롤링

- **카탈로그**: 쇼핑(inventory: emart) / 영화(movie: cgv·lotte·megabox) / 시세(price: fx·stock). `backend/src/catalog.ts` ↔ `frontend/src/catalog.ts` 동기 유지. 브랜드 아이콘은 `frontend/public/brands/*.jpg`(공식 앱 아이콘 128px, iTunes Search API로 확보).
- **스케줄**: `ScheduleRule[]` JSON — 규칙마다 요일(0~6) + `times`(특정 시각 목록) 또는 `interval`(start~end, every분마다). KST 24시간제 'HH:MM' 고정. 검증·매칭은 `backend/src/watch-schedule.ts`, 프론트 요약은 `frontend/src/schedule.ts`.
- **스케줄러**: `backend/src/scheduler.ts`가 매분 0초(KST)에 (1) `ends_at` 지난 알림 soft delete, (2) 스케줄 일치 watch를 **provider별로 묶어** 크롤러 배치 1회 실행 (알림당 1크롤링이 아니라 종류당 1크롤링). `enabled=0`(임시 중지)은 제외. 크롤러는 `backend/src/crawlers/index.ts` 레지스트리에 provider별 `Crawler`(run 배치 실행 + validateConfig 등록 검증)로 등록. run의 책무: 감지 시 `enqueueNotification()` 호출 + 필요 시 `watch.url`(리다이렉트 URL) 자동 설정.
- **환율(fx) 크롤러**: `crawlers/fx.ts` — 네이버 금융 front-api(`m.stock.naver.com/front-api/marketIndex/productDetail`, 하나은행 고시·장중 갱신). 통화 7종(USD·JPY·EUR·CNY·IDR·TWD·HKD, 엔·루피아는 100 단위 고시), config `{currency}`, 1통화 1알림(json_extract 중복 검사), 리다이렉트는 네이버 모바일 환율 상세. 같은 회차 알림은 통화별 1회만 조회.
- **주식(stock) 크롤러**: `crawlers/stock.ts` — 네이버 증권 시세 API. 국내(주식·ETF·지수)는 `m.stock.naver.com/api/{stock|index}/{code}/basic`, 해외는 `api.stock.naver.com/{stock|index}/{code}/basic`. 코드 규칙: 국내 6자리, 나스닥 `.O`, NYSE 무접미사(일부 `.K`), 해외지수 `.` 접두사 — 시드에 넣기 전 반드시 API로 코드 검증할 것. config `{code, name, kind}`(크롤러는 code로 stock_symbols 조회), 1종목 1알림. 표기: 국내 '278,500원' / 해외 '$311.30' / 지수 '6,890.01'.
- **이마트(emart) 크롤러**: `crawlers/emart.ts` — 이마트 앱 공유 링크 기반, 1알림 = 1상품 + N지점. 링크 해석: `emart.kr` 단축 → 302 Location의 `appLink.do?link=` 파싱. 지원 2유형: **digital**(디지털그랩 픽업, `panda/api/v1/digital-grab/item?skuCode&storeCode` + 취급지점 `digital-grab/store?skuCode`, 구매가능=`buyLimit.maxBuyUnlimited||maxBuyAvailableCount>0`) / **product**(일반 매장 상품, `product/api/v1/info/1100/{sku}?storeId=`(POST {}) + 전지점 `search/api/v1/store/branch?storeType=E` + **지점별 재고 일괄** `store/stock/branch?storeType=E&skuCode=`). 오더픽·와인그랩 등은 등록 시 거부(앱 전용, 비로그인 500). **주의: digital storeCode(구로 3400)와 product storeId(은평 1033)는 다른 체계**. 알림은 구매가능 지점이 있으면 매회 발송(상태 무관), config `{type,sku,name,link,stores[],endOnStock}` — endOnStock이면 알림 후 soft delete. 등록 폼용 `POST /api/emart/resolve {url}`.
- **CGV(cgv) 크롤러**: `crawlers/cgv.ts` — 영화 1편 × 지점 N × 날짜 N × 시간범위(상영 시작시간 기준)의 잔여석 감시, **알림은 지점당 1건**, 예매 가능한 회차(잔여석>0)가 있을 때만. 본문은 날짜 헤더(`8/21 (금)`) 아래 시간순으로 `17:05 1관 (Laser) · 92석` 줄 나열(최대 8회+외 N회). API: **쿠키(__cf_bm 등) 없는 반복 호출은 간헐적으로 200+HTML 에러 페이지로 차단** — 크롤러가 예매 페이지에서 쿠키를 받아 유지하고, 비JSON 응답 감지 시 쿠키 갱신 후 1회 재시도(fetchJson). 상영작 `cgv.co.kr/api/v1/booking/searchAtktTopPostrList`, 지점 `content/site/searchAllRegionAndSite`(siteNo 4자리), 상영표+잔여석 `booking/searchMovScnInfo?siteNo&scnYmd=YYYYMMDD&rtctlScopCd=08`(frSeatCnt=잔여석, scnsrtTm='1705'). config `{movieNo,movieName,stores[],dates[],start,end}`. `ends_at`은 마지막 선택 날짜+종료시각으로 **자동 설정**(프론트 buildPayload). 회차 내 (지점,날짜) 상영표는 캐시로 1회만 조회. `watches.state` 컬럼은 크롤러 전용 상태 저장용으로 예약(현재 미사용).
- **롯데시네마(lotte) 크롤러**: `crawlers/lotte.ts` — CGV와 동일 방식(지점당 1건, 잔여석>0만, 날짜 헤더+시간순). API: `www.lottecinema.co.kr/LCWS/Ticketing/TicketingData.aspx`에 multipart form `paramList` JSON POST(쿠키 불필요). **공통 파라미터에 `memberOnNo:'0'` 필수**(없으면 '호출 파라미터가 부족합니다'). `GetTicketingPageTOBE` 1콜로 영화(RepresentationMovieCode)+지점(DivisionCode 1만, code=`div|detail|cinemaID`)+지역, `GetPlaySequence{playDate,cinemaID,representationMovieCode:''}`가 상영표 — **BookingSeatCount=잔여석**(UI '잔여석 144/164' 대조 확인). 특별관은 ScreenDivisionNameKR('일반' 아니면 표기). 프론트는 CGV 폼을 MOVIE_PROVIDERS(['cgv','lotte'])로 일반화해 공유(IMAX 스위치는 cgv만), 라우트 `/api/lotte/movies`·`/theaters`.
- **메가박스(megabox) 크롤러**: `crawlers/megabox.ts` — CGV·롯데와 동일 방식. API: `megabox.co.kr/on/oh/ohb/SimpleBooking/selectBokdList.do`에 JSON POST(+`X-Requested-With: XMLHttpRequest`, 쿠키 불필요). onLoad:'Y' 1콜로 영화(movieNo)+지점(brchNo·areaCd) 목록, 상영표는 `{arrMovieNo,playDe:YYYYMMDD,brchNoListCnt:1,brchNo1,areaCd1,spclbYn1:'N',theabKindCd1:'10',movieNo1}` → movieFormList(**restSeatCnt=잔여석**, playStartTime, theabExpoNm, playKindNm은 HTML 엔티티 &#40;&#41; 디코딩 필요). **지점 code는 'brchNo|areaCd'**(상영표 조회에 areaCd 필수). 라우트 `/api/megabox/movies`·`/theaters`, 프론트 MOVIE_PROVIDERS에 포함.
- **수집 재시도·실패 처리**: 모든 크롤러의 외부 호출은 `crawlers/retry.ts`의 `withRetry`로 최대 3회 시도(대기 1초→2초, 4xx는 정상 흐름이라 즉시 중단, CGV는 2회차부터 쿠키 갱신 전략). 중간 실패와 최종 실패는 `crawl-log.ts`가 **일자별(KST) 파일** `{DATA_DIR}/logs/crawl-YYYY-MM-DD.log`에 기록. 최종 실패 시 해당 watch로 `⚠️ 수집 실패` 알림 발송 — 연속 실패는 `watch.state.failing`으로 최초 1회만 알리고, 수집이 복구되면 해제되어 다음 실패 때 다시 알린다.
- **알림 본문 공통 형식**: 줄바꿈 기반 — fx/stock `1,381원\n전일 대비 -0.9%`, emart `구매 가능 · 87,840원\n구로점\n...`(지점당 한 줄, 최대 6+외 N곳; product는 `은평점 · 2개`), 홈 목록 최근 알림은 `white-space: pre-line`으로 줄바꿈 그대로 표시.
- **API**: `/api/watches` CRUD + `/deleted`(등록 기준 최근 100) + `/:id/restore`(복구 시 상시로) + `/:id/toggle`(임시 중지·재개), `/api/stocks?q=`(종목 검색 - 코드·이름·keywords LIKE, 20건), `/api/emart/resolve`(공유 링크 해석), `/api/cgv/movies`·`/api/cgv/theaters`(등록 폼용 프록시). 알림센터(`/api/notifications`)는 최근 100개 고정.
- **프론트 라우트**: 홈(목록 - 다음 실행 카운트다운·주기 요약·최근 알림 내용/상대시간·중지 토글) / `#/watch/new`(2뎁스 픽커) / `#/watch/new/<provider>`(공통 등록 폼 + provider별 설정 섹션: fx는 통화 칩, stock은 종목 검색(디바운스 250ms) - 자동 명명 '달러 환율'/'삼성전자 주가'/'코스피 지수') / `#/watch/edit?id=N` / `#/trash`(종료된 알림·복구, 설정에서 진입). 영화·재고 provider 상세 폼·크롤러는 다음 단계에서 개별 구현.
- **바텀시트(dogroo 패턴)**: `sheets/sheet-base.ts`(pushModal 히스토리 스택 + 스크롤 잠금 + 드래그 닫기, ui.ts·style.ts `sheet` css) 위에 `time-wheel-sheet`(24시간제 시/분 휠, hourOnly 옵션)와 `calendar-sheet`(월 달력, 오늘 점 표기, min 이전 비활성). 시각·날짜 입력은 전부 이 시트를 쓴다. 기본값: 특정 시각=현재 시각, 종료 시점=지금+1일·현재 시.
- **알림-watch 연결**: `notifications.watch_id` — 크롤러가 enqueue 시 넣고, `GET /api/watches`가 watch별 최근 알림(body·created_at)을 LEFT JOIN으로 내려 홈 목록에 표기.

## 배포 토폴로지

리버스 프록시(80/443·TLS·웹훅 라우팅)는 **dogroo 리포의 `gateway/`** 공용 caddy가 담당한다. 이 리포의 compose는 backend/frontend 컨테이너만 정의하고 external 네트워크 `gateway`에 참여한다.

```
인터넷 ─ donoti.sudosoon.org (Cloudflare DNS, gateway의 cf-ddns가 갱신)
  [gateway-caddy]  ─ HTTPS 자동 인증서
    ├ /api/*       → donoti-backend:4646
    ├ /deploy/hook → host:9099 (gateway 소속 systemd webhook → deploy/deploy.sh)
    └ /*           → donoti-frontend:80 (컨테이너 내부 caddy가 SPA fallback)
  [donoti-backend] ─ /root/workspace/donoti-data ↔ /data
```

- `container_name`(donoti-backend/donoti-frontend)은 gateway Caddyfile이 참조하므로 변경 금지.
- 서버 env: `/etc/donoti/backend.env` (`INITIAL_USERNAME`, `INITIAL_PASSWORD`), chmod 600.
- 백업 = `/root/workspace/donoti-data` 디렉토리 복사.

## CI/CD

push to main → GitHub Actions가 backend/frontend 빌드 검증 → 성공 시 `https://donoti.sudosoon.org/deploy/hook` 웹훅 호출 (Secrets: `DEPLOY_URL`, `DEPLOY_KEY`) → 서버 webhook이 `deploy/deploy.sh` 실행 (`git reset --hard FETCH_HEAD` + `docker compose up -d --build --remove-orphans`). 로그: `/var/log/donoti-deploy.log`.

## 서버 셋업

dogroo 리포의 `gateway/server-setup.sh`가 donoti 클론·env·네트워크·기동까지 담당한다 (dogroo 리포의 gateway/README.md 참고).
