import argon2 from 'argon2';
import type { FastifyReply, FastifyRequest } from 'fastify';

declare module 'fastify' {
  interface Session {
    userId?: number;
  }
}

export async function hashPassword(plain: string): Promise<string> {
  return argon2.hash(plain, { type: argon2.argon2id });
}

export async function verifyPassword(hash: string, plain: string): Promise<boolean> {
  return argon2.verify(hash, plain);
}

// 로그인 필수 라우트용 preHandler
export async function requireAuth(req: FastifyRequest, reply: FastifyReply): Promise<void> {
  if (!req.session.userId) {
    await reply.code(401).send({ error: 'unauthorized' });
  }
}
