#!/usr/bin/env sh
# shunit2-style suite for the logging helpers.

. "$(dirname "$0")/../scripts/lib.sh"

testLogInfoWritesToStderr() {
  output=$(log_info hello 2>&1)
  assertEquals "[info] hello" "$output"
}

testLogErrorWritesToStderr() {
  output=$(log_error boom 2>&1)
  assertEquals "[error] boom" "$output"
}

oneTimeSetUp() {
  export TZ=UTC
}

. shunit2
