# `output-format` Setting Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the `--tap` boolean flag with an `--output-format` enum flag and add a corresponding `set output-format := "tap"` justfile setting, opening the door for future output formats.

**Architecture:** Introduce an `OutputFormat` enum (`Default`, `Tap`) used by both the CLI flag (`--output-format <FORMAT>`) and a new justfile setting (`set output-format := "tap"`). The CLI stores `Option<OutputFormat>` so "not specified" can fall through to the justfile setting. The justfile setting uses the expression-based pattern (like `set dotenv-filename`), validated at evaluation time.

**Tech Stack:** Rust, clap (ValueEnum), strum (EnumString)

---

### Task 1: Create OutputFormat enum

**Files:**
- Create: `src/output_format.rs`
- Modify: `src/lib.rs`

**Step 1: Create `src/output_format.rs`**

```rust
use super::*;

#[derive(Debug, Default, PartialEq, Clone, Copy, ValueEnum, EnumString)]
#[strum(serialize_all = "kebab-case")]
pub(crate) enum OutputFormat {
  #[default]
  Default,
  Tap,
}
```

`ValueEnum` enables clap CLI parsing. `EnumString` enables `"tap".parse::<OutputFormat>()` for the justfile setting evaluation.

**Step 2: Register module and import in `src/lib.rs`**

Add to the module declarations (near line 285, next to `mod tap_output;`):
```rust
mod output_format;
```

Add to the `use` block (near line 96, next to `tap_output::{TapTestResult, TapWriter}`):
```rust
output_format::OutputFormat,
```

**Step 3: Verify it compiles**

Run: `cargo check`

**Step 4: Commit**

```
feat: add OutputFormat enum with Default and Tap variants
```

---

### Task 2: Replace `tap: bool` with `output_format: Option<OutputFormat>` in Config

**Files:**
- Modify: `src/config.rs` (lines 43, 129, 413-419, 869)
- Modify: `completions/just.bash` (line 33)
- Modify: `completions/just.zsh` (line 67)
- Modify: `completions/just.fish` (line 71)
- Modify: `completions/just.elvish` (line 69)
- Modify: `completions/just.powershell` (line 72)

**Step 1: Update Config struct field**

Change line 43 from:
```rust
  pub(crate) tap: bool,
```
to:
```rust
  pub(crate) output_format: Option<OutputFormat>,
```

**Step 2: Replace arg constant**

Change the `TAP` constant at line 129 from:
```rust
  pub(crate) const TAP: &str = "TAP";
```
to:
```rust
  pub(crate) const OUTPUT_FORMAT: &str = "OUTPUT-FORMAT";
```

**Step 3: Replace the `--tap` Arg definition**

Replace lines 413-419:
```rust
      .arg(
        Arg::new(arg::TAP)
          .long("tap")
          .env("JUST_TAP")
          .action(ArgAction::SetTrue)
          .help("Format recipe execution results as TAP version 14 output"),
      )
```
with:
```rust
      .arg(
        Arg::new(arg::OUTPUT_FORMAT)
          .long("output-format")
          .env("JUST_OUTPUT_FORMAT")
          .action(ArgAction::Set)
          .value_parser(clap::value_parser!(OutputFormat))
          .value_name("FORMAT")
          .help("Set output format (default, tap)"),
      )
```

**Step 4: Update parsing**

Change line 869 from:
```rust
      tap: matches.get_flag(arg::TAP),
```
to:
```rust
      output_format: matches.get_one::<OutputFormat>(arg::OUTPUT_FORMAT).copied(),
```

**Step 5: Update completion files**

In each completion file, replace `--tap` references with `--output-format`:

