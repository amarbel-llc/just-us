# TAP-14 Validity Fixes Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** Fix invalid TAP-14 output so all documents pass `tap-dancer validate`.

**Architecture:** Four independent fixes to the TAP output pipeline in
`src/justfile.rs` and `src/recipe.rs`. Each fix has its own bats test proving
the issue, then the code change, then verification. Issues #7, #3, #5, #6.

**Tech Stack:** Rust, tap-dancer crate, bats integration tests

**Rollback:** `git revert` each commit independently --- fixes are additive and
independent.

--------------------------------------------------------------------------------

### Task 1: Fix plan-count mismatch on dependency failure (#7)

**Problem:** When `compile` (a dependency of `build`) fails,
`run_recipe("build")` returns early at line 448 via `?` from `run_dependencies`.
The `build` test point is never emitted, but the plan already declared `1..2`.

**Approach:** When `run_dependencies` fails in TAP mode, don't propagate the
error immediately. Instead, emit a `not ok` test point for the current recipe
with a SKIP-like message indicating the dependency failed, then return the
error.

**Promotion criteria:** N/A

**Files:** - Modify: `src/justfile.rs:436-448` (run_recipe dependency error
handling) - Modify: `zz-tests_bats/tap14_output.bats:124-138` (update
expected-failure test) - Test: `zz-tests_bats/tap14_output.bats`

**Step 1: Verify the existing test fails**

Run:
`cd zz-tests_bats && JUST_ME_BIN=$(which just-me) bats -f "buffered_dependency_fails_plan_valid" tap14_output.bats`
Expected: `not ok 1` with `plan-count-mismatch`

**Step 2: Add bats tests for the fix**

Add two new tests to `zz-tests_bats/tap14_output.bats` --- one for the dependent
recipe showing as `not ok` and one for deep dependency chains:

``` bash
function buffered_dependency_fails_dependent_marked_not_ok { # @test
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
  validate_tap
}
```

**Step 3: Run the new tests to verify they fail**

Run:
`cd zz-tests_bats && JUST_ME_BIN=$(which just-me) bats -f "dependency_fails_dependent_marked|deep_dependency_fails" tap14_output.bats`
Expected: Both fail --- the dependent recipe test points are missing.

**Step 4: Implement the fix**

In `src/justfile.rs`, change `run_recipe` to catch dependency failures in TAP
mode and emit a test point for the current recipe before propagating the error:

``` rust
    // Current code (line 436-448):
    Self::run_dependencies(
      config,
      &context,
      recipe.priors(),
      dotenv,
      &mut evaluator,
      ran,
      recipe,
      scopes,
      search,
      tap,
      output_format,
    )?;

    // Replace with:
    if let Err(dep_error) = Self::run_dependencies(
      config,
      &context,
      recipe.priors(),
      dotenv,
      &mut evaluator,
      ran,
      recipe,
      scopes,
      search,
      tap,
      output_format,
    ) {
      // In TAP mode, emit a test point for this recipe before propagating
      if let Some(tap) = tap {
        let mut tap = tap.lock().unwrap();
        tap.counter += 1;
        tap.failures += 1;
        let number = tap.counter;
        let comment = recipe.doc().map(Into::into);

        let test_result = tap_dancer::TestResult {
          number,
          name: recipe.name().into(),
          ok: false,
          directive: comment,
          error_message: Some(format!(
            "{}",
            dep_error.color_display(Color::never())
          )),
          exit_code: dep_error.code(),
          output: None,
          suppress_yaml: true,
        };

        let mut stdout = io::stdout().lock();
        let mut writer = tap_dancer::TapWriterBuilder::new(&mut stdout)
          .color(tap.color)
          .default_locale()
          .build_without_printing()
          .map_err(|io_error| Error::StdoutIo { io_error })?;
        writer
          .test_point(&test_result)
          .map_err(|io_error| Error::StdoutIo { io_error })?;
      }
      return Err(dep_error);
    }
```

**Step 5: Update the Rust unit test**

In `tests/tap.rs`, find `tap_failing_dependency` (line 165). Update the expected
output to include a `not ok 2 - build` test point:

``` rust
#[test]
fn tap_failing_dependency() {
  Test::new()
    .justfile(
      "
      compile:
        @exit 1

      build: compile
        echo building
      ",
    )
    .env("LC_ALL", "C")
    .output_format(Some("tap"))
    .arg("build")
    .stdout_regex("TAP version 14\n1..2\nnot ok 1 - compile\n  ---\n  message: \".*\"\n  severity: fail\n  exitcode: 1\n  \\.\\.\\.\nnot ok 2 - build\n  ---\n  message: \".*\"\n  severity: fail\n  exitcode: 1\n  \\.\\.\\.\n")
    .stderr("")
    .failure();
}
```

