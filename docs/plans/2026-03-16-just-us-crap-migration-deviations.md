# Migration Deviations: just-us tap-dancer → rust-crap

Deviations encountered while executing
`docs/plans/2026-03-16-just-us-crap-migration.md`.

---

### 1. Rev pin `579d90e` does not exist

**Plan (Task 1 Step 1):** Use `rev = "579d90e"` to pin to `fair-cypress` HEAD.

**Actual:** The rev was not found. Used the plan's fallback (no rev pin).
`cargo update -p rust-crap` was later needed to pick up `StatusLineProcessor`.

**Upstream action:** Remove the stale rev pin from the plan, or document which
branch/tag it refers to.

---

### 2. YAML output format differs: single-line values use quoted scalars

**Plan (Task 4):** Only mentioned replacing `"TAP version 14"` →
`"CRAP version 2"` and `"tty-build-last-line"` → `"status-line"`.

**Actual:** rust-crap's `write_yaml_field` uses `"value"` (quoted scalar) for
single-line output and `|` (block scalar) for multi-line. tap-dancer used block
scalar for all output. This required updating ~20 additional test assertions
that expected `output: |\n    hello` to instead expect `output: "hello"`.

**Upstream action:** Document this YAML formatting difference in
`docs/migration-from-tap-dancer.md`. Consumers with test suites checking exact
YAML output will hit this.

---

### 3. `build_without_printing()` does not auto-clear status lines

**Plan (Task 2 Step 3):** Delete the manual `\r\x1b[2K` clear before
`test_point()` because "rust-crap auto-clears status lines before test points."

**Actual:** `test_point()` calls `clear_status_if_active()`, but
`build_without_printing()` initializes `status_line_active: false`. When the
CrapWriter is created fresh for each test point (not threaded through recipe
execution), it has no knowledge of an active status line, so the auto-clear
never fires. The manual clear had to be restored.

**Upstream action:** Either:
- Document that auto-clear only works when the same CrapWriter instance is used
  for both `feed_status_bytes`/status writes and `test_point` calls, or
- Add a `CrapWriterBuilder::status_line_active(true)` option so callers using
  `build_without_printing()` can indicate an external status line is active, or
- Correct the migration guide to not recommend removing the manual clear in
  split-writer architectures.
