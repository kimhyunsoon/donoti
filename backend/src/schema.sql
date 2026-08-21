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

-- 범용 key-value 설정 (크롤러 주기 등 - 스키마 변경 없이 확장)
CREATE TABLE IF NOT EXISTS settings (
  key        TEXT PRIMARY KEY,            -- 예: crawler.<name>.interval
  value      TEXT NOT NULL,               -- JSON 또는 스칼라 문자열
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
