#!/usr/bin/env bash
# donoti 로컬 개발 실행 — backend(Rust :4646) + frontend(:4647, /api 프록시)
#   ./dev.sh     하나가 죽거나 Ctrl+C 면 전부 내린다. backend 만은 backend/dev.sh
set -uo pipefail
cd "$(dirname "$0")"
echo "🔔 donoti dev — 접속 주소: http://localhost:4647"

[[ -d frontend/node_modules ]] || (cd frontend && pnpm install)

cleanup() { trap - EXIT INT TERM; echo "[dev] 전부 내립니다." >&2; kill 0 2>/dev/null; wait 2>/dev/null; }
trap cleanup EXIT INT TERM

run() { # <이름> <명령…> : 출력 줄 앞에 [이름]
  local name="$1"; shift
  ( "$@" 2>&1 | sed -u "s/^/[$name] /" ) &
}

run backend bash backend/dev.sh
# Rust 첫 빌드는 1분을 넘을 수 있다 - backend 가 뜬 뒤 frontend 를 띄운다
echo "[dev] backend(4646) 기동 대기…"
for _ in $(seq 1 300); do
  curl -s -o /dev/null http://localhost:4646/api/health && break
  sleep 1
done
run frontend bash -c 'cd frontend && exec pnpm dev'

wait -n
echo "[dev] 프로세스 하나가 종료되었습니다." >&2
