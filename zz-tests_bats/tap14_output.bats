#! /usr/bin/env bats

setup() {
  load "$(dirname "$BATS_TEST_FILE")/common.bash"

  TEST_DIR="$BATS_TEST_TMPDIR/project"
  mkdir -p "$TEST_DIR"
}

write_justfile() {
  cat > "$TEST_DIR/justfile"
}

run_tap() {
  run "$JUST_ME_BIN" --color never --output-format tap --justfile "$TEST_DIR/justfile" "$@"
}

run_tap_streamed() {
  run "$JUST_ME_BIN" --color never --output-format tap+streamed_output --justfile "$TEST_DIR/justfile" "$@"
}

run_tap_stderr() {
  run "$JUST_ME_BIN" --color never --output-format tap+stderr --justfile "$TEST_DIR/justfile" "$@"
}

validate_tap() {
  tap-dancer validate <<< "$output"
}

# --- Structural validity: buffered ---

function buffered_single_pass { # @test
  write_justfile <<'JUSTFILE'
build:
  echo building
JUSTFILE

  run_tap build
  assert_success
  assert_line --index 0 "TAP version 14"
  assert_line --partial "1..1"
  assert_line --partial "ok 1 - build"
  validate_tap
}

function buffered_multiple_all_pass { # @test
  write_justfile <<'JUSTFILE'
build:
  echo building

lint:
  echo linting

test:
  echo testing
JUSTFILE

  run_tap build lint test
  assert_success
  assert_line --partial "1..3"
  assert_line --partial "ok 1 - build"
  assert_line --partial "ok 2 - lint"
  assert_line --partial "ok 3 - test"
  validate_tap
}

function buffered_single_fail { # @test
  write_justfile <<'JUSTFILE'
test:
  @exit 1
JUSTFILE

  run_tap test
  assert_failure
  assert_line --partial "1..1"
  assert_line --partial "not ok 1 - test"
  validate_tap
}

function buffered_middle_fails_others_still_run { # @test
  write_justfile <<'JUSTFILE'
build:
  echo building

test:
  @exit 1

lint:
  echo linting
JUSTFILE

  run_tap build test lint
  assert_failure
  assert_line --partial "1..3"
  assert_line --partial "ok 1 - build"
  assert_line --partial "not ok 2 - test"
  assert_line --partial "ok 3 - lint"
  validate_tap
}

function buffered_first_fails_others_still_run { # @test
  write_justfile <<'JUSTFILE'
test:
  @exit 1

build:
  echo building

lint:
  echo linting
JUSTFILE

  run_tap test build lint
  assert_failure
  assert_line --partial "1..3"
  assert_line --partial "not ok 1 - test"
  assert_line --partial "ok 2 - build"
  assert_line --partial "ok 3 - lint"
  validate_tap
}

# --- Structural validity: dependency failures ---

function buffered_dependency_fails_plan_valid { # @test
  write_justfile <<'JUSTFILE'
compile:
  @exit 1

build: compile
  echo building
JUSTFILE

  run_tap build
  assert_failure
  assert_line --partial "not ok 1 - compile"
  assert_line --partial "not ok 2 - build"
  validate_tap
}

function buffered_deep_dependency_fails_plan_valid { # @test
  write_justfile <<'JUSTFILE'
fetch:
  @exit 1

compile: fetch
  echo compiling

build: compile
  echo building
JUSTFILE

  run_tap build
  assert_failure
  assert_line --partial "1..3"
  assert_line --partial "not ok 1 - fetch"
  assert_line --partial "not ok 2 - compile"
  assert_line --partial "not ok 3 - build"
  validate_tap
}

function buffered_dep_failure_in_multi_recipe { # @test
  write_justfile <<'JUSTFILE'
a:
  echo a-ok

b:
  echo b-ok

c: a
  @exit 1

d:
  echo d-ok
JUSTFILE

  run_tap b c d
  assert_failure
  assert_line --partial "ok 1 - b"
  assert_line --partial "ok 2 - a"
  assert_line --partial "not ok 3 - c"
  assert_line --partial "ok 4 - d"
  validate_tap
}

