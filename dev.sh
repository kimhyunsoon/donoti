#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
echo "🔔 donoti dev — 접속 주소: http://localhost:4647"

if [[ ! -f backend/.env ]]; then
  cp backend/.env.example backend/.env
  echo "backend/.env를 생성했습니다 — 필요 시 값을 수정하세요"
fi

[[ -d backend/node_modules ]] || (cd backend && pnpm install)
[[ -d frontend/node_modules ]] || (cd frontend && pnpm install)

trap 'kill 0' EXIT INT TERM   # 종료 시 두 프로세스 모두 정리

# .env는 자동 로드되지 않으므로 여기서 주입 (배포에서는 compose env_file이 담당)
(cd backend && set -a && source .env && set +a && pnpm dev) &
(cd frontend && pnpm dev) &
wait
