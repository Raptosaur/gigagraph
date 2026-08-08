#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "$0")/lib.sh"

APP_NAME="warehoused"
REGISTRY="registry.example.com"

build_image() {
  local tag="$1"
  log_info "building ${APP_NAME}:${tag}"
  docker build -t "${REGISTRY}/${APP_NAME}:${tag}" .
}

push_image() {
  local tag="$1"
  require_cmd docker
  docker push "${REGISTRY}/${APP_NAME}:${tag}"
}

rollout() {
  local tag="$1"
  build_image "$tag"
  push_image "$tag"
  kubectl set image "deploy/${APP_NAME}" "${APP_NAME}=${REGISTRY}/${APP_NAME}:${tag}"
}

main() {
  rollout "${1:-latest}"
  log_info "done"
}

main "$@"