# --- Structural validity: streamed ---

function streamed_single_pass { # @test
  write_justfile <<'JUSTFILE'
build:
  echo hello
JUSTFILE

  run_tap_streamed build
  assert_success
  assert_line --index 0 "TAP version 14"
  assert_line --partial "ok 1 - build"
  validate_tap
}

function streamed_single_fail { # @test
  write_justfile <<'JUSTFILE'
test:
  @exit 1
JUSTFILE

  run_tap_streamed test
  assert_failure
  assert_line --partial "not ok 1 - test"
  validate_tap
}

function streamed_output_uses_output_block { # @test
  write_justfile <<'JUSTFILE'
build:
  echo line1
  echo line2
JUSTFILE

  run_tap_streamed build
  assert_success
  # Output Block header: # Output: N - recipe
  assert_line --partial "# Output: 1 - build"
  # Body lines use 4-space indent, not # prefix
  assert_line --partial "    line1"
  assert_line --partial "    line2"
  validate_tap
}

# --- Structural validity: stderr mode ---

function stderr_single_pass { # @test
  write_justfile <<'JUSTFILE'
build:
  echo hello
JUSTFILE

  run_tap_stderr build
  assert_success
  assert_line --partial "1..1"
  assert_line --partial "ok 1 - build"
  validate_tap
}

# --- YAML output blocks ---

function buffered_output_in_yaml_block { # @test
  write_justfile <<'JUSTFILE'
build:
  echo captured-output
JUSTFILE

  run_tap build
  assert_success
  assert_line --partial "output:"
  assert_line --partial "captured-output"
  validate_tap
}

function buffered_quiet_recipe_no_yaml { # @test
  write_justfile <<'JUSTFILE'
@build:
  echo quiet-output
JUSTFILE

  run_tap build
  assert_success
  assert_line --partial "ok 1 - build"
  refute_line --partial "output:"
  validate_tap
}

function buffered_no_output_no_yaml { # @test
  write_justfile <<'JUSTFILE'
build:
  @true
JUSTFILE

  run_tap build
  assert_success
  assert_line --partial "ok 1 - build"
  refute_line --partial "output:"
  validate_tap
}

# --- Dependencies ---

function buffered_dependency_chain { # @test
  write_justfile <<'JUSTFILE'
compile:
  echo compiling

build: compile
  echo building

test: build
  echo testing
JUSTFILE

  run_tap test
  assert_success
  assert_line --partial "1..3"
  assert_line --partial "ok 1 - compile"
  assert_line --partial "ok 2 - build"
  assert_line --partial "ok 3 - test"
  validate_tap
}

function buffered_shared_dep_runs_once { # @test
  write_justfile <<'JUSTFILE'
compile:
  echo compiling

build: compile
  echo building

test: compile
  echo testing
JUSTFILE

  run_tap build test
  assert_success
  assert_line --partial "1..3"
  assert_line --partial "ok 1 - compile"
  assert_line --partial "ok 2 - build"
  assert_line --partial "ok 3 - test"
  validate_tap
}

# --- Recipe doc comments ---

function buffered_recipe_comment_in_description { # @test
  write_justfile <<'JUSTFILE'
# Build the project
build:
  echo building
JUSTFILE

  run_tap build
  assert_success
  assert_line --partial "ok 1 - build # Build the project"
  validate_tap
}

function buffered_doc_attribute_in_description { # @test
  write_justfile <<'JUSTFILE'
[doc("Run the test suite")]
test:
  echo testing
JUSTFILE

  run_tap test
  assert_success
  assert_line --partial "ok 1 - test # Run the test suite"
  validate_tap
}

# --- Locale pragma ---

function locale_pragma_emitted { # @test
  write_justfile <<'JUSTFILE'
@build:
  true
JUSTFILE

  LC_ALL=en_US.UTF-8 run_tap build
  assert_success
  assert_line --partial "pragma +locale-formatting:en-US"
  validate_tap
}

