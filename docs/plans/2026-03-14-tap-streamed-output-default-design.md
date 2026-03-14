# Design: TAP+streamed_output as default output format

## Problem

The fork supports TAP-14 output with streamed output, but it defaults to
plain output. Previous attempts to switch the default failed due to a bug:
empty lines in recipe output produced bare `# ` comment lines, creating
visual noise between test points.

## Changes

### 1. Bug fix: elide empty streamed output lines

In `recipe.rs`, both `run_linewise()` and `run_script()` streamed output
callbacks emit `# {line}` for every line including empty ones. Fix: skip
lines where the content after `\r`-stripping is empty.

### 2. Merge output-format and tap-stream into one field

Replace separate `OutputFormat` + `TapStream` enums with a single
`OutputFormat` enum using pandoc-style `+` delimiter:

- `tap+streamed_output` (new default)
- `default` (plain output)
- `tap` (buffered TAP)
- `tap+stderr` (TAP with stderr streaming)

Remove entirely:
- `src/tap_stream.rs`
- `--tap-stream` CLI flag
- `JUST_TAP_STREAM` env var
- `set tap-stream` justfile setting / keyword
- `tap_stream` field from `Config` and `Settings`

### 3. Switch default to tap+streamed_output

Change `#[default]` on `OutputFormat` from `Default` to `TapStreamedOutput`.

### 4. Test strategy

**Integration tests** (tests/test.rs):
- Add `output_format: Option<String>` field to `Test` struct
- Default to `Some("default")` so mainline tests see plain output
- Auto-inject `--output-format <value>` in `Test::status()` (same pattern
  as `--shell bash`)
- TAP tests in `tests/tap.rs` explicitly set their desired format

**Unit tests** (src/testing.rs):
- `run_error!` macro checks `Error` variants, not stdout — should be
  unaffected
- If any unit tests check stdout content, add `--output-format default`
  to their args

### 5. Rollback

- Revert: change `#[default]` back to `Default` (one-line change)
- Per-invocation opt-out: `--output-format default` or
  `JUST_OUTPUT_FORMAT=default`
