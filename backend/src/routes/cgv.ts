import type { FastifyInstance } from 'fastify';
import { requireAuth } from '../auth.js';
import { fetchCgvMovies, fetchCgvTheaters } from '../crawlers/cgv.js';

// CGV 등록 폼용 목록 프록시 (상영작·지점)
export async function cgvRoutes(app: FastifyInstance): Promise<void> {
  app.addHook('preHandler', requireAuth);

  app.get('/movies', async (req, reply) => {
    try {
      return await fetchCgvMovies();
    } catch {
      return reply.code(502).send({ error: 'cgv_error', message: 'CGV 상영작을 불러오지 못했어요' });
    }
  });

  app.get('/theaters', async (req, reply) => {
    try {
      return await fetchCgvTheaters();
    } catch {
      return reply.code(502).send({ error: 'cgv_error', message: 'CGV 지점을 불러오지 못했어요' });
    }
  });
}