- `completions/just.bash` line 33: replace `--tap` with `--output-format`
- `completions/just.zsh` line 67: replace `'--tap[Format recipe execution results as TAP version 14 output]' \` with `'--output-format[Set output format (default, tap)]' \`
- `completions/just.fish` line 71: replace `complete -c just -l tap -d 'Format recipe execution results as TAP version 14 output'` with `complete -c just -l output-format -d 'Set output format (default, tap)'`
- `completions/just.elvish` line 69: replace `cand --tap 'Format recipe execution results as TAP version 14 output'` with `cand --output-format 'Set output format (default, tap)'`
- `completions/just.powershell` line 72: replace the `--tap` completion with `--output-format`

**Step 6: Verify it compiles (will have errors in justfile.rs and recipe.rs, that's expected)**

Run: `cargo check 2>&1 | head -30`

Expect: errors about `config.tap` no longer existing.

**Step 7: Commit**

```
refactor: replace --tap flag with --output-format enum in Config
```

---

### Task 3: Update justfile.rs and recipe.rs to use `config.output_format`

**Files:**
- Modify: `src/justfile.rs` (line 211)
- Modify: `src/recipe.rs` (lines 216, 220, 290, 331, 439, 457)

All references to `config.tap` become checks against the output format. Since Config currently stores `Option<OutputFormat>` (justfile setting merging comes later), use a helper or inline check. For now, treat `None` as `Default`.

**Step 1: Update `justfile.rs` TAP check**

Change line 211 from:
```rust
    if config.tap {
      return self.run_tap(config, &dotenv, &scopes, search, invocations);
    }
```
to:
```rust
    if config.output_format == Some(OutputFormat::Tap) {
      return self.run_tap(config, &dotenv, &scopes, search, invocations);
    }
```

**Step 2: Update all `config.tap` references in `recipe.rs`**

Replace every `config.tap` with `config.output_format == Some(OutputFormat::Tap)`:

Line 216: `!context.config.tap` -> `context.config.output_format != Some(OutputFormat::Tap)`
Line 220: `!context.config.tap` -> `context.config.output_format != Some(OutputFormat::Tap)`
Line 290: `!config.tap` -> `config.output_format != Some(OutputFormat::Tap)`
Line 331: `config.tap` -> `config.output_format == Some(OutputFormat::Tap)`
Line 439: `!config.tap` -> `config.output_format != Some(OutputFormat::Tap)`
Line 457: `!config.tap` -> `config.output_format != Some(OutputFormat::Tap)`

**Step 3: Verify it compiles**

Run: `cargo check`

**Step 4: Commit**

```
refactor: update justfile.rs and recipe.rs to use output_format enum
```

---

### Task 4: Add `set output-format` justfile setting

**Files:**
- Modify: `src/keyword.rs` (line 3, add variant)
- Modify: `src/setting.rs` (line 3, add variant)
- Modify: `src/settings.rs` (line 8, add field)
- Modify: `src/parser.rs` (line 1340, add match arm)
- Modify: `src/evaluator.rs` (line 84, add match arm)

**Step 1: Add `OutputFormat` to Keyword enum in `src/keyword.rs`**

Add between `NoExitMessage` and `PositionalArguments` (alphabetical within kebab-case):
```rust
  OutputFormat,
