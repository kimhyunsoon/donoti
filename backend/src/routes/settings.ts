import type { FastifyInstance } from 'fastify';
import { db } from '../db.js';
import { requireAuth } from '../auth.js';

interface SettingRow {
  key: string;
  value: string;
  updated_at: string;
}

interface PutBody {
  value: string;
}

// 범용 key-value 설정 (크롤러 주기 등은 다음 단계에서 이 위에 얹는다)
export async function settingsRoutes(app: FastifyInstance): Promise<void> {
  app.addHook('preHandler', requireAuth);

  app.get('/', async (): Promise<SettingRow[]> => {
    return db.prepare('SELECT * FROM settings ORDER BY key').all() as SettingRow[];
  });

  app.put<{ Params: { key: string }; Body: PutBody }>('/:key', async (req) => {
    db.prepare(
      `INSERT INTO settings (key, value, updated_at) VALUES (?, ?, datetime('now'))
       ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = datetime('now')`,
    ).run(req.params.key, req.body.value);
    return { ok: true };
  });
}