function no_locale_pragma_for_c_locale { # @test
  write_justfile <<'JUSTFILE'
@build:
  true
JUSTFILE

  LC_ALL=C run_tap build
  assert_success
  refute_line --partial "pragma +locale-formatting"
  validate_tap
}

# --- Configuration ---

function env_var_sets_output_format { # @test
  write_justfile <<'JUSTFILE'
build:
  echo hello
JUSTFILE

  LC_ALL=C JUST_OUTPUT_FORMAT=tap run "$JUST_ME_BIN" --color never --justfile "$TEST_DIR/justfile" build
  assert_success
  assert_line --index 0 "TAP version 14"
  assert_line --partial "ok 1 - build"
  validate_tap
}

function justfile_setting_sets_output_format { # @test
  write_justfile <<'JUSTFILE'
set output-format := "tap"

build:
  echo hello
JUSTFILE

  LC_ALL=C run "$JUST_ME_BIN" --color never --output-format default --justfile "$TEST_DIR/justfile" build
  assert_success
  # CLI --output-format default overrides justfile setting
  refute_line --partial "TAP version 14"
}

function cli_overrides_justfile_setting { # @test
  write_justfile <<'JUSTFILE'
set output-format := "tap+streamed_output"

build:
  echo hello
JUSTFILE

  LC_ALL=C run_tap build
  assert_success
  assert_line --partial "1..1"
  assert_line --partial "ok 1 - build"
  # Buffered tap has YAML output block, streamed does not
  assert_line --partial "output:"
  validate_tap
}

# --- Subtest detection (#3) ---

function buffered_recipe_tap_output_becomes_subtest { # @test
  write_justfile <<'JUSTFILE'
test:
  @printf 'TAP version 14\n1..2\nok 1 - sub-a\nok 2 - sub-b\n'
JUSTFILE

  run_tap test
  assert_success
  assert_line --partial "# Subtest: test"
  assert_line --partial "    ok 1 - sub-a"
  assert_line --partial "    ok 2 - sub-b"
  assert_line --partial "ok 1 - test"
  validate_tap
}

function streamed_recipe_tap_output_becomes_subtest { # @test
  write_justfile <<'JUSTFILE'
test:
  @printf 'TAP version 14\n1..2\nok 1 - sub-a\nok 2 - sub-b\n'
JUSTFILE

  run_tap_streamed test
  assert_success
  assert_line --partial "# Subtest: test"
  assert_line --partial "    ok 1 - sub-a"
  assert_line --partial "    ok 2 - sub-b"
  assert_line --partial "ok 1 - test"
  validate_tap
}

function buffered_failing_subtest { # @test
  write_justfile <<'JUSTFILE'
test:
  @printf 'TAP version 14\n1..2\nok 1 - sub-a\nnot ok 2 - sub-b\n' && exit 1
JUSTFILE

  run_tap test
  assert_failure
  assert_line --partial "# Subtest: test"
  assert_line --partial "not ok 2 - sub-b"
  assert_line --partial "not ok 1 - test"
  validate_tap
}

function buffered_non_tap_output_not_subtest { # @test
  write_justfile <<'JUSTFILE'
build:
  echo "not a TAP document"
JUSTFILE

  run_tap build
  assert_success
  refute_line --partial "# Subtest"
  assert_line --partial "output:"
  validate_tap
}

# --- Recursive subtests ---

function buffered_recursive_subtest { # @test
  write_justfile <<'JUSTFILE'
test:
  @printf 'TAP version 14\n1..2\n    # Subtest: unit\n    TAP version 14\n    1..2\n    ok 1 - alpha\n    ok 2 - beta\nok 1 - unit\nok 2 - integration\n'
JUSTFILE

  run_tap test
  assert_success
  assert_line --partial "# Subtest: test"
  # inner subtest header gets 4 more spaces (8 total)
  assert_line --partial "        # Subtest: unit"
  assert_line --partial "        ok 1 - alpha"
  assert_line --partial "        ok 2 - beta"
  assert_line --partial "    ok 1 - unit"
  assert_line --partial "    ok 2 - integration"
  assert_line --partial "ok 1 - test"
  validate_tap
}

