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

function streamed_comments_show_output { # @test
  write_justfile <<'JUSTFILE'
build:
  echo line1
  echo line2
JUSTFILE

  run_tap_streamed build
  assert_success
  assert_line --partial "# line1"
  assert_line --partial "# line2"
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
  assert_line --partial "# line1"
  assert_line --partial "# line2"
  refute_line --regexp "^# $"
  validate_tap
}
