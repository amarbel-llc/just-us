# bats file_tags=events_fd
#
# Activation contract for --events-fd, per RFC 0002 §Activation.
# Tests 2-4 are RED until config.rs/run.rs wiring lands; that is
# intentional — the test suite is the red→green driver for the rest
# of the implementation.

setup() {
  load "$(dirname "$BATS_TEST_FILE")/common.bash"
  setup_test_home
}

@test "no --events-fd: existing behavior unchanged (stdout passthrough)" {
  # `@` quiet prefix suppresses the command-echo banner so the test
  # asserts on just the child's stdout, not the recipe-runner chrome.
  cat > justfile <<'EOF'
foo:
  @echo hello
EOF
  run_just foo
  assert_success
  assert_output 'hello'
}

@test "--events-fd 3 writes a plan record with version 1 as the first event" {
  cat > justfile <<'EOF'
foo:
  @echo hello
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
    fail "first line was not a plan record: $first_line"
  [[ $first_line == *'"version":1'* ]] || \
    fail "plan record missing version 1: $first_line"
  [[ $first_line == *'"recipe_count":1'* ]] || \
    fail "plan record missing recipe_count 1: $first_line"
}

@test "--events-fd N where N is an unopened fd fails before any recipe runs" {
  # The recipe writes a marker file so we can tell whether it ran.
  # If fd-validation happens after recipe execution starts, this
  # marker will exist; the RFC requires it to NOT exist.
  cat > justfile <<'EOF'
foo:
  @touch ran-marker
EOF
  # fd 99 is not opened by the calling shell.
  run "${JUST_BIN:-just}" --events-fd 99 foo
  assert_failure
  [[ ! -e ran-marker ]] || \
    fail "fd-validation happened after recipe execution started"
  # The error MUST be about the fd being invalid, NOT clap complaining
  # that --events-fd is an unknown argument. Stays RED until the flag
  # is wired up in config.rs and a real fd-validation path emits a
  # specific diagnostic.
  refute_output --partial 'unexpected argument'
  refute_output --partial 'For more information, try'
}

@test "--events-fd active: child stdout does not pass through to just stdout" {
  cat > justfile <<'EOF'
foo:
  @echo hello
EOF
  # When --events-fd is set, child output goes into the event stream
  # only (RFC 0002 §Suppressing Inherited stdout/stderr).
  run bash -c '"$0" --events-fd 3 foo 3>events.log' "${JUST_BIN:-just}"
  assert_success
  # Just's own stdout MUST be empty of child output.
  [[ $output != *'hello'* ]] || \
    fail "child output leaked to just's stdout: $output"
  # The event stream MUST contain the child's bytes as an output record.
  grep -q '"type":"output"' events.log || \
    fail "no output record in event stream: $(cat events.log)"
  grep -q '"data":"hello' events.log || \
    fail "child bytes not captured into event stream: $(cat events.log)"
}