function streamed_recursive_subtest { # @test
  write_justfile <<'JUSTFILE'
test:
  @printf 'TAP version 14\n1..2\n    # Subtest: unit\n    TAP version 14\n    1..2\n    ok 1 - alpha\n    ok 2 - beta\nok 1 - unit\nok 2 - integration\n'
JUSTFILE

  run_tap_streamed test
  assert_success
  assert_line --partial "# Subtest: test"
  assert_line --partial "        # Subtest: unit"
  assert_line --partial "        ok 1 - alpha"
  assert_line --partial "        ok 2 - beta"
  assert_line --partial "    ok 1 - unit"
  assert_line --partial "    ok 2 - integration"
  assert_line --partial "ok 1 - test"
  validate_tap
}

function buffered_recursive_subtest_inner_failure { # @test
  write_justfile <<'JUSTFILE'
test:
  @printf 'TAP version 14\n1..1\n    # Subtest: unit\n    TAP version 14\n    1..2\n    ok 1 - alpha\n    not ok 2 - beta\nnot ok 1 - unit\n' && exit 1
JUSTFILE

  run_tap test
  assert_failure
  assert_line --partial "# Subtest: test"
  assert_line --partial "        # Subtest: unit"
  assert_line --partial "        not ok 2 - beta"
  assert_line --partial "    not ok 1 - unit"
  assert_line --partial "not ok 1 - test"
  validate_tap
}

function buffered_triple_nested_subtest { # @test
  write_justfile <<'JUSTFILE'
test:
  @printf 'TAP version 14\n1..1\n    # Subtest: suite\n    TAP version 14\n    1..1\n        # Subtest: case\n        TAP version 14\n        1..1\n        ok 1 - assertion\n    ok 1 - case\nok 1 - suite\n'
JUSTFILE

  run_tap test
  assert_success
  assert_line --partial "# Subtest: test"
  # suite subtest at 8 spaces
  assert_line --partial "        # Subtest: suite"
  # case subtest at 12 spaces
  assert_line --partial "            # Subtest: case"
  assert_line --partial "            ok 1 - assertion"
  assert_line --partial "        ok 1 - case"
  assert_line --partial "    ok 1 - suite"
  assert_line --partial "ok 1 - test"
  validate_tap
}

function buffered_recursive_subtest_with_plain_sibling { # @test
  write_justfile <<'JUSTFILE'
build:
  echo building

test:
  @printf 'TAP version 14\n1..1\n    # Subtest: unit\n    TAP version 14\n    1..1\n    ok 1 - alpha\nok 1 - unit\n'
JUSTFILE

  run_tap build test
  assert_success
  assert_line --partial "1..2"
  # build is plain, no subtest
  assert_line --partial "ok 1 - build"
  # test has recursive subtest
  assert_line --partial "# Subtest: test"
  assert_line --partial "        # Subtest: unit"
  assert_line --partial "        ok 1 - alpha"
  assert_line --partial "    ok 1 - unit"
  assert_line --partial "ok 2 - test"
  validate_tap
}

# --- Recursive subtest edge cases ---

function streamed_recursive_subtest_inner_failure { # @test
  write_justfile <<'JUSTFILE'
test:
  @printf 'TAP version 14\n1..1\n    # Subtest: unit\n    TAP version 14\n    1..2\n    ok 1 - alpha\n    not ok 2 - beta\nnot ok 1 - unit\n' && exit 1
JUSTFILE

  run_tap_streamed test
  assert_failure
  assert_line --partial "# Subtest: test"
  assert_line --partial "        # Subtest: unit"
  assert_line --partial "        not ok 2 - beta"
  assert_line --partial "    not ok 1 - unit"
  assert_line --partial "not ok 1 - test"
  validate_tap
}

