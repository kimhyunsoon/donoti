#!/usr/bin/env bash
# 사용법: deploy.sh <targets>   (인자는 웹훅 페이로드 호환용 - 항상 전체 재적용)
# dogroo 리포 gateway/hooks.json의 webhook이 호출. 직접 실행해도 동작한다.
set -euo pipefail

REPO_DIR="${REPO_DIR:-/root/workspace/donoti}"
LOG_FILE="${DEPLOY_LOG:-/var/log/donoti-deploy.log}"

# 동시 배포 직렬화 - 앞선 배포가 끝날 때까지 대기
exec 9>/tmp/donoti-deploy.lock
flock 9

{
  echo "[$(date '+%F %T')] 배포 시작: ${1:-}"

  cd "$REPO_DIR"
  # git 1.8 호환 (FETCH_HEAD 기준으로 최신화)
  git fetch origin main
  git reset --hard FETCH_HEAD

  cd deploy
  # 항상 전체 재적용 - 변경 없는 이미지는 Docker 캐시로 수 초에 끝난다
  docker compose up -d --build --remove-orphans

  # 사용하지 않는 이전 이미지 정리
  docker image prune -f
  echo "[$(date '+%F %T')] 배포 완료"
} >> "$LOG_FILE" 2>&1
