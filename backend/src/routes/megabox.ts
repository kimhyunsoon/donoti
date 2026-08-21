import type { FastifyInstance } from 'fastify';
import { requireAuth } from '../auth.js';
import { fetchMegaboxMovies, fetchMegaboxTheaters } from '../crawlers/megabox.js';

// 메가박스 등록 폼용 목록 프록시 (상영작·지점)
export async function megaboxRoutes(app: FastifyInstance): Promise<void> {
  app.addHook('preHandler', requireAuth);

  app.get('/movies', async (req, reply) => {
    try {
      return await fetchMegaboxMovies();
    } catch {
      return reply.code(502).send({ error: 'megabox_error', message: '메가박스 상영작을 불러오지 못했어요' });
    }
  });

  app.get('/theaters', async (req, reply) => {
    try {
      return await fetchMegaboxTheaters();
    } catch {
      return reply.code(502).send({ error: 'megabox_error', message: '메가박스 지점을 불러오지 못했어요' });
    }
  });
}
