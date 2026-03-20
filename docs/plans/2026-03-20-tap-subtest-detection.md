# TAP-14 Subtest Detection Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** When a recipe's command outputs a TAP-14 document, render it as a properly indented subtest instead of plain text output.

**Architecture:** Detect TAP-14 output (first non-empty line is `TAP version 14`) in captured recipe output. For non-streamed modes (`tap`, `tap+stderr`), detect post-capture and write as an indented subtest block. For streamed mode (`tap+streamed_output`), detect on the first line and switch from `# ` comment prefixing to 4-space subtest indentation.

**Tech Stack:** Rust, tap-dancer crate (subtest indentation format)

**Rollback:** Revert the commits on this branch. No existing behavior changes for non-TAP output.

---

### Task 1: Add test for subtest detection in `tap` (buffered) mode

**Files:**
- Modify: `tests/tap.rs`

**Step 1: Write the failing test**

Add to end of `tests/tap.rs`:

```rust
#[test]
fn tap_recipe_outputting_tap_becomes_subtest() {
  Test::new()
    .justfile(
      r#"
      test:
        #!/bin/sh
        echo "TAP version 14"
        echo "1..2"
        echo "ok 1 - sub-a"
        echo "ok 2 - sub-b"
      "#,
    )
    .env("LC_ALL", "C")
    .output_format(Some("tap"))
    .arg("test")
    .stdout_regex(
      "TAP version 14\n1..1\n    # Subtest: test\n    TAP version 14\n    1..2\n    ok 1 - sub-a\n    ok 2 - sub-b\nok 1 - test\n",
    )
    .stderr("")
    .success();
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --test integration tap_recipe_outputting_tap_becomes_subtest`
Expected: FAIL — output currently has `output:` YAML block instead of subtest indentation

**Step 3: Commit**

```
git add tests/tap.rs
git commit -m "test: add failing test for TAP subtest detection in buffered mode"
```

---

### Task 2: Add test for subtest detection in `tap+streamed_output` mode

**Files:**
- Modify: `tests/tap.rs`

**Step 1: Write the failing test**

Add to end of `tests/tap.rs`:

```rust
#[test]
fn tap_streamed_recipe_outputting_tap_becomes_subtest() {
  Test::new()
    .justfile(
      r#"
      test:
        #!/bin/sh
        echo "TAP version 14"
        echo "1..2"
        echo "ok 1 - sub-a"
        echo "ok 2 - sub-b"
      "#,
    )
    .env("LC_ALL", "C")
    .output_format(Some("tap+streamed_output"))
    .arg("test")
    .stdout_regex(
      "TAP version 14\n1\\.\\.1\n    # Subtest: test\n    TAP version 14\n    1\\.\\.2\n    ok 1 - sub-a\n    ok 2 - sub-b\nok 1 - test\n",
    )
    .stderr("")
    .success();
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --test integration tap_streamed_recipe_outputting_tap_becomes_subtest`
Expected: FAIL — output currently prefixes each line with `# ` instead of 4-space indent

**Step 3: Commit**

```
git add tests/tap.rs
git commit -m "test: add failing test for TAP subtest detection in streamed mode"
```

---

### Task 3: Implement subtest detection for buffered modes in `justfile.rs`

**Files:**
- Modify: `src/justfile.rs:461-519`

**Step 1: Implement the detection and rewriting logic**

In `justfile.rs`, replace the block starting at line 461 (`if let Some(tap) = tap {`) through line 519 (`writer.test_point(...)?;`) with logic that:

1. After collecting the output buffer, checks if the first non-empty line equals `TAP version 14`
2. If TAP detected: writes `    # Subtest: <recipe_name>` header, then writes each line of the captured output with 4-space indentation, then writes a plain `ok/not ok` test point (no YAML output block)
3. If not TAP: uses the existing `TestResult` + `test_point` path

Replace the section from `if let Some(tap) = tap {` through `writer.test_point(&test_result).map_err(...)`:

