#!/usr/bin/env bash
# backend 로컬 실행 (Rust, :4646). 루트 dev.sh 가 frontend 와 함께 부른다.
#   bash backend/dev.sh
# 필요: Rust 툴체인(rust-toolchain.toml 버전을 rustup 이 맞춘다), backend/.env (.env.example 참고)
# 소스를 고치면 다시 실행한다 (cargo-watch 가 있으면 자동 재시작).
set -euo pipefail
cd "$(dirname "$0")"
[[ -f .env ]] || { cp .env.example .env; echo "[backend] backend/.env 를 생성했습니다 - 필요 시 값을 수정하세요"; }
set -a; source .env; set +a
if command -v cargo-watch >/dev/null; then
  exec cargo watch -q -w src -x 'run --bin donoti-backend'
fi
exec cargo run --bin donoti-backend
