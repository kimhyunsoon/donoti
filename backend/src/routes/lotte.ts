import type { FastifyInstance } from 'fastify';
import { requireAuth } from '../auth.js';
import { fetchLotteMovies, fetchLotteTheaters } from '../crawlers/lotte.js';

// 롯데시네마 등록 폼용 목록 프록시 (상영작·지점)
export async function lotteRoutes(app: FastifyInstance): Promise<void> {
  app.addHook('preHandler', requireAuth);

  app.get('/movies', async (req, reply) => {
    try {
      return await fetchLotteMovies();
    } catch {
      return reply.code(502).send({ error: 'lotte_error', message: '롯데시네마 상영작을 불러오지 못했어요' });
    }
  });

  app.get('/theaters', async (req, reply) => {
    try {
      return await fetchLotteTheaters();
    } catch {
      return reply.code(502).send({ error: 'lotte_error', message: '롯데시네마 지점을 불러오지 못했어요' });
    }
  });
}
