import type { SessionStore } from '@fastify/session';
import type { Session } from 'fastify';
import { db } from './db.js';

const FALLBACK_TTL_MS = 1000 * 60 * 60 * 24 * 90;

// SQLite 기반 세션 스토어 - 서버 재시작·재배포에도 로그인이 유지된다
export class SqliteSessionStore implements SessionStore {
  constructor() {
    db.exec(
      `CREATE TABLE IF NOT EXISTS sessions (
        sid     TEXT PRIMARY KEY,
        sess    TEXT NOT NULL,
        expires INTEGER NOT NULL
      )`,
    );
    // 기동 시 만료 세션 정리
    db.prepare('DELETE FROM sessions WHERE expires < ?').run(Date.now());
  }

  set(sessionId: string, session: Session, callback: (err?: Error | null) => void): void {
    const expires = session.cookie.expires
      ? new Date(session.cookie.expires).getTime()
      : Date.now() + FALLBACK_TTL_MS;
    db.prepare('INSERT OR REPLACE INTO sessions (sid, sess, expires) VALUES (?, ?, ?)').run(
      sessionId,
      JSON.stringify(session),
      expires,
    );
    callback();
  }

  get(sessionId: string, callback: (err: Error | null, session: Session | null) => void): void {
    const row = db.prepare('SELECT sess, expires FROM sessions WHERE sid = ?').get(sessionId) as
      | { sess: string; expires: number }
      | undefined;
    if (!row || row.expires < Date.now()) {
      callback(null, null);
      return;
    }
    const parsed = JSON.parse(row.sess) as Session;
    // JSON 직렬화로 문자열이 된 만료일을 Date로 복원
    if (parsed.cookie?.expires) parsed.cookie.expires = new Date(parsed.cookie.expires);
    callback(null, parsed);
  }

  destroy(sessionId: string, callback: (err?: Error | null) => void): void {
    db.prepare('DELETE FROM sessions WHERE sid = ?').run(sessionId);
    callback();
  }
}
