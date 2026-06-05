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

@test "--events-fd emits recipe_start and recipe_complete per recipe" {
  # `@foo:` makes the whole recipe quiet (recipe-level quiet=true).
  # A per-line `@echo` only suppresses that line's echo and leaves
  # `recipe.quiet=false`. We want to assert the recipe-level field.
  cat > justfile <<'EOF'
# the foo recipe
@foo:
  echo hello
EOF
  run bash -c '"$0" --events-fd 3 foo 3>events.log' "${JUST_BIN:-just}"
  assert_success
  # Plan record first.
  head -1 events.log | grep -q '"type":"plan"' || \
    fail "first line not plan: $(head -1 events.log)"
  # Recipe start carries name, namepath, depth=0, parent=null, doc, quiet.
  grep -q '"type":"recipe_start"' events.log || \
    fail "no recipe_start: $(cat events.log)"
  grep '"type":"recipe_start"' events.log | grep -q '"name":"foo"' || \
    fail "recipe_start missing name: $(cat events.log)"
  grep '"type":"recipe_start"' events.log | grep -q '"depth":0' || \
    fail "recipe_start missing depth 0: $(cat events.log)"
  grep '"type":"recipe_start"' events.log | grep -q '"parent":null' || \
    fail "recipe_start missing null parent: $(cat events.log)"
  grep '"type":"recipe_start"' events.log | grep -q '"doc":"the foo recipe"' || \
    fail "recipe_start missing doc: $(cat events.log)"
  grep '"type":"recipe_start"' events.log | grep -q '"quiet":true' || \
    fail "recipe_start missing quiet=true (recipe declared with @ prefix): $(cat events.log)"
  # Recipe complete: success → exit_code 0, signal null.
  grep -q '"type":"recipe_complete"' events.log || \
    fail "no recipe_complete: $(cat events.log)"
  grep '"type":"recipe_complete"' events.log | grep -q '"exit_code":0' || \
    fail "recipe_complete missing exit_code 0: $(cat events.log)"
  grep '"type":"recipe_complete"' events.log | grep -q '"signal":null' || \
    fail "recipe_complete missing null signal: $(cat events.log)"
  # Ordering: recipe_start before output before recipe_complete.
  start_line=$(grep -n '"type":"recipe_start"' events.log | head -1 | cut -d: -f1)
  output_line=$(grep -n '"type":"output"' events.log | head -1 | cut -d: -f1)
  complete_line=$(grep -n '"type":"recipe_complete"' events.log | head -1 | cut -d: -f1)
  (( start_line < output_line )) || \
    fail "recipe_start (line $start_line) not before output (line $output_line)"
  (( output_line < complete_line )) || \
    fail "output (line $output_line) not before recipe_complete (line $complete_line)"
}

@test "--events-fd: dep ordering — parent recipe_start before child events; child recipe_complete before parent's body" {
  cat > justfile <<'EOF'
foo: bar
  @echo foo-body
bar:
  @echo bar-body
EOF
  run bash -c '"$0" --events-fd 3 foo 3>events.log' "${JUST_BIN:-just}"
  assert_success
  # Two recipe_start and two recipe_complete records.
  starts=$(grep -c '"type":"recipe_start"' events.log)
  completes=$(grep -c '"type":"recipe_complete"' events.log)
  [[ $starts == 2 ]] || fail "expected 2 recipe_start, got $starts: $(cat events.log)"
  [[ $completes == 2 ]] || fail "expected 2 recipe_complete, got $completes: $(cat events.log)"
  # foo (depth 0, parent null) recipe_start MUST precede bar's events.
  foo_start_line=$(grep -n '"name":"foo"' events.log | grep recipe_start | head -1 | cut -d: -f1)
  bar_start_line=$(grep -n '"name":"bar"' events.log | grep recipe_start | head -1 | cut -d: -f1)
  bar_complete_line=$(grep -n '"name":"bar"' events.log | grep recipe_complete \
                       || true)
  # bar's recipe_complete might not carry the name field, but we know the
  # second recipe_complete in the stream corresponds to whichever recipe
  # finished last. We rely on the tp tag for stricter checks below.
  (( foo_start_line < bar_start_line )) || \
    fail "foo recipe_start (line $foo_start_line) should precede bar recipe_start (line $bar_start_line)"
  # bar has depth 1 and parent = foo's tp (= 1).
  grep '"name":"bar"' events.log | grep -q '"depth":1' || \
    fail "bar's recipe_start missing depth 1: $(cat events.log)"
  grep '"name":"bar"' events.log | grep -q '"parent":1' || \
    fail "bar's recipe_start missing parent=1: $(cat events.log)"
  # foo's body output ("foo-body") must come AFTER bar's recipe_complete.
  bar_complete_line=$(awk 'NR==2 && /"type":"recipe_complete"/{exit} /"type":"recipe_complete"/{print NR; exit}' events.log)
  foo_body_line=$(grep -n '"data":"foo-body' events.log | head -1 | cut -d: -f1)
  bar_complete_line=$(grep -n '"type":"recipe_complete"' events.log | head -1 | cut -d: -f1)
  (( bar_complete_line < foo_body_line )) || \
    fail "bar recipe_complete (line $bar_complete_line) should precede foo body output (line $foo_body_line)"
}
