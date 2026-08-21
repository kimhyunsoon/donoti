import type { FastifyInstance } from 'fastify';
import { db } from '../db.js';
import { requireAuth } from '../auth.js';
import type { StockSymbol } from '../crawlers/stock-symbols.js';

// 주식 종목 마스터 검색 (알림 등록 폼에서 코드·이름으로 검색)
export async function stockRoutes(app: FastifyInstance): Promise<void> {
  app.addHook('preHandler', requireAuth);

  app.get<{ Querystring: { q?: string } }>('/', async (req): Promise<StockSymbol[]> => {
    const q = (req.query.q ?? '').trim();
    if (q === '') return [];
    const like = `%${q}%`;
    // 이름 접두 일치 → 코드 접두 일치 → 나머지 순으로 정렬
    return db
      .prepare(
        `SELECT code, name, kind, market, keywords FROM stock_symbols
         WHERE code LIKE ? OR name LIKE ? OR keywords LIKE ?
         ORDER BY CASE WHEN name LIKE ? THEN 0 WHEN code LIKE ? THEN 1 ELSE 2 END, name
         LIMIT 20`,
      )
      .all(like, like, like, `${q}%`, `${q}%`) as StockSymbol[];
  });
}