```rust
    if let Some(tap) = tap {
      let mut tap = tap.lock().unwrap();
      tap.counter += 1;
      let number = tap.counter;

      let output = tap_output_buf
        .map(|buf| {
          let buf = buf.into_inner().unwrap();
          let raw = String::from_utf8_lossy(&buf);
          raw
            .lines()
            .map(|line| line.trim_end_matches('\r'))
            .filter(|line| !line.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n")
        })
        .filter(|s| !s.is_empty());

      let is_subtest = output
        .as_ref()
        .is_some_and(|o| o.lines().next().is_some_and(|l| l.trim() == "TAP version 14"));

      let quiet =
        recipe.quiet || (module.settings.quiet && !recipe.no_quiet()) || config.verbosity.quiet();

      let comment = recipe.doc().map(Into::into);

      let mut stdout = io::stdout().lock();

      if is_subtest {
        let output = output.unwrap();
        writeln!(stdout, "    # Subtest: {}", recipe.name())
          .map_err(|io_error| Error::StdoutIo { io_error })?;
        for line in output.lines() {
          writeln!(stdout, "    {line}")
            .map_err(|io_error| Error::StdoutIo { io_error })?;
        }

        let test_result = match run_result {
          Ok(()) => tap_dancer::TestResult {
            number,
            name: recipe.name().into(),
            ok: true,
            directive: comment,
            error_message: None,
            exit_code: None,
            output: None,
            suppress_yaml: true,
          },
          Err(ref error) => {
            tap.failures += 1;
            tap_dancer::TestResult {
              number,
              name: recipe.name().into(),
              ok: false,
              directive: comment,
              error_message: Some(format!("{}", error.color_display(Color::never()))),
              exit_code: error.code(),
              output: None,
              suppress_yaml: quiet,
            }
          }
        };

        let mut writer = tap_dancer::TapWriterBuilder::new(&mut stdout)
          .color(tap.color)
          .default_locale()
          .build_without_printing()
          .map_err(|io_error| Error::StdoutIo { io_error })?;
        writer
          .test_point(&test_result)
          .map_err(|io_error| Error::StdoutIo { io_error })?;
      } else {
        let test_result = match run_result {
          Ok(()) => tap_dancer::TestResult {
            number,
            name: recipe.name().into(),
            ok: true,
            directive: comment,
            error_message: None,
            exit_code: None,
            output,
            suppress_yaml: quiet
              || (output_format == OutputFormat::TapStreamedOutput
                && !config.verbosity.loquacious()),
          },
          Err(ref error) => {
            tap.failures += 1;
            tap_dancer::TestResult {
              number,
              name: recipe.name().into(),
              ok: false,
              directive: comment,
              error_message: Some(format!("{}", error.color_display(Color::never()))),
              exit_code: error.code(),
              output,
              suppress_yaml: quiet,
            }
          }
        };

        let mut writer = tap_dancer::TapWriterBuilder::new(&mut stdout)
          .color(tap.color)
          .default_locale()
          .build_without_printing()
          .map_err(|io_error| Error::StdoutIo { io_error })?;
        writer
          .test_point(&test_result)
          .map_err(|io_error| Error::StdoutIo { io_error })?;
      }

      if let Err(error) = run_result {
        return Err(error);
      }
```

**Step 2: Run the buffered subtest test**

Run: `cargo test --test integration tap_recipe_outputting_tap_becomes_subtest`
Expected: PASS

**Step 3: Run all TAP tests to check for regressions**

Run: `cargo test --test integration tap`
Expected: all pass except the streamed subtest test from Task 2

**Step 4: Commit**

```
git add src/justfile.rs
git commit -m "feat: detect TAP-14 output and render as subtest in buffered modes"
```

---

### Task 4: Implement subtest detection for streamed mode in `recipe.rs`

**Files:**
- Modify: `src/recipe.rs:509-528` (run_linewise TapStreamedOutput closure)
- Modify: `src/recipe.rs:743-759` (run_script TapStreamedOutput closure)

**Step 1: Implement streamed subtest detection**

The streaming closures in both `run_linewise` (line 512-528) and `run_script` (line 743-759) currently prefix every non-empty line with `# `. They need to:

1. Track whether the first non-empty line has been seen
2. If the first non-empty line is `TAP version 14`, emit `    # Subtest: <name>` and switch to 4-space indent mode
3. Otherwise, use the existing `# ` prefix behavior

Both closures need access to the recipe name. The recipe name is available as `self.name()` in the enclosing method.

For both `run_linewise` and `run_script`, replace the `TapStreamedOutput` match arm. The closures need a `first_line_seen` state variable alongside the existing `line_buf`:

