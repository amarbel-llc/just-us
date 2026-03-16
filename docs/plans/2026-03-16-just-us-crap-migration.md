# just-us: Migrate from tap-dancer to rust-crap

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** Replace tap-dancer dependency with rust-crap in just-us, removing inline TTY helpers that are now provided by rust-crap.

**Architecture:** Mechanical dependency swap + deletion of ~40 lines of inline TTY helpers replaced by `feed_status_bytes()`. The rust-crap API mirrors tap-dancer's with renames (`Tap*` → `Crap*`, `tty_build_last_line` → `status_line`).

**Tech Stack:** Rust, rust-crap (git dep from github.com/amarbel-llc/crap)

**Rollback:** Revert the Cargo.toml change + `git checkout src/justfile.rs src/recipe.rs`

**Reference:** See `docs/migration-from-tap-dancer.md` in the crap repo for the full API mapping.

---

### Task 1: Update Cargo.toml dependency

**Files:**
- Modify: `Cargo.toml`

**Step 1: Replace tap-dancer with rust-crap**

Find the line:
```toml
tap-dancer = { git = "https://github.com/amarbel-llc/bob" }
```

Replace with:
```toml
rust-crap = { git = "https://github.com/amarbel-llc/crap", rev = "579d90e" }
```

Note: Use the `rev` pin to the current HEAD of the `fair-cypress` branch. The package is in `rust-crap/` subdirectory of the repo, so you may need to verify the path resolves. If cargo can't find the crate, try:
```toml
rust-crap = { git = "https://github.com/amarbel-llc/crap" }
```

**Step 2: Run cargo check to verify the dependency resolves**

