#!/usr/bin/env bash
# 사용법: deploy.sh [targets]   (인자는 웹훅 페이로드 호환용 - 빌드 대상은 아래에서 변경 파일로 판단)
# dogroo 리포 gateway/hooks.json의 webhook이 호출. 직접 실행해도 동작한다.
#   FORCE=1 deploy.sh   변경 여부와 무관하게 backend·frontend 전부 다시 빌드
#
# 서비스(backend/frontend)마다 마지막으로 배포에 성공한 커밋을 기록해 두고,
# 그 뒤로 해당 경로가 바뀐 서비스만 빌드한다. 실패하면 기록이 남지 않아 다음 배포에서 다시 빌드된다.
set -euo pipefail

APP=donoti
REPO_DIR="${REPO_DIR:-/root/workspace/$APP}"
LOG_FILE="${DEPLOY_LOG:-/var/log/$APP-deploy.log}"
STATE_DIR="${DEPLOY_STATE_DIR:-/var/lib/do-deploy/$APP}"
# 서비스 → 이미지에 영향을 주는 경로 (리포 루트 기준)
declare -A PATHS=(
  [backend]="backend deploy/docker-compose.yml"
  [frontend]="frontend deploy/docker-compose.yml"
)

# 실행 중 git reset으로 스크립트 자신이 갱신되어도 안전하도록 전체를 함수로 감싼다
main() {
  # 4개 앱(dogroo·donoti·docook·doquiz) 공용 잠금 - 서버 빌드가 동시에 돌지 않게 앞선 배포를 기다린다
  exec 9>/tmp/do-series-deploy.lock
  flock 9

  echo "[$(date '+%F %T')] 배포 시작: ${1:-}"
  cd "$REPO_DIR"
  # git 1.8 호환 (FETCH_HEAD 기준으로 최신화)
  git fetch origin main
  git reset --hard FETCH_HEAD
  local head; head=$(git rev-parse HEAD)
  mkdir -p "$STATE_DIR"

  local targets=() svc last
  for svc in backend frontend; do
    last=$(cat "$STATE_DIR/$svc" 2>/dev/null || true)
    if [[ "${FORCE:-0}" == 1 || -z "$last" ]] \
      || ! git cat-file -e "$last^{commit}" 2>/dev/null \
      || [[ -n "$(git diff --name-only "$last" "$head" -- ${PATHS[$svc]})" ]] \
      || [[ -z "$(cd deploy && docker compose ps -q "$svc")" ]]; then
      targets+=("$svc")
    fi
  done

  cd deploy
  if (( ${#targets[@]} )); then
    echo "빌드 대상: ${targets[*]}"
    docker compose up -d --build --remove-orphans "${targets[@]}"
    for svc in "${targets[@]}"; do echo "$head" > "$STATE_DIR/$svc"; done
  else
    echo "변경 없음 - 빌드 생략"
  fi
  # 빌드하지 않은 서비스도 떠 있게 (--remove-orphans: compose에서 제거된 서비스의 잔류 컨테이너 정리)
  docker compose up -d --remove-orphans

  # 사용하지 않는 이전 이미지 정리 (태그 있는 롤백 이미지는 남는다)
  docker image prune -f
  echo "[$(date '+%F %T')] 배포 완료"
}

main "$@" >> "$LOG_FILE" 2>&1
