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
| `settings` | 범용 key-value. 크롤러 주기 등은 `crawler.<name>.*` 키로 확장 예정 |

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
