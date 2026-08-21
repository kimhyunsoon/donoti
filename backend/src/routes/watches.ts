import type { FastifyInstance, FastifyReply } from 'fastify';
import { db } from '../db.js';
import { requireAuth } from '../auth.js';
import { isValidTarget } from '../catalog.js';
import { validateSchedule, type ScheduleRule } from '../watch-schedule.js';
import { getCrawler, type WatchRow } from '../crawlers/index.js';

// 종료 시점: KST 'YYYY-MM-DD HH:MM' (프론트는 시 단위까지만 받아 분은 00)
const ENDS_RE = /^\d{4}-\d{2}-\d{2} ([01]\d|2[0-3]):[0-5]\d$/;

interface CreateBody {
  category: string;
  provider: string;
  name: string;
  schedule: ScheduleRule[];
  config?: Record<string, unknown>;
  endsAt?: string | null;
}

interface UpdateBody {
  name?: string;
  schedule?: ScheduleRule[];
  config?: Record<string, unknown>;
  endsAt?: string | null;
}

// 400 응답 헬퍼
function bad(reply: FastifyReply, message: string): FastifyReply {
  return reply.code(400).send({ error: 'invalid_input', message });
}

export async function watchRoutes(app: FastifyInstance): Promise<void> {
  app.addHook('preHandler', requireAuth);

  // 활성 알림 목록 (메인페이지) - watch별 최근 알림 내용·시각 포함
  app.get('/', async (): Promise<WatchRow[]> => {
    return db
      .prepare(
        `SELECT w.*, ln.body AS last_body, ln.created_at AS last_at
         FROM watches w
         LEFT JOIN notifications ln ON ln.id = (
           SELECT id FROM notifications WHERE watch_id = w.id ORDER BY id DESC LIMIT 1
         )
         WHERE w.deleted_at IS NULL ORDER BY w.id DESC`,
      )
      .all() as WatchRow[];
  });

  // 종료·삭제된 알림 - 등록(생성) 기준 최근 100개
  app.get('/deleted', async (): Promise<WatchRow[]> => {
    return db
      .prepare('SELECT * FROM watches WHERE deleted_at IS NOT NULL ORDER BY id DESC LIMIT 100')
      .all() as WatchRow[];
  });

  app.get<{ Params: { id: string } }>('/:id', async (req, reply) => {
    const row = db.prepare('SELECT * FROM watches WHERE id = ?').get(Number(req.params.id));
    if (!row) return reply.code(404).send({ error: 'not_found' });
    return row;
  });

  app.post<{ Body: CreateBody }>('/', async (req, reply) => {
    const { category, provider, name, schedule, config, endsAt } = req.body;
    if (!isValidTarget(category, provider)) return bad(reply, '알 수 없는 알림 종류입니다');
    if (typeof name !== 'string' || name.trim() === '') return bad(reply, '이름을 입력해 주세요');
    const scheduleError = validateSchedule(schedule);
    if (scheduleError) return bad(reply, scheduleError);
    if (endsAt != null && !ENDS_RE.test(endsAt)) return bad(reply, '종료 시점이 올바르지 않습니다');
    const configError = getCrawler(provider)?.validateConfig?.(config ?? {}, null);
    if (configError) return bad(reply, configError);

    const result = db
      .prepare(
        `INSERT INTO watches (category, provider, name, schedule, config, ends_at)
         VALUES (?, ?, ?, ?, ?, ?)`,
      )
      .run(
        category,
        provider,
        name.trim(),
        JSON.stringify(schedule),
        JSON.stringify(config ?? {}),
        endsAt ?? null,
      );
    return reply.code(201).send({ id: Number(result.lastInsertRowid) });
  });

  app.put<{ Params: { id: string }; Body: UpdateBody }>('/:id', async (req, reply) => {
    const id = Number(req.params.id);
    const row = db
      .prepare('SELECT * FROM watches WHERE id = ? AND deleted_at IS NULL')
      .get(id) as WatchRow | undefined;
    if (!row) return reply.code(404).send({ error: 'not_found' });

    const { name, schedule, config, endsAt } = req.body;
    if (name !== undefined && (typeof name !== 'string' || name.trim() === '')) {
      return bad(reply, '이름을 입력해 주세요');
    }
    if (schedule !== undefined) {
      const scheduleError = validateSchedule(schedule);
      if (scheduleError) return bad(reply, scheduleError);
    }
    if (endsAt != null && !ENDS_RE.test(endsAt)) return bad(reply, '종료 시점이 올바르지 않습니다');
    if (config !== undefined) {
      const configError = getCrawler(row.provider)?.validateConfig?.(config, id);
      if (configError) return bad(reply, configError);
    }

    db.prepare(
      `UPDATE watches
       SET name = ?, schedule = ?, config = ?, ends_at = ?, updated_at = datetime('now')
       WHERE id = ?`,
    ).run(
      name?.trim() ?? row.name,
      schedule !== undefined ? JSON.stringify(schedule) : row.schedule,
      config !== undefined ? JSON.stringify(config) : row.config,
      // endsAt: undefined = 유지, null = 상시로 해제
      endsAt === undefined ? row.ends_at : endsAt,
      id,
    );
    return { ok: true };
  });

  // 임시 중지·재개 - 중지 동안은 크롤링과 알림 모두 쉰다
  app.post<{ Params: { id: string }; Body: { enabled: boolean } }>('/:id/toggle', async (req, reply) => {
    const result = db
      .prepare(
        `UPDATE watches SET enabled = ?, updated_at = datetime('now')
         WHERE id = ? AND deleted_at IS NULL`,
      )
      .run(req.body.enabled ? 1 : 0, Number(req.params.id));
    if (result.changes === 0) return reply.code(404).send({ error: 'not_found' });
    return { ok: true };
  });

  // 목록에서 제거 (soft delete - 종료된 알림 페이지에서 복구 가능)
  app.delete<{ Params: { id: string } }>('/:id', async (req, reply) => {
    const result = db
      .prepare(
        `UPDATE watches SET deleted_at = datetime('now'), updated_at = datetime('now')
         WHERE id = ? AND deleted_at IS NULL`,
      )
      .run(Number(req.params.id));
    if (result.changes === 0) return reply.code(404).send({ error: 'not_found' });
    return { ok: true };
  });

  // 복구 - 종료 시점은 해제되어 상시로 되살아난다
  app.post<{ Params: { id: string } }>('/:id/restore', async (req, reply) => {
    const result = db
      .prepare(
        `UPDATE watches SET deleted_at = NULL, ends_at = NULL, updated_at = datetime('now')
         WHERE id = ? AND deleted_at IS NOT NULL`,
      )
      .run(Number(req.params.id));
    if (result.changes === 0) return reply.code(404).send({ error: 'not_found' });
    return { ok: true };
  });
}
