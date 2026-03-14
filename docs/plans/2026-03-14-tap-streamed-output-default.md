# tap+streamed_output Default Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** Make `tap+streamed_output` the default output format, merge the separate `--tap-stream` flag into `--output-format` using pandoc-style `+` syntax, and fix empty-line noise in streamed output.

**Architecture:** Extend `OutputFormat` enum to encode both format and stream mode (`Default`, `Tap`, `TapStreamedOutput`, `TapStderr`). Remove `TapStream` enum and all its plumbing. Add `output_format` field to test harness `Test` struct defaulting to `"default"` so mainline tests are unaffected.

**Tech Stack:** Rust, clap (CLI parsing), strum (enum string conversion), tap-dancer (TAP-14 output)

**Rollback:** Change `#[default]` on `OutputFormat` back to `Default` (one-line revert). Per-invocation opt-out: `--output-format default` or `JUST_OUTPUT_FORMAT=default`.

---

### Task 1: Fix empty-line bug in streamed output

**Files:**
- Modify: `src/recipe.rs:507-511` (run_linewise StreamedOutput callback)
- Modify: `src/recipe.rs:738-742` (run_script StreamedOutput callback)
- Test: `tests/tap.rs` (new test)

**Step 1: Write the failing test**

Add to `tests/tap.rs`:

```rust
#[test]
fn tap_stream_streamed_output_elides_empty_lines() {
  Test::new()
    .justfile(
      "
      build:
        echo line1
        echo ''
        echo line2
      ",
    )
    .args(["--output-format", "tap", "--tap-stream", "streamed-output"])
    .arg("build")
    .stdout_regex("TAP version 14\n1\\.\\.1\npragma \\+streamed-output\n# line1\n# line2\nok 1 - build\n")
    .stderr("")
    .success();
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test tap_stream_streamed_output_elides_empty_lines -- --nocapture`
Expected: FAIL — stdout will contain `# \n` between line1 and line2

**Step 3: Fix the bug in run_linewise**

In `src/recipe.rs:507-511`, change:

```rust
// Before:
let line = line.rsplit('\r').next().unwrap_or(&line);
writeln!(stdout, "# {line}")?;

// After:
let line = line.rsplit('\r').next().unwrap_or(&line);
if !line.is_empty() {
  writeln!(stdout, "# {line}")?;
}
```

**Step 4: Apply same fix in run_script**

In `src/recipe.rs:739-742`, apply the identical change.

**Step 5: Run test to verify it passes**

Run: `cargo test tap_stream_streamed_output_elides_empty_lines -- --nocapture`
Expected: PASS

**Step 6: Run full TAP test suite**

Run: `cargo test --test tap`
Expected: All tests pass

**Step 7: Commit**

```
fix: elide empty lines in TAP streamed output

Empty lines from recipe output were emitted as bare `# ` comment lines,
creating visual noise between test points. Now skip lines that are empty
after \r-stripping.
```

---

### Task 2: Extend OutputFormat enum with pandoc-style variants

**Files:**
- Modify: `src/output_format.rs`
- Modify: `src/justfile.rs:226-248,274-332,366-377,440-444,457,529-541`
- Modify: `src/recipe.rs` (change `tap_stream: TapStream` params to `output_format: OutputFormat`)
- Modify: `src/config.rs:44,131,424-432,883`
- Modify: `src/settings.rs:27`
- Modify: `src/evaluator.rs:135-145`
- Modify: `src/setting.rs:21,35,69-75`
- Modify: `src/parser.rs:1341`
- Modify: `src/node.rs:331`
- Modify: `src/keyword.rs:31`
- Modify: `src/lib.rs:97,287`
- Delete: `src/tap_stream.rs`

**Step 1: Rewrite OutputFormat enum**

Replace `src/output_format.rs` with:

```rust
use super::*;

#[derive(Debug, Default, PartialEq, Clone, Copy, Serialize)]
pub(crate) enum OutputFormat {
  #[default]
  TapStreamedOutput,
  Default,
  Tap,
  TapStderr,
}

impl std::str::FromStr for OutputFormat {
  type Err = String;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    match s {
      "tap+streamed_output" => Ok(Self::TapStreamedOutput),
      "default" => Ok(Self::Default),
      "tap" => Ok(Self::Tap),
      "tap+stderr" => Ok(Self::TapStderr),
      other => Err(format!("unknown output format: {other}")),
    }
  }
}

impl std::fmt::Display for OutputFormat {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::TapStreamedOutput => write!(f, "tap+streamed_output"),
      Self::Default => write!(f, "default"),
      Self::Tap => write!(f, "tap"),
      Self::TapStderr => write!(f, "tap+stderr"),
    }
  }
}

