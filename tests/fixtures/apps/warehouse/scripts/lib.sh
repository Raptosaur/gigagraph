#!/usr/bin/env bash

log_info() {
  printf '[info] %s\n' "$*" >&2
}

log_error() {
  printf '[error] %s\n' "$*" >&2
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || {
    log_error "missing required command: $1"
    return 1
  }
}

retry() {
  local attempts="$1"
  shift
  local n=0
  until "$@"; do
    n=$((n + 1))
    [ "$n" -ge "$attempts" ] && return 1
    sleep 1
  done
}
