#!/usr/bin/env bash
# Shared helpers for the deploy scripts.

. ./colors.sh

log_info() {
  echo "[info] $*"
}

function log_error {
  echo "[error] $*" >&2
}

function count_matches() {
  grep -c "$1" "$2"
}

current_branch() {
  git rev-parse --abbrev-ref HEAD
}
