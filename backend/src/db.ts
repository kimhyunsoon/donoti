import Database from 'better-sqlite3';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { config } from './config.js';

export const db: Database.Database = new Database(join(config.dataDir, 'donoti.db'));

db.pragma('journal_mode = WAL');
db.pragma('foreign_keys = ON');

// 스키마 적용 (IF NOT EXISTS 기반이라 기동 시마다 실행해도 안전)
export function migrate(): void {
  const schema = readFileSync(new URL('./schema.sql', import.meta.url), 'utf-8');
  db.exec(schema);
  // 기존 DB에 없는 컬럼 추가
  const notiCols = db.prepare('PRAGMA table_info(notifications)').all() as { name: string }[];
  if (!notiCols.some((c) => c.name === 'read_at')) {
    db.exec('ALTER TABLE notifications ADD COLUMN read_at TEXT');
  }
}
