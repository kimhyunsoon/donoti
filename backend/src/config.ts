import { randomBytes } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import webpush from 'web-push';

// 개인용 단일 서버 전제 - 바뀔 일 없는 값은 상수로 고정
// 포트는 로컬에서 다른 프로그램(dogroo 4746/4747)과 안 겹치게 donoti의 D(4)·O(6)를 딴 4646/4647 사용
const PORT = 4646;
const HOST = '0.0.0.0';
const TZ = 'Asia/Seoul';
const VAPID_SUBJECT = 'mailto:sudosoon@gmail.com';

// 데이터 디렉토리 (SQLite) - 도커에서만 /data로 주입
const dataDir = process.env.DATA_DIR ?? './data';
mkdirSync(dataDir, { recursive: true });

// 세션 시크릿은 최초 기동 시 자동 생성해 데이터 디렉토리에 보관 (env 불필요)
function loadSessionSecret(): string {
  const file = join(dataDir, '.session-secret');
  if (existsSync(file)) return readFileSync(file, 'utf-8').trim();
  const secret = randomBytes(48).toString('hex');
  writeFileSync(file, secret, { mode: 0o600 });
  return secret;
}

// VAPID 키쌍(ECDSA P-256)도 최초 기동 시 자동 생성해 보관.
// 재생성되면 기존 푸시 구독이 전부 무효가 되므로 구독 DB와 같은 디렉토리에 함께 유지한다.
function loadVapidKeys(): { publicKey: string; privateKey: string } {
  const file = join(dataDir, '.vapid.json');
  if (existsSync(file)) {
    return JSON.parse(readFileSync(file, 'utf-8')) as { publicKey: string; privateKey: string };
  }
  const keys = webpush.generateVAPIDKeys();
  writeFileSync(file, JSON.stringify(keys), { mode: 0o600 });
  return keys;
}

export const config = {
  port: PORT,
  host: HOST,
  tz: TZ,
  dataDir,
  sessionSecret: loadSessionSecret(),
  initialUsername: process.env.INITIAL_USERNAME,
  initialPassword: process.env.INITIAL_PASSWORD,
  vapid: { ...loadVapidKeys(), subject: VAPID_SUBJECT },
} as const;
