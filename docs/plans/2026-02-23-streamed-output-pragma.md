# `pragma +streamed-output` Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Emit `pragma +streamed-output` when `--tap-stream comments` is active, aligning the existing comments streaming mode with the new TAP-14 streamed-output amendment. Also rename the `Comments` variant to `StreamedOutput` to match the spec terminology.

**Architecture:** Rename `TapStream::Comments` to `TapStream::StreamedOutput` (keeping `comments` as the CLI value for backwards compat via strum rename). In `run_tap`, emit `pragma +streamed-output` after the version+plan lines when the resolved tap-stream mode is `StreamedOutput`. The streaming behavior in `recipe.rs` is unchanged — it already emits `# line` comments. The YAML `output` field is already omitted for this mode (producer's choice per the amendment).

**Tech Stack:** Rust, clap (ValueEnum), strum (EnumString)

---

### Task 1: Rename `TapStream::Comments` to `TapStream::StreamedOutput`

**Files:**
- Modify: `src/tap_stream.rs:8`

**Step 1: Rename the variant**

Change line 8 from:
```rust
  Comments,
```
to:
```rust
  #[strum(serialize = "comments")]
  #[value(alias = "comments")]
  StreamedOutput,
```

The `strum(serialize)` keeps `"comments".parse::<TapStream>()` working for the justfile setting. The `value(alias)` keeps `--tap-stream comments` working on the CLI. The canonical kebab-case name becomes `streamed-output`.

**Step 2: Verify it compiles**

Run: `cargo check 2>&1 | head -30`

Expected: errors about `TapStream::Comments` no longer existing in recipe.rs and justfile.rs.

**Step 3: Update all `TapStream::Comments` references**

In `src/recipe.rs`, replace both occurrences of `TapStream::Comments` with `TapStream::StreamedOutput` (lines 500 and 731).

In `src/justfile.rs`, replace `TapStream::Comments` with `TapStream::StreamedOutput` (line 438).

**Step 4: Verify it compiles**

Run: `cargo check`

Expected: clean compilation.

**Step 5: Commit**

```
refactor: rename TapStream::Comments to StreamedOutput
```

---

### Task 2: Add `write_pragma` to `tap_output.rs`

**Files:**
- Modify: `src/tap_output.rs`

**Step 1: Write the failing test**

Add to the `tests` module in `src/tap_output.rs`:

```rust
#[test]
fn pragma_line() {
  let mut buf = Vec::new();
  write_pragma(&mut buf, "streamed-output").unwrap();
  assert_eq!(
    String::from_utf8(buf).unwrap(),
    "pragma +streamed-output\n"
  );
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test --lib tap_output::tests::pragma_line`

Expected: FAIL — `write_pragma` not found.

**Step 3: Write minimal implementation**

Add after `write_plan` in `src/tap_output.rs`:

```rust
pub(crate) fn write_pragma(writer: &mut impl Write, name: &str) -> io::Result<()> {
  writeln!(writer, "pragma +{name}")
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test --lib tap_output::tests::pragma_line`

Expected: PASS.

**Step 5: Commit**

```
feat: add write_pragma to tap_output
```

---

### Task 3: Emit pragma in `run_tap` when StreamedOutput is active

**Files:**
- Modify: `src/justfile.rs:276-278`

**Step 1: Add pragma emission after version+plan**

In `run_tap`, after the `write_plan` call (line 278) and after `tap_stream` is resolved (line 283), add:

```rust
if tap_stream == TapStream::StreamedOutput {
  tap_output::write_pragma(&mut stdout, "streamed-output")
    .map_err(|io_error| Error::StdoutIo { io_error })?;
}
```

The pragma must come after the version and plan lines but before any test points, per TAP-14 pragma placement rules.

**Step 2: Verify it compiles**

Run: `cargo check`

Expected: clean compilation.

**Step 3: Commit**

```
feat: emit pragma +streamed-output for StreamedOutput tap-stream mode
```

---

### Task 4: Update existing tests to expect pragma line

**Files:**
- Modify: `tests/tap.rs`

**Step 1: Update all comments-mode test regexes**

Every test that uses `--tap-stream comments` or `set tap-stream := "comments"` now needs to expect `pragma \+streamed-output\n` after the plan line.

Update these tests:

- `tap_stream_comments_single_recipe` (line 367): change regex from
  `TAP version 14\n1\\.\\.1\n# hello\nok 1 - build\n`
  to
  `TAP version 14\n1\\.\\.1\npragma \\+streamed-output\n# hello\nok 1 - build\n`

- `tap_stream_comments_failing` (line 383): insert `pragma \\+streamed-output\n` after `1\\.\\.1\n`

- `tap_stream_comments_no_output_field` (line 399): insert `pragma \\+streamed-output\n` after `1\\.\\.1\n`

- `tap_stream_justfile_setting` (line 464): insert `pragma \\+streamed-output\n` after `1\\.\\.1\n`

- `tap_stream_cli_overrides_setting` (line 483): this test overrides to `buffered`, so NO change needed

- `tap_stream_env_var` (line 500): insert `pragma \\+streamed-output\n` after `1\\.\\.1\n`

- `tap_stream_comments_multiline` (line 517): insert `pragma \\+streamed-output\n` after `1\\.\\.1\n`

**Step 2: Run the TAP tests**

Run: `cargo test --test integration tap -- --test-threads=1`

Expected: all tests pass.

**Step 3: Commit**

```
test: update comments-mode TAP tests to expect pragma +streamed-output
```

---

### Task 5: Add test for `--tap-stream streamed-output` canonical name

**Files:**
- Modify: `tests/tap.rs`

**Step 1: Write the test**

Add a new test:

```rust
#[test]
fn tap_stream_streamed_output_canonical_name() {
  Test::new()
    .justfile(
      "
      build:
        echo hello
      ",
    )
    .args(["--output-format", "tap", "--tap-stream", "streamed-output"])
    .arg("build")
    .stdout_regex("TAP version 14\n1\\.\\.1\npragma \\+streamed-output\n# hello\nok 1 - build\n")
    .stderr("")
    .success();
}
```

**Step 2: Run the test**

Run: `cargo test --test integration tap_stream_streamed_output_canonical_name`

Expected: PASS (the alias and canonical name should both work via strum/clap).

**Step 3: Commit**

```
test: add test for --tap-stream streamed-output canonical name
```

---

### Task 6: Update completions for new canonical name

**Files:**
- Modify: `completions/just.bash`
- Modify: `completions/just.zsh`
- Modify: `completions/just.fish`
- Modify: `completions/just.elvish`
- Modify: `completions/just.powershell`

**Step 1: Check if completions reference tap-stream values**

Search completions files for `buffered`, `comments`, or `stderr` references. If they list valid values for `--tap-stream`, add `streamed-output` as an option alongside `comments`.

**Step 2: Update if needed, verify compilation**

Run: `cargo check`

**Step 3: Commit (if changes needed)**

```
chore: update completions for streamed-output tap-stream value
```

---

### Task 7: Validate with tap-dancer

**Files:** None (validation only)

**Step 1: Build just**

Run: `cargo build`

**Step 2: Run just with streamed-output and pipe to tap-dancer validate**

Create a temporary justfile and validate the output:

```bash
echo 'build:
  echo hello
  echo world
test:
  @exit 1' > /tmp/test-justfile

./target/debug/just --justfile /tmp/test-justfile --output-format tap --tap-stream streamed-output build test 2>/dev/null | tap-dancer validate
```

Expected: tap-dancer accepts the output as valid TAP-14 (pragmas are valid TAP lines).

**Step 3: Clean up**

```bash
rm /tmp/test-justfile
```

---

### Task 8: Update TODO.md

**Files:**
- Modify: `TODO.md`

**Step 1: Mark the TODO as done**

Replace:
```
- [ ] add support for tap14+streamed_output
```
with:
```
- [x] add support for tap14+streamed_output
```

**Step 2: Commit**

```
chore: mark tap14+streamed_output TODO as complete
```
