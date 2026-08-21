#!/usr/bin/env bash
# 크론/systemd 에서 앱을 실행하기 위한 래퍼
# 사용법: bin/run.sh <앱이름> [인자...]  (예: bin/run.sh hello "메시지")
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP="$1"
shift

cd "$ROOT"
exec ./node_modules/.bin/tsx "apps/$APP/index.ts" "$@"
