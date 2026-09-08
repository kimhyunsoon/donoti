import type { FastifyInstance } from 'fastify';
import { requireAuth } from '../auth.js';
import { resolveDcinsideLink } from '../crawlers/dcinside.js';

// 디시인사이드 갤러리 링크 해석 (알림 등록 폼에서 갤러리 확인용)
export async function dcinsideRoutes(app: FastifyInstance): Promise<void> {
  app.addHook('preHandler', requireAuth);

  app.post<{ Body: { url: string } }>('/resolve', async (req, reply) => {
    try {
      return await resolveDcinsideLink(req.body.url ?? '');
    } catch (err) {
      const message = err instanceof Error ? err.message : '링크를 해석하지 못했어요';
      return reply.code(400).send({ error: 'invalid_link', message });
    }
  });
}
