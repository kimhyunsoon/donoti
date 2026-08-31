import type { FastifyInstance } from 'fastify';
import { requireAuth } from '../auth.js';
import { resolveNintendoLink } from '../crawlers/nintendo.js';

// 닌텐도 스토어 상품 링크 해석 (알림 등록 폼에서 상품·가격 확인용)
export async function nintendoRoutes(app: FastifyInstance): Promise<void> {
  app.addHook('preHandler', requireAuth);

  app.post<{ Body: { url: string } }>('/resolve', async (req, reply) => {
    try {
      return await resolveNintendoLink(req.body.url ?? '');
    } catch (err) {
      const message = err instanceof Error ? err.message : '링크를 해석하지 못했어요';
      return reply.code(400).send({ error: 'invalid_link', message });
    }
  });
}