impl OutputFormat {
  pub(crate) fn is_tap(self) -> bool {
    matches!(self, Self::Tap | Self::TapStreamedOutput | Self::TapStderr)
  }
}
```

Drop `ValueEnum` and `EnumString` derives since the `+` syntax needs custom
parsing. clap will use `FromStr` via `value_parser!` with
`.value_parser(clap::builder::TypedValueParser::map(
  clap::builder::NonEmptyStringValueParser::new(),
  |s| s.parse::<OutputFormat>().unwrap(),
))` or just use `.value_parser(|s: &str| s.parse::<OutputFormat>())`.

**Step 2: Update justfile.rs — resolve output format and route to TAP**

In `src/justfile.rs:226-248`, replace:

```rust
// Before:
let output_format = config
  .output_format
  .or(self.settings.output_format)
  .unwrap_or_default();

if output_format == OutputFormat::Tap {
  return self.run_tap(config, &dotenv, &scopes, search, invocations);
}

let ran = Ran::default();
for invocation in invocations {
  Self::run_recipe(
    &invocation.arguments,
    config,
    &dotenv,
    false,
    &ran,
    invocation.recipe,
    &scopes,
    search,
    None,
    TapStream::default(),
  )?;
}

// After:
let output_format = config
  .output_format
  .or(self.settings.output_format)
  .unwrap_or_default();

if output_format.is_tap() {
  return self.run_tap(config, &dotenv, &scopes, search, invocations, output_format);
}

