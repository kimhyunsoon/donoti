import type { FastifyInstance } from 'fastify';
import { db } from '../db.js';
import { verifyPassword, requireAuth } from '../auth.js';

interface LoginBody {
  username: string;
  password: string;
}

interface UserRow {
  id: number;
  username: string;
  password_hash: string;
}

export async function authRoutes(app: FastifyInstance): Promise<void> {
  app.post<{ Body: LoginBody }>('/login', async (req, reply) => {
    const { username, password } = req.body;
    const user = db
      .prepare('SELECT id, username, password_hash FROM users WHERE username = ?')
      .get(username) as UserRow | undefined;
    if (!user || !(await verifyPassword(user.password_hash, password))) {
      return reply.code(401).send({ error: 'invalid_credentials' });
    }
    req.session.userId = user.id;
    return { id: user.id, username: user.username };
  });

  app.post('/logout', { preHandler: requireAuth }, async (req) => {
    await req.session.destroy();
    return { ok: true };
  });

  app.get('/me', { preHandler: requireAuth }, async (req) => {
    const user = db
      .prepare('SELECT id, username FROM users WHERE id = ?')
      .get(req.session.userId) as Pick<UserRow, 'id' | 'username'>;
    return user;
  });
}
