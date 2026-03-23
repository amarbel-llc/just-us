# Recursive Subtest Fixes Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** Fix two bugs in TAP-14 subtest output: (1) streamed mode flattens
nested indentation, (2) `Bail out!` in subtest produces invalid plan count.

**Architecture:** Bug 1 is a one-line fix in `tap_stream_sink` --- replace
`line.trim()` with `line.trim_end()` so leading whitespace (nested subtest
indentation) is preserved. Bug 2 requires the buffered subtest wrapper to detect
`Bail out!` and adjust the inner plan accordingly. Both bugs have failing bats
tests already committed.

**Tech Stack:** Rust, tap-dancer crate

**Rollback:** Revert the commits on this branch. No existing behavior changes
for non-subtest output.

--------------------------------------------------------------------------------

### Task 1: Fix streamed mode indentation flattening

**Files:** - Modify: `src/recipe.rs:14-16`

**Step 1: Fix the trim call**

In `tap_stream_sink`, line 15 calls `line.trim()` which strips leading
whitespace --- destroying nested subtest indentation. The buffered path
(`src/justfile.rs:502`) correctly uses `trim_end_matches('\r')`. Apply the same
approach here, but we also need to filter empty lines (lines that are
whitespace-only). Split the trim into two operations: `trim_end()` for the
output value, `trim()` only for the emptiness check.

Replace lines 14-16 of `src/recipe.rs`:

``` rust
      let line = String::from_utf8_lossy(&buf[..pos]);
      let line = line.trim();
      if !line.is_empty() {
```

With:

``` rust
      let line = String::from_utf8_lossy(&buf[..pos]);
      let line = line.trim_end();
      if !line.trim().is_empty() {
```

This preserves leading whitespace (subtest indentation) while still filtering
blank lines.

**Step 2: Run the failing streamed tests**

Run:
`JUST_ME_BIN=target/debug/just-me bats zz-tests_bats/tap14_output.bats --filter "streamed_recursive|streamed_triple|streamed_subtest_with_yaml"`
Expected: all 4 tests PASS

**Step 3: Run all TAP bats tests for regressions**

Run: `JUST_ME_BIN=target/debug/just-me bats zz-tests_bats/tap14_output.bats`
Expected: only `buffered_subtest_with_bail_out` still fails (Task 2's bug)

**Step 4: Commit**

    git add src/recipe.rs
    git commit -m "fix: preserve nested indentation in streamed TAP subtest output"

--------------------------------------------------------------------------------

### Task 2: Fix `Bail out!` producing invalid subtest plan

**Files:** - Modify: `src/justfile.rs:520-530`

**Step 1: Understand the problem**

When a recipe outputs TAP containing `Bail out!`, the subtest wrapper blindly
re-emits all lines including the original plan line (e.g. `1..3`). But only 1
test ran before bail. The inner subtest is invalid because plan says 3 but only
1 ran.

The fix: when wrapping subtest output, detect `Bail out!` and stop emitting
lines after it. The plan line from the child is already emitted, and `Bail out!`
is a legal way to end a TAP document early --- tap-dancer should accept it. The
key is that lines after `Bail out!` must not be emitted.

However, looking more carefully: the child process itself only emits 1 test
point + `Bail out!` and then exits. The plan `1..3` is already in the output.
According to the TAP spec, `Bail out!` is valid and overrides the plan --- a TAP
consumer MUST accept early termination via bail. The issue may be in
tap-dancer's validation. Let's first check what tap-dancer does.

Run:
`echo 'TAP version 14\n1..1\n    # Subtest: test\n    TAP version 14\n    1..3\n    ok 1 - first\n    Bail out! disk full\nok 1 - test' | tap-dancer validate`

If tap-dancer rejects this, the fix belongs in tap-dancer (accepting bail as
valid early termination). If tap-dancer accepts it, the bug is in how just-us
wraps the output.

**Step 2: Determine fix location based on Step 1**

**If tap-dancer rejects valid bail-out subtests:** The fix belongs in tap-dancer
--- it should treat `Bail out!` as valid early plan termination within subtests.
File an issue or fix tap-dancer directly.

**If tap-dancer accepts it but just-us mangles the output:** Fix the wrapping in
`src/justfile.rs`.

**Step 3: Implement the fix**

This step depends on Step 2's findings. The implementer should investigate and
fix accordingly.

**Step 4: Run the failing test**

Run:
`JUST_ME_BIN=target/debug/just-me bats zz-tests_bats/tap14_output.bats --filter "bail_out"`
Expected: PASS

**Step 5: Run full suite**

Run: `JUST_ME_BIN=target/debug/just-me bats zz-tests_bats/tap14_output.bats`
Expected: all 45 tests PASS

**Step 6: Commit**

    git add <changed files>
    git commit -m "fix: handle Bail out! in TAP subtest output"

--------------------------------------------------------------------------------

### Task 3: Run full test suite

**Step 1: Run all bats tests**

Run: `JUST_ME_BIN=target/debug/just-me bats zz-tests_bats/tap14_output.bats`
Expected: 45/45 pass

**Step 2: Run cargo tests for regressions**

Run: `cargo test --test integration tap` Expected: all TAP integration tests
pass

**Step 3: Run clippy**

Run: `cargo clippy --all-targets` Expected: no errors
