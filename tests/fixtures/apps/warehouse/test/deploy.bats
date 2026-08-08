#!/usr/bin/env bats

setup() {
  load '../scripts/lib.sh'
  export REGISTRY="registry.test"
}

@test "require_cmd succeeds for an existing command" {
  run require_cmd bash
  [ "$status" -eq 0 ]
}

@test "require_cmd fails for a missing command" {
  run require_cmd definitely-not-a-real-binary
  [ "$status" -eq 1 ]
}

@test "retry gives up after the attempt budget" {
  run retry 2 false
  [ "$status" -eq 1 ]
}

teardown() {
  unset REGISTRY
}
