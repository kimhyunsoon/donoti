import Fastify, { type FastifyInstance } from 'fastify';
import cookie from '@fastify/cookie';
import session from '@fastify/session';
import { config } from './config.js';
import { db, migrate } from './db.js';
import { SqliteSessionStore } from './session-store.js';
import { hashPassword } from './auth.js';
import { startNotifier } from './notify.js';
import { startScheduler } from './scheduler.js';
import { authRoutes } from './routes/auth.js';
import { pushRoutes } from './routes/push.js';
import { notificationRoutes } from './routes/notifications.js';
import { settingsRoutes } from './routes/settings.js';
import { watchRoutes } from './routes/watches.js';
import { stockRoutes } from './routes/stocks.js';
import { emartRoutes } from './routes/emart.js';
import { nintendoRoutes } from './routes/nintendo.js';
import { playstationRoutes } from './routes/playstation.js';
import { cgvRoutes } from './routes/cgv.js';
import { lotteRoutes } from './routes/lotte.js';
import { megaboxRoutes } from './routes/megabox.js';

// users 테이블이 비어있으면 env의 초기 계정 생성
async function bootstrapInitialUser(): Promise<void> {
  const count = db.prepare('SELECT COUNT(*) AS c FROM users').get() as { c: number };
  if (count.c > 0) return;
  if (!config.initialUsername || !config.initialPassword) return;
  const hash = await hashPassword(config.initialPassword);
  db.prepare('INSERT INTO users (username, password_hash) VALUES (?, ?)').run(
    config.initialUsername,
    hash,
  );
}

async function buildServer(): Promise<FastifyInstance> {
  // trustProxy: Caddy 뒤에서 X-Forwarded-Proto를 신뢰해야 secure 세션 쿠키가 발급된다
  const app = Fastify({ logger: true, trustProxy: true });

  await app.register(cookie);
  await app.register(session, {
    secret: config.sessionSecret,
    store: new SqliteSessionStore(),
    saveUninitialized: false, // 로그인 전 요청은 세션을 만들지 않음
    cookie: {
      secure: process.env.NODE_ENV === 'production',
      httpOnly: true,
      sameSite: 'lax',
      maxAge: 1000 * 60 * 60 * 24 * 90, // 로그인 유지 90일
    },
  });

  app.get('/api/health', async (): Promise<{ ok: boolean }> => ({ ok: true }));

  await app.register(authRoutes, { prefix: '/api/auth' });
  await app.register(pushRoutes, { prefix: '/api/push' });
  await app.register(notificationRoutes, { prefix: '/api/notifications' });
  await app.register(settingsRoutes, { prefix: '/api/settings' });
  await app.register(watchRoutes, { prefix: '/api/watches' });
  await app.register(stockRoutes, { prefix: '/api/stocks' });
  await app.register(emartRoutes, { prefix: '/api/emart' });
  await app.register(nintendoRoutes, { prefix: '/api/nintendo' });
  await app.register(playstationRoutes, { prefix: '/api/playstation' });
  await app.register(cgvRoutes, { prefix: '/api/cgv' });
  await app.register(lotteRoutes, { prefix: '/api/lotte' });
  await app.register(megaboxRoutes, { prefix: '/api/megabox' });

  return app;
}

async function main(): Promise<void> {
  migrate();
  await bootstrapInitialUser();
  const app = await buildServer();
  startNotifier((msg) => app.log.info(msg));
  startScheduler((msg) => app.log.info(msg));
  await app.listen({ port: config.port, host: config.host });
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