**Step 6: Build and run tests**

Run: `cargo test tap_failing_dependency -- --nocapture` Run:
`cd zz-tests_bats && JUST_ME_BIN=$(cargo bin-path just) bats -f "dependency_fails" tap14_output.bats`
Expected: All pass. The `buffered_dependency_fails_plan_valid` test should now
pass too.

**Step 7: Run full test suite**

Run: `cargo test` Run:
`cd zz-tests_bats && JUST_ME_BIN=$(cargo bin-path just) bats tap14_output.bats`
Expected: All 23+ bats tests pass, no Rust test regressions.

**Step 8: Commit**

    test: add dep-failure plan-count tests and fix (#7)

    When a dependency fails in TAP mode, emit a not-ok test point for the
    dependent recipe before propagating the error. This ensures the plan
    count matches the actual test point count.

    Fixes #7

--------------------------------------------------------------------------------

### Task 2: Subtest detection (#3)

**Problem:** When a recipe's stdout is valid TAP-14, it's flattened into a YAML
`output:` block. It should be rendered as an indented subtest.

**Note:** This is already implemented at HEAD (commits `bfb0806..9e98517`).
Verify the existing implementation works correctly with the tests. If it does,
this task is just adding bats coverage and closing the issue.

**Promotion criteria:** N/A

**Files:** - Modify: `zz-tests_bats/tap14_output.bats` (add subtest tests) -
Test: `zz-tests_bats/tap14_output.bats`

**Step 1: Add bats tests for subtest detection**

``` bash
function buffered_recipe_tap_output_becomes_subtest { # @test
  write_justfile <<'JUSTFILE'
test:
  #!/bin/sh
  echo "TAP version 14"
  echo "1..2"
  echo "ok 1 - sub-a"
  echo "ok 2 - sub-b"
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
  #!/bin/sh
  echo "TAP version 14"
  echo "1..2"
  echo "ok 1 - sub-a"
  echo "ok 2 - sub-b"
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
  #!/bin/sh
  echo "TAP version 14"
  echo "1..2"
  echo "ok 1 - sub-a"
  echo "not ok 2 - sub-b"
  exit 1
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
```

**Step 2: Run the tests**

Run:
`cd zz-tests_bats && JUST_ME_BIN=$(which just-me) bats -f "subtest|non_tap" tap14_output.bats`
Expected: All pass (feature is already implemented at HEAD).

**Step 3: Commit**

    test: add bats coverage for TAP subtest detection (#3)

--------------------------------------------------------------------------------

### Task 3: TTY-only ANSI escapes (#5)

**Problem:** In `tap+streamed_output` mode, `\r\x1b[2K` escape sequences appear
in output even when stdout is not a TTY (piped). Check if this is still an issue
at HEAD.

**Files:** - Modify: `zz-tests_bats/tap14_output.bats` (add clean-output test) -
Possibly modify: `src/recipe.rs` or `src/justfile.rs`

**Step 1: Add a bats test**

Bats captures stdout, so it's inherently non-TTY. Add a test that asserts no
ANSI escapes in streamed output:

``` bash
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
```

**Step 2: Run the test**

Run:
`cd zz-tests_bats && JUST_ME_BIN=$(which just-me) bats -f "streamed_no_ansi" tap14_output.bats`

If it passes, the fix is already in place. If it fails, implement the fix.

**Step 3: If fix needed**

Find where `\r\x1b[2K` is emitted in `src/recipe.rs` or `src/justfile.rs` and
gate it behind `atty::is(Stream::Stdout)` or equivalent.

**Step 4: Commit**

    test: add ANSI escape assertion for non-TTY output (#5)

or

    fix: suppress ANSI escapes when stdout is not a TTY (#5)

--------------------------------------------------------------------------------

### Task 4: Empty line filtering (#6)

**Problem:** Empty lines from recipe output should be filtered in all TAP
formats. Check if this is already working at HEAD.

**Files:** - Modify: `zz-tests_bats/tap14_output.bats` (add empty-line tests) -
Possibly modify: `src/justfile.rs`

**Step 1: Add bats tests**

``` bash
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
  # YAML output should not contain blank lines
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
```

**Step 2: Run the tests**

Run:
`cd zz-tests_bats && JUST_ME_BIN=$(which just-me) bats -f "empty_lines" tap14_output.bats`

If they pass, filtering is already in place. If not, add filtering to the
buffered path.

**Step 3: Commit**

    test: add empty line filtering assertions (#6)
