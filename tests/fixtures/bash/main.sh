#!/usr/bin/env bash
set -euo pipefail

source ./lib.sh
source "./config.sh"

deploy() {
  local branch
  branch=$(current_branch)
  if grep -q "ready" status.txt; then
    log_info "deploying $branch"
  else
    log_error "not ready"
    return 1
  fi

  for host in web1 web2 web3; do
    rsync -az build/ "$host:/srv/app"
    log_info "synced $host"
  done
}

report() {
  while read -r line; do
    echo "$line"
  done < deploy.log
  awk '{print $2}' deploy.log | sort | uniq -c
}

case "${1:-}" in
  deploy) deploy ;;
  report) report ;;
  *) log_error "usage: $0 deploy|report" ;;
esac

log_info "done: $(count_matches synced deploy.log) hosts"
