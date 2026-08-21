import type { FastifyInstance } from 'fastify';
import { db } from '../db.js';
import { config } from '../config.js';
import { requireAuth } from '../auth.js';

interface SubscriptionBody {
  endpoint: string;
  keys: { p256dh: string; auth: string };
}

export async function pushRoutes(app: FastifyInstance): Promise<void> {
  app.addHook('preHandler', requireAuth);

  app.get('/vapid-public-key', async (): Promise<{ key: string }> => {
    return { key: config.vapid.publicKey };
  });

  app.post<{ Body: SubscriptionBody }>('/subscriptions', async (req, reply) => {
    const { endpoint, keys } = req.body;
    db.prepare(
      `INSERT INTO push_subscriptions (endpoint, keys_p256dh, keys_auth) VALUES (?, ?, ?)
       ON CONFLICT(endpoint) DO UPDATE SET keys_p256dh = excluded.keys_p256dh, keys_auth = excluded.keys_auth`,
    ).run(endpoint, keys.p256dh, keys.auth);
    return reply.code(201).send({ ok: true });
  });
}