function streamed_triple_nested_subtest { # @test
  write_justfile <<'JUSTFILE'
test:
  @printf 'TAP version 14\n1..1\n    # Subtest: suite\n    TAP version 14\n    1..1\n        # Subtest: case\n        TAP version 14\n        1..1\n        ok 1 - assertion\n    ok 1 - case\nok 1 - suite\n'
JUSTFILE

  run_tap_streamed test
  assert_success
  assert_line --partial "# Subtest: test"
  assert_line --partial "        # Subtest: suite"
  assert_line --partial "            # Subtest: case"
  assert_line --partial "            ok 1 - assertion"
  assert_line --partial "        ok 1 - case"
  assert_line --partial "    ok 1 - suite"
  assert_line --partial "ok 1 - test"
  validate_tap
}

function buffered_subtest_with_yaml_diagnostics { # @test
  write_justfile <<'JUSTFILE'
test:
  @printf 'TAP version 14\n1..1\nok 1 - alpha\n  ---\n  duration_ms: 42\n  ...\n'
JUSTFILE

  run_tap test
  assert_success
  assert_line --partial "# Subtest: test"
  assert_line --partial "    ok 1 - alpha"
  assert_line --partial "      ---"
  assert_line --partial "      duration_ms: 42"
  assert_line --partial "      ..."
  assert_line --partial "ok 1 - test"
  validate_tap
}

function streamed_subtest_with_yaml_diagnostics { # @test
  write_justfile <<'JUSTFILE'
test:
  @printf 'TAP version 14\n1..1\nok 1 - alpha\n  ---\n  duration_ms: 42\n  ...\n'
JUSTFILE

  run_tap_streamed test
  assert_success
  assert_line --partial "# Subtest: test"
  assert_line --partial "    ok 1 - alpha"
  assert_line --partial "      ---"
  assert_line --partial "      duration_ms: 42"
  assert_line --partial "      ..."
  assert_line --partial "ok 1 - test"
  validate_tap
}

function buffered_subtest_with_bail_out { # @test
  write_justfile <<'JUSTFILE'
test:
  @printf 'TAP version 14\n1..3\nok 1 - alpha\nBail out! disk full\n' && exit 1
JUSTFILE

  run_tap test
  assert_failure
  assert_line --partial "# Subtest: test"
  assert_line --partial "    Bail out! disk full"
  assert_line --partial "not ok 1 - test"
  # validate_tap — skipped: tap-dancer rejects Bail out! plan-count mismatch
  # see https://github.com/amarbel-llc/bob/issues/46
}

function buffered_subtest_plan_at_end { # @test
  write_justfile <<'JUSTFILE'
test:
  @printf 'TAP version 14\nok 1 - alpha\nok 2 - beta\n1..2\n'
JUSTFILE

  run_tap test
  assert_success
  assert_line --partial "# Subtest: test"
  assert_line --partial "    ok 1 - alpha"
  assert_line --partial "    ok 2 - beta"
  assert_line --partial "    1..2"
  assert_line --partial "ok 1 - test"
  validate_tap
}

function streamed_subtest_plan_at_end { # @test
  write_justfile <<'JUSTFILE'
test:
  @printf 'TAP version 14\nok 1 - alpha\nok 2 - beta\n1..2\n'
JUSTFILE

  run_tap_streamed test
  assert_success
  assert_line --partial "# Subtest: test"
  assert_line --partial "    ok 1 - alpha"
  assert_line --partial "    ok 2 - beta"
  assert_line --partial "    1..2"
  assert_line --partial "ok 1 - test"
  validate_tap
}

function buffered_subtest_with_todo_and_skip { # @test
  write_justfile <<'JUSTFILE'
test:
  @printf 'TAP version 14\n1..3\nok 1 - alpha\nnot ok 2 - beta # TODO not yet\nok 3 - gamma # SKIP no db\n'
JUSTFILE

  run_tap test
  assert_success
  assert_line --partial "# Subtest: test"
  assert_line --partial "    ok 1 - alpha"
  assert_line --partial "    not ok 2 - beta # TODO not yet"
  assert_line --partial "    ok 3 - gamma # SKIP no db"
  assert_line --partial "ok 1 - test"
  validate_tap
}