let ran = Ran::default();
for invocation in invocations {
  Self::run_recipe(
    &invocation.arguments,
    config,
    &dotenv,
    false,
    &ran,
    invocation.recipe,
    &scopes,
    search,
    None,
    OutputFormat::Default,
  )?;
}
```

**Step 3: Update run_tap to accept OutputFormat**

In `src/justfile.rs:274-332`, change signature and body:

```rust
fn run_tap(
  &self,
  config: &Config,
  dotenv: &BTreeMap<String, String>,
  scopes: &BTreeMap<String, (&Self, &Scope<'src, '_>)>,
  search: &Search,
  invocations: Vec<Invocation<'src, '_>>,
  output_format: OutputFormat,
) -> RunResult<'src> {
  let mut stdout = io::stdout().lock();

  let mut seen = BTreeSet::<String>::new();
  let mut plan_count = 0;
  for invocation in &invocations {
    plan_count += Self::count_recipes(invocation.recipe, &mut seen, config.no_dependencies);
  }

  tap_dancer::write_version(&mut stdout).map_err(|io_error| Error::StdoutIo { io_error })?;
  tap_dancer::write_plan(&mut stdout, plan_count)
    .map_err(|io_error| Error::StdoutIo { io_error })?;

  if output_format == OutputFormat::TapStreamedOutput {
    tap_dancer::write_pragma(&mut stdout, "streamed-output", true)
      .map_err(|io_error| Error::StdoutIo { io_error })?;
  }

  let tap_tally = Mutex::new(TapTally::new());
  let ran = Ran::default();

  for invocation in &invocations {
    let _ = Self::run_recipe(
      &invocation.arguments,
      config,
      dotenv,
      false,
      &ran,
      invocation.recipe,
      scopes,
      search,
      Some(&tap_tally),
      output_format,
    );
  }

  let tap = tap_tally.into_inner().unwrap();

  if tap.failures > 0 {
    Err(Error::TapFailure {
      count: tap.counter,
      failures: tap.failures,
    })
  } else {
    Ok(())
  }
}
```

**Step 4: Update run_recipe and run_dependencies signatures**

Change `tap_stream: TapStream` to `output_format: OutputFormat` in:
- `run_recipe` (justfile.rs:376)
- `run_dependencies` (justfile.rs:540)

Update the body of `run_recipe`:
- Line 450: pass `output_format` instead of `tap_stream` to `recipe.run()`
- Line 457: `if output_format == OutputFormat::TapStreamedOutput {`
- Lines 433-445 and 512-524: pass `output_format` to recursive calls

**Step 5: Update recipe.rs**

Change all `tap_stream: TapStream` parameters to `output_format: OutputFormat`.
Replace `TapStream::Buffered` with `OutputFormat::Tap`,
`TapStream::StreamedOutput` with `OutputFormat::TapStreamedOutput`,
`TapStream::Stderr` with `OutputFormat::TapStderr` in the match arms
(lines 498-522, 729-753).

**Step 6: Remove TapStream from config.rs**

- Remove `tap_stream` field from Config struct (line 44)
- Remove `TAP_STREAM` constant from arg module (line 131)
- Remove the `--tap-stream` Arg definition (lines 424-432)
- Remove `tap_stream` from the `from_matches` assignment (line 883)

**Step 7: Remove TapStream from settings.rs**

- Remove `tap_stream` field (line 27)

**Step 8: Remove TapStream from evaluator.rs**

- Remove the `Setting::TapStream` match arm (lines 135-145)

**Step 9: Remove TapStream from setting.rs, parser.rs, node.rs, keyword.rs**

- `setting.rs`: Remove `TapStream(Expression<'src>)` variant and all match arms
- `parser.rs:1341`: Remove `Keyword::TapStream` case
- `node.rs:331`: Remove `Setting::TapStream` from match arm
- `keyword.rs:31`: Remove `TapStream` keyword

**Step 10: Remove tap_stream module from lib.rs**

- Remove `tap_stream::TapStream` use (line 97)
- Remove `mod tap_stream` (line 287)

**Step 11: Delete src/tap_stream.rs**

**Step 12: Update clap value_parser for OutputFormat**

In `config.rs`, the `--output-format` arg currently uses
`clap::value_parser!(OutputFormat)` which requires `ValueEnum`. Since we
now use custom `FromStr`, change to:

```rust
.value_parser(|s: &str| s.parse::<OutputFormat>())
```

Also update the help text:

```rust
.help("Set output format (default, tap, tap+streamed_output, tap+stderr)")
```

**Step 13: Compile and fix any remaining references**

Run: `cargo build 2>&1`
Expected: Compiles clean. Fix any remaining TapStream references.

**Step 14: Run all tests**

Run: `cargo test --test tap`
Expected: Many failures — tests using `--tap-stream` flag no longer exists.

**Step 15: Commit**

```
refactor: merge tap-stream into output-format with pandoc-style syntax

Replace separate --output-format and --tap-stream flags with a single
--output-format flag using pandoc-style + syntax:
- tap+streamed_output (new default)
- tap+stderr
- tap (buffered)
- default (plain output)

Remove --tap-stream CLI flag, JUST_TAP_STREAM env var, set tap-stream
justfile setting, TapStream enum, and src/tap_stream.rs.
```

---

### Task 3: Update test harness to inject --output-format default

**Files:**
- Modify: `tests/test.rs:13-54,239-243`

**Step 1: Add output_format field to Test struct**

In `tests/test.rs`, add field to struct:

```rust
pub(crate) struct Test {
  pub(crate) args: Vec<String>,
  pub(crate) current_dir: PathBuf,
  pub(crate) env: BTreeMap<String, String>,
  pub(crate) expected_files: BTreeMap<PathBuf, Vec<u8>>,
  pub(crate) justfile: Option<String>,
  pub(crate) output_format: Option<String>,  // NEW
  // ... rest unchanged
}
```

Default it in `with_tempdir`:

```rust
output_format: Some("default".into()),
```

Add builder method:

```rust
pub(crate) fn output_format(mut self, format: Option<&str>) -> Self {
  self.output_format = format.map(Into::into);
  self
}
```

**Step 2: Inject --output-format in Test::status()**

In `Test::status()`, after the `--shell bash` injection (line 241-243), add:

```rust
if let Some(ref format) = self.output_format {
  command.args(["--output-format", format]);
}
```

**Step 3: Compile and run tests**

Run: `cargo test 2>&1 | tail -20`
Expected: Mainline tests pass (they get `--output-format default` injected).

**Step 4: Commit**

```
test: inject --output-format default in test harness

Add output_format field to Test struct defaulting to "default" so
mainline tests are unaffected by the default change to
tap+streamed_output. TAP tests can override with .output_format(Some("tap")).
```

---

### Task 4: Update TAP tests for new syntax

**Files:**
- Modify: `tests/tap.rs`
- Modify: `tests/json.rs:98`

**Step 1: Update TAP tests**

Replace all occurrences:
- `.args(["--output-format", "tap", "--tap-stream", "comments"])` →
  `.output_format(Some("tap+streamed_output"))`
- `.args(["--output-format", "tap", "--tap-stream", "streamed-output"])` →
  `.output_format(Some("tap+streamed_output"))`
- `.args(["--output-format", "tap", "--tap-stream", "stderr"])` →
  `.output_format(Some("tap+stderr"))`
- `.args(["--output-format", "tap", "--tap-stream", "buffered"])` →
  `.output_format(Some("tap"))`
- `.args(["--output-format", "tap"])` → `.output_format(Some("tap"))`
- `.args(["--output-format", "default"])` → `.output_format(Some("default"))`
- `.env("JUST_OUTPUT_FORMAT", "tap")` → keep as-is
- `.env("JUST_TAP_STREAM", "comments")` → remove (test becomes invalid, rewrite)
- `.args(["--tap-stream", "buffered"])` → `.output_format(Some("tap"))`

For tests that use `set tap-stream := "comments"` in justfile content, replace with
`set output-format := "tap+streamed_output"` and remove the separate `set output-format := "tap"`.

For `tap_stream_cli_overrides_setting` test — rewrite to test
`--output-format tap` overriding `set output-format := "tap+streamed_output"`.

For `tap_stream_env_var` test — rewrite to use
`JUST_OUTPUT_FORMAT=tap+streamed_output`.

Remove redundant `--output-format` args from tests that already use
`.output_format()`.

**Step 2: Remove tap_stream from json.rs Settings**

In `tests/json.rs:98`, remove the `tap_stream` field from the Settings struct.

**Step 3: Run all tests**

Run: `cargo test`
Expected: All tests pass

**Step 4: Commit**

```
test: update TAP tests for merged output-format syntax

Replace --tap-stream flag usage with tap+streamed_output,
tap+stderr pandoc-style output-format values. Remove tap_stream
from JSON settings test struct.
```

---

### Task 5: Switch default to tap+streamed_output

This is already done in Task 2 (the enum has `#[default]` on
`TapStreamedOutput`). This task validates it works end-to-end.

**Step 1: Write test for default behavior**

Add to `tests/tap.rs`:

```rust
#[test]
fn default_output_is_tap_streamed() {
  Test::new()
    .justfile(
      "
      build:
        echo hello
      ",
    )
    .output_format(None)
    .arg("build")
    .stdout_regex("TAP version 14\n1\\.\\.1\npragma \\+streamed-output\n# hello\nok 1 - build\n")
    .stderr("")
    .success();
}
```

**Step 2: Run test**

Run: `cargo test default_output_is_tap_streamed -- --nocapture`
Expected: PASS

**Step 3: Write test for opting out**

Add to `tests/tap.rs`:

```rust
#[test]
fn output_format_default_produces_plain_output() {
  Test::new()
    .justfile(
      "
      build:
        echo hello
      ",
    )
    .output_format(Some("default"))
    .arg("build")
    .stdout("hello\n")
    .stderr("echo hello\n")
    .success();
}
```

**Step 4: Run all tests**

Run: `cargo test`
Expected: All pass

**Step 5: Commit**

```
test: validate tap+streamed_output default and opt-out
```

---

### Task 6: Validate with tap-dancer and human review

**Step 1: Build the binary**

Run: `cargo build`

**Step 2: Create a sample justfile for manual testing**

Create `/tmp/test-justfile`:

```
# Build the project
build:
  echo "building..."
  echo "done"

# Run tests
test: build
  echo "testing..."
  echo ""
  echo "all passed"

fail:
  @exit 1
```

**Step 3: Test default output (tap+streamed_output)**

Run: `./target/debug/just --justfile /tmp/test-justfile build`
Expected: TAP-14 with `pragma +streamed-output`, comment lines, no bare `# ` for empty lines

**Step 4: Test opt-out**

Run: `./target/debug/just --justfile /tmp/test-justfile --output-format default build`
Expected: Plain output like upstream just

**Step 5: Validate with tap-dancer**

Run: `./target/debug/just --justfile /tmp/test-justfile build test 2>/dev/null | tap-dancer validate`
Expected: Valid TAP-14

Run: `./target/debug/just --justfile /tmp/test-justfile --output-format tap build test 2>/dev/null | tap-dancer validate`
Expected: Valid TAP-14

**Step 6: Test failure output**

Run: `./target/debug/just --justfile /tmp/test-justfile fail 2>/dev/null | tap-dancer validate`
Expected: Valid TAP-14 (not ok test point)

**Step 7: Test empty line elision**

Run: `./target/debug/just --justfile /tmp/test-justfile test 2>/dev/null`
Verify: No bare `# ` lines between `# testing...` and `# all passed`

**Step 8: Run full cargo test suite**

Run: `cargo test`
Expected: All pass

**Step 9: Commit (if any fixes needed)**

---

### Task 7: Update completions

**Step 1: Regenerate shell completions**

Run: `cargo run -- --completions bash > completions/just.bash`
Run: `cargo run -- --completions zsh > completions/just.zsh`
Run: `cargo run -- --completions fish > completions/just.fish`
Run: `cargo run -- --completions powershell > completions/just.ps1`
Run: `cargo run -- --completions elvish > completions/just.elvish`

Note: This may not be needed if completions are not checked in, or if
clap auto-generates them from the value_parser. Check if completions
directory exists first.

**Step 2: Commit**

```
chore: regenerate shell completions for merged output-format
```
