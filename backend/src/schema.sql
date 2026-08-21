CREATE TABLE IF NOT EXISTS users (
  id            INTEGER PRIMARY KEY,
  username      TEXT NOT NULL UNIQUE,
  password_hash TEXT NOT NULL,            -- argon2id
  created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);

-- sessions 테이블은 session-store.ts 생성자가 자체 생성 (sid/sess/expires)

-- 알림 발송 큐: 생산자(크롤러·API)는 행만 넣고, 워커가 Web Push로 브로드캐스트한다
CREATE TABLE IF NOT EXISTS notifications (
  id              INTEGER PRIMARY KEY,    -- 생성순 = FIFO
  source          TEXT NOT NULL,          -- 넣은 주체 (크롤러·잡 이름)
  title           TEXT NOT NULL,
  body            TEXT NOT NULL DEFAULT '',
  url             TEXT,                   -- 알림 클릭 시 이동 경로 (없으면 /)
  status          TEXT NOT NULL DEFAULT 'pending'
                  CHECK (status IN ('pending','sending','sent','failed')),
  attempts        INTEGER NOT NULL DEFAULT 0,
  next_attempt_at INTEGER NOT NULL DEFAULT 0,  -- epoch ms (Date.now() 비교 - datetime 텍스트와 섞지 말 것)
  last_error      TEXT,
  delivered       INTEGER,                -- 성공 시 실제 전달된 구독 수
  read_at         TEXT,                   -- 알림센터에서 읽은 시각 (NULL = 안읽음, 앱 배지 카운트 기준)
  watch_id        INTEGER,                -- 이 알림을 만든 watch (홈 목록 '최근 알림' 표기용)
  created_at      TEXT NOT NULL DEFAULT (datetime('now')),
  sent_at         TEXT
);
-- 워커 폴링 쿼리 전용 부분 인덱스
CREATE INDEX IF NOT EXISTS idx_notifications_pending
  ON notifications(next_attempt_at) WHERE status = 'pending';

-- 기기별 Web Push 구독
CREATE TABLE IF NOT EXISTS push_subscriptions (
  id          INTEGER PRIMARY KEY,
  endpoint    TEXT NOT NULL UNIQUE,
  keys_p256dh TEXT NOT NULL,
  keys_auth   TEXT NOT NULL,
  created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 감시 알림 (2뎁스: category > provider, 예: movie > cgv)
CREATE TABLE IF NOT EXISTS watches (
  id         INTEGER PRIMARY KEY,
  category   TEXT NOT NULL,               -- inventory(재고) | movie(영화) | price(시세)
  provider   TEXT NOT NULL,               -- emart | cgv | lotte | megabox | fx | stock
  name       TEXT NOT NULL,
  url        TEXT,                        -- 리다이렉트 URL (사용자는 설정하지 않음 - 크롤러 로직이 자동 설정)
  schedule   TEXT NOT NULL,               -- JSON ScheduleRule[] (KST 24시간제, watch-schedule.ts 참고)
  config     TEXT NOT NULL DEFAULT '{}',  -- provider별 등록 폼 데이터 (각 크롤러 단계에서 정의)
  enabled    INTEGER NOT NULL DEFAULT 1,  -- 0 = 임시 중지 (알림·크롤링 모두 쉼, 사용자가 다시 켤 때까지)
  state      TEXT,                        -- 크롤러 전용 상태 JSON (예: cgv 예매 열림 감지용 seen 키)
  ends_at    TEXT,                        -- 종료 시점 KST 'YYYY-MM-DD HH:MM', NULL = 상시
  deleted_at TEXT,                        -- 종료/삭제 시각 (soft delete - 복구 가능)
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 주식 종목 마스터 (유명 종목만 - crawlers/stock-symbols.ts 시드가 기동 시마다 upsert)
CREATE TABLE IF NOT EXISTS stock_symbols (
  code     TEXT PRIMARY KEY,           -- '005930' | 'AAPL.O'(나스닥) | 'JPM'(NYSE) | 'KOSPI' | '.DJI'
  name     TEXT NOT NULL,              -- '삼성전자'
  kind     TEXT NOT NULL,              -- kr(한국주식) | us(미국주식) | index(지수) | etf(ETF)
  market   TEXT NOT NULL,              -- domestic | world (네이버 API 라우팅)
  keywords TEXT NOT NULL DEFAULT ''    -- 추가 검색어 (영문명 등)
);

-- 범용 key-value 설정 (크롤러 주기 등 - 스키마 변경 없이 확장)
CREATE TABLE IF NOT EXISTS settings (
  key        TEXT PRIMARY KEY,            -- 예: crawler.<name>.interval
  value      TEXT NOT NULL,               -- JSON 또는 스칼라 문자열
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