Run: `cargo check 2>&1 | head -20`
Expected: Compilation errors about `tap_dancer` not found (expected — we haven't updated imports yet). The dependency itself should resolve.

**Step 3: Commit**

```
feat: replace tap-dancer dependency with rust-crap
```

---

### Task 2: Rename all tap-dancer references in justfile.rs

**Files:**
- Modify: `src/justfile.rs`

**Step 1: Find all tap_dancer references**

Search `src/justfile.rs` for `tap_dancer`. There should be ~8 occurrences.

**Step 2: Apply renames**

| Before | After |
|---|---|
| `tap_dancer::TapWriterBuilder` | `rust_crap::CrapWriterBuilder` |
| `tap_dancer::TestResult` | `rust_crap::TestResult` |
| `.tty_build_last_line(...)` | `.status_line(...)` |

Do NOT change:
- `TestResult` field names (they're identical)
- `.build()`, `.build_without_printing()`, `.plan_ahead()`, `.test_point()` (same names)
- `.color()`, `.default_locale()` (same names)

**Step 3: Remove manual status line clear before test_point**

In `justfile.rs`, find the block that looks like:
```rust
if output_format == OutputFormat::TapStreamedOutput {
    write!(stdout, "\r\x1b[2K").map_err(|io_error| Error::StdoutIo { io_error })?;
    stdout.flush().map_err(|io_error| Error::StdoutIo { io_error })?;
}
```

This appears just before `writer.test_point(&test_result)`. Delete it — rust-crap auto-clears status lines before test points.

**Step 4: Verify it compiles**

Run: `cargo check`
Expected: May still have errors in `recipe.rs` if it references `tap_dancer` — that's Task 3.

**Step 5: Commit**

```
refactor: rename tap-dancer types to rust-crap in justfile.rs
```

---

### Task 3: Replace inline TTY helpers in recipe.rs with feed_status_bytes

**Files:**
- Modify: `src/recipe.rs`

This is the most impactful change. `recipe.rs` has two nearly-identical blocks of inline PTY streaming code (one in `run_linewise` ~line 519-545, one in `run_script` ~line 760-785). Both will be replaced with `feed_status_bytes`.

**Step 1: Delete `has_visible_content` function**

Remove the `has_visible_content` function at the top of `recipe.rs` (approximately lines 3-23). This function is now provided by `rust_crap::has_visible_content` (though we won't need to import it — `feed_status_bytes` uses it internally).

**Step 2: Replace the streaming callback in run_linewise**

Find the `OutputFormat::TapStreamedOutput` match arm in `run_linewise` (around line 522). It currently looks like:

```rust
OutputFormat::TapStreamedOutput => {
    use std::io::IsTerminal;
    let stdout_lock = io::stdout();
    let is_tty = stdout_lock.is_terminal();
    let line_buf = Mutex::new(Vec::<u8>::new());
    stream_command_output(cmd, &|chunk| {
        let mut buf = line_buf.lock().unwrap();
        buf.extend_from_slice(chunk);
        let mut stdout = stdout_lock.lock();
        while let Some(pos) = buf.iter().position(|&b| b == b'\n' || b == b'\r') {
            let line = String::from_utf8_lossy(&buf[..pos]);
            let line = line.trim();
            if has_visible_content(line) {
                if is_tty {
                    write!(stdout, "\r\x1b[2K\x1b[?7l# {line}\x1b[?7h")?;
                } else {
                    write!(stdout, "\r\x1b[2K# {line}")?;
                }
                stdout.flush()?;
            }
            buf.drain(..=pos);
        }
        Ok(())
    })
}
```

**Important design consideration:** `feed_status_bytes` requires a `&mut CrapWriter`, but the streaming callback is a closure that runs during command execution — before the test result is known. The current architecture creates a CrapWriter at two points:

1. In `justfile.rs` at the start of `run_tap()` for version/plan emission
2. In `justfile.rs` after each recipe completes for test point emission

Neither writer is available inside `recipe.rs`'s streaming callback. There are two approaches:

**Approach A (recommended): Pass a CrapWriter into the recipe execution**

This requires threading a `&mut CrapWriter` (or a shared reference) through `run_recipe` → `run_linewise`/`run_script`. This is a larger refactor but produces the cleanest result.

**Approach B (minimal): Use StatusLineProcessor directly**

Replace the inline logic with `StatusLineProcessor` but keep the manual `write!` calls. This is a smaller change:

```rust
OutputFormat::TapStreamedOutput => {
    use std::io::IsTerminal;
    let stdout_lock = io::stdout();
    let is_tty = stdout_lock.is_terminal();
    let proc = Mutex::new(rust_crap::StatusLineProcessor::new());
    stream_command_output(cmd, &|chunk| {
        let mut proc = proc.lock().unwrap();
        let mut stdout = stdout_lock.lock();
        for line in proc.feed(chunk) {
            if is_tty {
                write!(stdout, "\r\x1b[2K\x1b[?7l# {line}\x1b[?7h")?;
            } else {
                write!(stdout, "\r\x1b[2K# {line}")?;
            }
            stdout.flush()?;
        }
        Ok(())
    })
}
```

**Use Approach B for now.** It validates the `StatusLineProcessor` API without requiring architectural changes to how CrapWriter flows through just-us. A follow-up can thread CrapWriter through for `feed_status_bytes`.

**Step 3: Apply the same replacement in run_script**

Find the second identical streaming block in `run_script` (around line 763-785) and apply the same Approach B replacement.

**Step 4: Clean up unused imports**

After removing `has_visible_content`, check if any imports are now unused (the function may have been the only consumer of certain `use` items).

**Step 5: Verify it compiles**

Run: `cargo check`
Expected: Clean compilation.

**Step 6: Commit**

```
refactor: replace inline TTY helpers with rust-crap StatusLineProcessor
```

---

### Task 4: Update integration tests

**Files:**
- Modify: `tests/tap.rs`

**Step 1: Find all TAP version assertions**

Search `tests/tap.rs` for:
- `"TAP version 14"` — replace with `"CRAP version 2"`
- `"tty-build-last-line"` — replace with `"status-line"`

**Step 2: Apply replacements**

This should be a mechanical find-and-replace. There are approximately 56 tests;
many will contain version string assertions.

**Step 3: Run the tests**

Run: `cargo test`
Expected: All tests pass. If any fail, investigate — the output format change
from TAP to CRAP may surface edge cases in test assertions that check exact
output rather than `contains()`.

**Step 4: Commit**

```
test: update assertions for CRAP version 2 output format
```

---

### Task 5: Verify run_linewise streaming with has_visible_content

**Step 1: Check the second streaming block**

In `run_script` (around line 775), the original code used `!line.is_empty()`
instead of `has_visible_content(line)`. The `StatusLineProcessor` already
filters via `has_visible_content`, so this is now handled. Verify that both
streaming blocks use the same `StatusLineProcessor`-based approach.

**Step 2: Run the full test suite**

Run: `cargo test`
Expected: All tests pass.

**Step 3: Manual smoke test**

Run just-us against a justfile with a recipe that produces output:

```
echo 'default:\n\techo hello' | cargo run -- --output-format tap
```

Verify output shows `CRAP version 2` header and `ok 1` result.

**Step 4: Commit (if any fixes needed)**

```
fix: align both streaming paths to use StatusLineProcessor
```

---

### Task 6: Clean up and final verification

**Step 1: Run clippy**

Run: `cargo clippy`
Expected: No new warnings.

**Step 2: Run fmt**

Run: `cargo fmt`

**Step 3: Run full test suite one more time**

Run: `cargo test`

**Step 4: Final commit if needed**

```
chore: clean up after tap-dancer to rust-crap migration
```
