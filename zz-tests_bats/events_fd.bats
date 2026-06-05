# bats file_tags=events_fd
#
# Activation contract for --events-fd, per RFC 0002 §Activation.
# These tests are RED until config.rs/run.rs wiring lands; that is
# intentional — the test suite is the red→green driver for the rest
# of the implementation.

setup() {
  load "$(dirname "$BATS_TEST_FILE")/common.bash"
  setup_test_home
}

@test "no --events-fd: existing behavior unchanged (stdout passthrough)" {
  cat > justfile <<'EOF'
foo:
  echo hello
EOF
  run_just foo
  assert_success
  assert_output 'hello'
}

@test "--events-fd 3 writes NDJSON to fd 3" {
  cat > justfile <<'EOF'
foo:
  echo hello
EOF
  # Redirect fd 3 to events.log; run just; close fd 3.
  # bats `run` invokes the command in the calling shell so the 3>
  # redirection works as expected.
  run bash -c '"$0" --events-fd 3 foo 3>events.log' "${JUST_BIN:-just}"
  assert_success
  assert [ -s events.log ]
  # First record MUST be the plan record per RFC 0002 §Document Format.
  first_line=$(head -1 events.log)
  [[ $first_line == *'"type":"plan"'* ]] || \
    fail "first line was: $first_line"
  [[ $first_line == *'"version":1'* ]] || \
    fail "plan record missing version 1: $first_line"
}

@test "--events-fd to an unopened fd exits non-zero with diagnostic on stderr" {
  cat > justfile <<'EOF'
foo:
  echo hello
EOF
  # fd 99 is not opened by the calling shell.
  run "${JUST_BIN:-just}" --events-fd 99 foo
  assert_failure
  # Diagnostic SHOULD mention the events fd / the descriptor / similar.
  # Loose match so the exact wording can iterate.
  assert_output --partial 'events-fd' || assert_output --partial 'descriptor'
}

@test "--events-fd active: child stdout/stderr does not pass through to just stdout" {
  cat > justfile <<'EOF'
foo:
  echo hello
EOF
  # When --events-fd is set, child output goes into the event stream,
  # not just's stdout (RFC 0002 §Suppressing Inherited stdout/stderr).
  run bash -c '"$0" --events-fd 3 foo 3>events.log' "${JUST_BIN:-just}"
  assert_success
  # Just's own stdout should be empty (or contain only diagnostics
  # that the upstream code path would have emitted on stderr anyway).
  # The child's `echo hello` output went into an `output` event, not stdout.
  [[ $output != *'hello'* ]] || \
    fail "child output leaked to just's stdout: $output"
}
