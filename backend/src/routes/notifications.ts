import type { FastifyInstance } from 'fastify';
import { db } from '../db.js';
import { requireAuth } from '../auth.js';
import { enqueueNotification } from '../notify.js';

interface ListQuery {
  status?: string;
  limit?: number;
}

interface EnqueueBody {
  title: string;
  body?: string;
  url?: string;
}

export async function notificationRoutes(app: FastifyInstance): Promise<void> {
  app.addHook('preHandler', requireAuth);

  // 알림센터는 항상 최근 100개까지만 보여준다
  app.get<{ Querystring: ListQuery }>('/', async (req) => {
    const limit = Math.min(req.query.limit ?? 100, 100);
    if (req.query.status) {
      return db
        .prepare('SELECT * FROM notifications WHERE status = ? ORDER BY id DESC LIMIT ?')
        .all(req.query.status, limit);
    }
    return db.prepare('SELECT * FROM notifications ORDER BY id DESC LIMIT ?').all(limit);
  });

  // 수동 등록 (파이프라인 테스트용 - 워커가 몇 초 내 이 기기로 푸시를 보낸다)
  app.post<{ Body: EnqueueBody }>('/', async (req, reply) => {
    const id = enqueueNotification({
      source: 'manual',
      title: req.body.title,
      body: req.body.body,
      url: req.body.url,
    });
    return reply.code(201).send({ id });
  });

  // 안읽은 알림 수 (알림센터 배지용)
  app.get('/unread-count', async (): Promise<{ count: number }> => {
    const row = db
      .prepare(`SELECT COUNT(*) AS c FROM notifications WHERE status = 'sent' AND read_at IS NULL`)
      .get() as { c: number };
    return { count: row.c };
  });

  // 전부 읽음 처리 (알림센터 '모두 읽음' 버튼)
  app.post('/read-all', async () => {
    db.prepare(
      `UPDATE notifications SET read_at = datetime('now') WHERE status = 'sent' AND read_at IS NULL`,
    ).run();
    return { ok: true };
  });

  // 개별 읽음 처리 (알림센터 스와이프·링크 이동·OS 알림 클릭) - 남은 안읽음 수를 함께 반환해 앱 배지 갱신에 쓴다
  app.post<{ Params: { id: string } }>('/:id/read', async (req) => {
    db.prepare(`UPDATE notifications SET read_at = datetime('now') WHERE id = ? AND read_at IS NULL`).run(
      Number(req.params.id),
    );
    const row = db
      .prepare(`SELECT COUNT(*) AS c FROM notifications WHERE status = 'sent' AND read_at IS NULL`)
      .get() as { c: number };
    return { ok: true, unread: row.c };
  });

  // 최종 실패한 알림을 재시도 큐로 되돌린다
  app.post<{ Params: { id: string } }>('/:id/retry', async (req, reply) => {
    const result = db
      .prepare(
        `UPDATE notifications
         SET status = 'pending', attempts = 0, next_attempt_at = ?, last_error = NULL
         WHERE id = ? AND status = 'failed'`,
      )
      .run(Date.now(), Number(req.params.id));
    if (result.changes === 0) return reply.code(404).send({ error: 'not_found' });
    return { ok: true };
  });
}