```rust
          OutputFormat::TapStreamedOutput => {
            let stdout_lock = io::stdout();
            let line_buf = Mutex::new(Vec::<u8>::new());
            let is_tap_subtest = Mutex::new(Option::<bool>::None);
            let recipe_name = self.name();
            stream_command_output(cmd, &|chunk| {
              let mut buf = line_buf.lock().unwrap();
              buf.extend_from_slice(chunk);
              let mut stdout = stdout_lock.lock();
              while let Some(pos) = buf.iter().position(|&b| b == b'\n' || b == b'\r') {
                let line = String::from_utf8_lossy(&buf[..pos]);
                let line = line.trim();
                if !line.is_empty() {
                  let mut is_sub = is_tap_subtest.lock().unwrap();
                  if is_sub.is_none() {
                    if line == "TAP version 14" {
                      *is_sub = Some(true);
                      writeln!(stdout, "    # Subtest: {recipe_name}")?;
                    } else {
                      *is_sub = Some(false);
                    }
                  }
                  if is_sub == Some(true) {
                    writeln!(stdout, "    {line}")?;
                  } else {
                    writeln!(stdout, "# {line}")?;
                  }
                }
                buf.drain(..=pos);
              }
              Ok(())
            })
          }
```

Apply the same change to both the `run_linewise` closure (line 512) and the `run_script` closure (line 743). The only difference is the variable name (`cmd` vs `command`).

**Step 2: Run the streamed subtest test**

Run: `cargo test --test integration tap_streamed_recipe_outputting_tap_becomes_subtest`
Expected: PASS

**Step 3: Run all TAP tests**

Run: `cargo test --test integration tap`
Expected: all pass

**Step 4: Commit**

```
git add src/recipe.rs
git commit -m "feat: detect TAP-14 output and render as subtest in streamed mode"
```

---

### Task 5: Add test for non-TAP output is unaffected

**Files:**
- Modify: `tests/tap.rs`

**Step 1: Write a regression test ensuring non-TAP output is unchanged**

```rust
#[test]
fn tap_recipe_non_tap_output_unchanged() {
  Test::new()
    .justfile(
      "
      build:
        echo 'not a TAP document'
      ",
    )
    .env("LC_ALL", "C")
    .output_format(Some("tap"))
    .arg("build")
    .stdout_regex(
      "TAP version 14\n1..1\nok 1 - build\n  ---\n  output: \"not a TAP document\"\n  \\.\\.\\.\n",
    )
    .stderr("")
    .success();
}
```

**Step 2: Run test**

Run: `cargo test --test integration tap_recipe_non_tap_output_unchanged`
Expected: PASS (existing behavior preserved)

**Step 3: Add test for streamed non-TAP output unchanged**

```rust
#[test]
fn tap_streamed_recipe_non_tap_output_unchanged() {
  Test::new()
    .justfile(
      "
      build:
        echo 'not a TAP document'
      ",
    )
    .env("LC_ALL", "C")
    .output_format(Some("tap+streamed_output"))
    .arg("build")
    .stdout_regex("TAP version 14\n1\\.\\.1\n# not a TAP document\nok 1 - build\n")
    .stderr("")
    .success();
}
```

**Step 4: Run test**

Run: `cargo test --test integration tap_streamed_recipe_non_tap_output_unchanged`
Expected: PASS

**Step 5: Commit**

```
git add tests/tap.rs
git commit -m "test: add regression tests for non-TAP output unchanged"
```

---

### Task 6: Add test for failing subtest recipe

**Files:**
- Modify: `tests/tap.rs`

**Step 1: Write test for a recipe that outputs TAP but exits non-zero**

```rust
#[test]
fn tap_recipe_outputting_tap_failing_becomes_subtest() {
  Test::new()
    .justfile(
      r#"
      test:
        #!/bin/sh
        echo "TAP version 14"
        echo "1..2"
        echo "ok 1 - sub-a"
        echo "not ok 2 - sub-b"
        exit 1
      "#,
    )
    .env("LC_ALL", "C")
    .output_format(Some("tap"))
    .arg("test")
    .stdout_regex(
      "TAP version 14\n1..1\n    # Subtest: test\n    TAP version 14\n    1..2\n    ok 1 - sub-a\n    not ok 2 - sub-b\nnot ok 1 - test\n  ---\n  message: \".*\"\n  severity: fail\n  exitcode: 1\n  \\.\\.\\.\n",
    )
    .stderr("")
    .failure();
}
```

**Step 2: Run test**

Run: `cargo test --test integration tap_recipe_outputting_tap_failing_becomes_subtest`
Expected: PASS

**Step 3: Commit**

```
git add tests/tap.rs
git commit -m "test: add test for failing recipe with TAP subtest output"
```

---

### Task 7: Run full test suite

**Step 1: Run all tests**

Run: `cargo test --test integration`
Expected: all pass

**Step 2: Run clippy**

Run: `cargo clippy --all-targets`
Expected: no errors

**Step 3: Run fmt check**

Run: `cargo fmt -- --check`
Expected: no formatting issues
