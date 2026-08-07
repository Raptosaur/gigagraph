log_info() {
    echo "[info] $1"
}

archive_logs() {
    tar -czf logs.tgz ./*.log
    log_info "archived"
}
