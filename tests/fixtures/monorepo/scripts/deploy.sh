#!/usr/bin/env bash
source ./lib.sh

deploy() {
    log_info "deploying"
    rsync -a build/ remote:/srv/app
    archive_logs
}

deploy