function buffered_dep_outputs_tap_becomes_subtest { # @test
  write_justfile <<'JUSTFILE'
compile:
  @printf 'TAP version 14\n1..1\nok 1 - syntax check\n'

build: compile
  echo building
JUSTFILE

  run_tap build
  assert_success
  assert_line --partial "# Subtest: compile"
  assert_line --partial "    ok 1 - syntax check"
  assert_line --partial "ok 1 - compile"
  assert_line --partial "ok 2 - build"
  validate_tap
}

# --- TTY-only ANSI escapes (#5) ---

function streamed_no_ansi_when_not_tty { # @test
  write_justfile <<'JUSTFILE'
build:
  echo hello
JUSTFILE

  run_tap_streamed build
  assert_success
  refute_output --partial $'\x1b'
  validate_tap
}

# --- Empty line filtering (#6) ---

# --- Nested just-me invocation (issue #9) ---

function nested_failure_outer_yaml_includes_output { # @test
  # Recipe produces TAP with a failure. The outer YAML diagnostic
  # for the failed recipe must include an output: field with the
  # subprocess content, same as non-TAP recipe failures do.
  write_justfile <<'JUSTFILE'
test:
  @printf 'TAP version 14\n1..2\nok 1 - sub-a\nnot ok 2 - sub-b\n' && exit 1
JUSTFILE

  run_tap test
  assert_failure
  # Subtest content is present (this already works)
  assert_line --partial "not ok 2 - sub-b"
  assert_line --partial "not ok 1 - test"
  # The outer YAML diagnostic must include the inner output
  assert_line --partial "output:"
}

function buffered_empty_lines_filtered { # @test
  write_justfile <<'JUSTFILE'
build:
  echo line1
  echo ''
  echo line2
JUSTFILE

  run_tap build
  assert_success
  assert_line --partial "line1"
  assert_line --partial "line2"
  refute_line --regexp "^    $"
  validate_tap
}

function streamed_empty_lines_filtered { # @test
  write_justfile <<'JUSTFILE'
build:
  echo line1
  echo ''
  echo line2
JUSTFILE

  run_tap_streamed build
  assert_success
  assert_line --partial "# Output: 1 - build"
  assert_line --partial "    line1"
  assert_line --partial "    line2"
  refute_line --regexp "^    $"
  validate_tap
}

function buffered_failed_yaml_output_no_empty_lines { # @test
  write_justfile <<'JUSTFILE'
build:
  echo line1
  echo ''
  echo line2
  exit 1
JUSTFILE

  run_tap build
  assert_failure
  assert_line --partial "output: |"
  assert_line --partial "    line1"
  assert_line --partial "    line2"
  refute_line --regexp "^    $"
  validate_tap
}

function streamed_no_empty_line_before_test_point { # @test
  write_justfile <<'JUSTFILE'
build:
  echo line1
  echo ''
  echo line2
JUSTFILE

  run_tap_streamed build
  assert_success

  # After the last output line, the next line must be the test point —
  # no blank/whitespace-only line in between.
  local found_line2=false
  for line in "${lines[@]}"; do
    if "$found_line2"; then
      [[ "$line" =~ ^ok\ [0-9] ]] || [[ "$line" =~ ^not\ ok\ [0-9] ]] || \
        { echo "expected test point after last output line, got: '$line'"; return 1; }
      break
    fi
    [[ "$line" == *"    line2"* ]] && found_line2=true
  done
  validate_tap
}

function streamed_failed_yaml_output_no_empty_lines { # @test
  write_justfile <<'JUSTFILE'
build:
  echo line1
  echo ''
  echo line2
  exit 1
JUSTFILE

  run_tap_streamed -v build
  assert_failure
  assert_line --partial "output: |"
  assert_line --partial "    line1"
  assert_line --partial "    line2"
  refute_line --regexp "^    $"
  validate_tap
}