```

**Step 2: Add `OutputFormat` variant to Setting enum in `src/setting.rs`**

Add after `NoExitMessage(bool)`:
```rust
  OutputFormat(Expression<'src>),
```

**Step 3: Add `output_format` field to Settings struct in `src/settings.rs`**

Add after `no_exit_message: bool,`:
```rust
  pub(crate) output_format: Option<crate::output_format::OutputFormat>,
```

**Step 4: Add parser match arm in `src/parser.rs`**

In the expression-based match block (after the `Keyword::DotenvPath` arm, around line 1340), add:
```rust
      Keyword::OutputFormat => Some(Setting::OutputFormat(self.parse_expression()?)),
```

**Step 5: Add evaluator match arm in `src/evaluator.rs`**

Add a new arm in `evaluate_sets` (after the `Setting::NoExitMessage` arm):
```rust
        Setting::OutputFormat(value) => {
          let value = self.evaluate_expression(&value)?;
          settings.output_format = Some(
            value
              .parse::<crate::output_format::OutputFormat>()
              .map_err(|_| Error::FormatUnknown {
                format: value.clone(),
                setting: "output-format".into(),
              })?,
          );
        }
```

**Step 6: Add `FormatUnknown` error variant**

In `src/error.rs`, add a new variant to the `Error` enum (near `TapFailure`):
```rust
  FormatUnknown {
    format: String,
    setting: String,
  },
```

Add the display arm (near the `TapFailure` display):
```rust
      FormatUnknown { format, setting } => {
        write!(f, "Unknown {setting} value: \"{format}\"")?;
      }
```

**Step 7: Verify it compiles**

Run: `cargo check`

**Step 8: Commit**

```
feat: add `set output-format` justfile setting
```

---

### Task 5: Merge CLI and justfile setting with CLI precedence

**Files:**
- Modify: `src/justfile.rs` (line 211)

**Step 1: Update the TAP check to merge both sources**

Change the check from:
```rust
    if config.output_format == Some(OutputFormat::Tap) {
```
to:
```rust
    let output_format = config
      .output_format
      .or(self.settings.output_format)
      .unwrap_or_default();

    if output_format == OutputFormat::Tap {
```

This gives CLI precedence: if the user passed `--output-format`, use it; otherwise fall back to the justfile setting; otherwise use `Default`.

**Step 2: Verify it compiles**

Run: `cargo check`

**Step 3: Commit**

```
feat: merge CLI and justfile output-format with CLI precedence
```

---

### Task 6: Update tests

**Files:**
- Modify: `tests/tap.rs`

**Step 1: Update all `--tap` args to `--output-format tap`**

Replace every `.arg("--tap")` with `.args(["--output-format", "tap"])`.

**Step 2: Update `tap_with_env_var` test**

Change `.env("JUST_TAP", "true")` to `.env("JUST_OUTPUT_FORMAT", "tap")`.

**Step 3: Add test for `set output-format := "tap"` in justfile**

```rust
#[test]
fn output_format_justfile_setting() {
  Test::new()
    .justfile(
      "
      set output-format := \"tap\"

      build:
        echo hello
      ",
    )
    .arg("build")
    .stdout_regex("TAP version 14\n1\\.\\.1\nok 1 - build\n  ---\n  output: \\|\n    hello\n  \\.\\.\\.\n")
    .stderr("")
    .success();
}
```

**Step 4: Add test for CLI overriding justfile setting**

```rust
#[test]
fn output_format_cli_overrides_justfile() {
  Test::new()
    .justfile(
      "
      set output-format := \"tap\"

      build:
        echo hello
      ",
    )
    .args(["--output-format", "default"])
    .arg("build")
    .stdout("hello\n")
    .stderr("echo hello\n")
    .success();
}
```

**Step 5: Run the TAP tests**

Run: `cargo test --test integration tap -- --test-threads=1`

Expected: all tests pass.

**Step 6: Run the full test suite**

Run: `cargo test --test integration -- --test-threads=4`

Expected: all tests pass (existing tests don't use TAP by default).

**Step 7: Commit**

```
test: update TAP tests for --output-format and add justfile setting tests
```

---

### Task 7: Update config unit tests

**Files:**
- Modify: `src/config.rs` (tests module, near line 910)

**Step 1: Add config parsing test for `--output-format`**

The `test!` macro in config.rs doesn't directly support `output_format`, so we need to verify it compiles and the `testing::config` helper returns the right default. Check if `testing::config` needs updating.

Look at `src/testing.rs` for the `config` function and ensure `output_format: None` is the default (it should be via `Default` derive since it's `Option<OutputFormat>`).

**Step 2: Commit if changes were needed**

```
test: update config unit tests for output-format
```
