# Upstream resync via `--events-fd` externalization

## Problem

just-us is forked from upstream casey/just at v1.46.0 (commit `2eebdbb`)
with ~130 commits adding TAP-14 output and a related MCP-server crate.
Upstream has since released through v1.51.0 (HEAD `50dd0ab`, ~250
commits ahead). The fork is unbuildable as-shipped because its
`tap-dancer` git dep points at `amarbel-llc/bob`, which no longer
exists (the crate moved to `amarbel-llc/tap/rust`).

A direct rebase of the fork onto upstream v1.51.0 hits 17 file-level
content conflicts plus 1 modify/delete plus 2 add/add, concentrated in
the recipe-execution hot path (`recipe.rs`, `justfile.rs`, `config.rs`,
`parser.rs`). The conflicts are non-trivial because both sides
refactored the same files: upstream did sigil/attribute/working-dir
rewrites; the fork added a parallel `output_format`-aware code path in
`Recipe::run`. There is no clean separation to align.

Reading upstream v1.51.0's `recipe.rs` confirmed there is no output
formatter abstraction we could layer onto: `Recipe::run` hardcodes
`cmd.status_guard()` with inherited stdout/stderr, no sink, no
formatter trait. The fork had to thread `tap_output: Option<...>`,
`output_format`, `tap_test_number` through the whole execution path
because that's the only way to intercept child output.

## Pivot

Instead of merging the fork forward, externalize the TAP-14 emission
to a separate tool. Add a small `--events-fd N` feature to upstream
`just` (or carry as a small fork patch if not accepted) that emits
structured execution events on a side fd:

- `plan { recipe_count }`
- `recipe_start { tp, name, namepath, depth, parent, doc, quiet }`
- `recipe_command { tp, command, line }`
- `output { tp, stream, data }`
- `recipe_complete { tp, exit_code, signal, duration_ms }`

When `--events-fd` is set, `just` PTY-spawns children (preserving ANSI
color), captures their stdout/stderr in chunks, emits `output` events
with byte-faithful UTF-8-with-U+FFFD-replacement encoding, and
suppresses passthrough to its own stdout/stderr. The external wrapper
is the sole presenter.

Wire-format details live in `docs/rfcs/0002-just-events-fd-stream.md`
in this PR. That RFC is the contract `just` exposes; it will move to
`amarbel-llc/tap/docs/rfcs/0002-...` once finalized.

## Why this is the right move

1. **The fork delta shrinks from ~5k lines to ~185 lines.** All of it
   in one file (`event.rs`) plus minimal touch-points in `config.rs`,
   `execution_context.rs`, `recipe.rs`, `justfile.rs`. Future resync
   becomes trivial.
2. **The patch is a candidate for upstream merge.** Casey has accepted
   similar small structured-output features (`--dump-format`,
   `--evaluate`). A `--events-fd` flag framed as "machine-readable
   observation of recipe execution" has moderate-to-good odds. Even if
   rejected, the carry cost is minimal.
3. **Presentation lives outside `just` in `amarbel-llc/tap`**, where it
   can iterate on its own release cycle independent of upstream. The
   `tap-just` wrapper joins `tap-dancer`'s existing tooling (`go-test`,
   `cargo-test`, `exec`, `format-ndjson`) as another converter.
4. **Multiple consumers are possible** from a single event stream
   (TAP-14, structured logs, CI dashboards, build telemetry, AI agent
   observability). Today each requires its own intrusive fork.

## What gets removed from the fork

- `src/output_format.rs` — entire file
- `OutputFormat`-aware branches in `recipe.rs` and `justfile.rs`
- `set output-format` justfile setting
- `--output-format` / `--tap` / `--tap-stream` CLI flags
- `just-me` binary alias
- The `[agents()]` recipe attribute and `just-us-agents` MCP server
  (already removed in the fork's most recent commit, included here for
  completeness)
- `crates/generate-man` — likely retained or upstreamed separately
- Plan docs under `docs/plans/2026-0[2-3]-*` for the abandoned
  approach (kept in git history)

## What gets added to the fork

- A single commit on top of upstream v1.51.0 implementing the
  `--events-fd` patch (~185 lines). Specifically:
  - `src/event.rs` (~80 LOC): `Event` enum + `EventSink` wrapping
    `Option<Mutex<Box<dyn Write + Send>>>`
  - `src/config.rs` (~15 LOC): `--events-fd N` flag + config field
  - `src/execution_context.rs` (~5 LOC): thread `EventSink` reference
  - `src/recipe.rs` (~80 LOC): when `events_fd` set, replace
    `cmd.status_guard()` path with PTY-allocate + chunk-capture +
    emit `output` events. Otherwise no behavior change.
  - `src/justfile.rs` (~5 LOC): emit `plan` event in the dep-graph
    pre-walk.
- `docs/rfcs/0002-just-events-fd-stream.md` — the wire-format RFC.

## Trade-offs vs the current fork

| Capability | Current fork | After |
|---|---|---|
| Live streaming output | Yes (in-process) | Yes (via event stream) |
| ANSI color preservation | Yes (PTY + YAML) | Yes (PTY + `output` events) |
| TAP-14 subtest detection of recipe-emitted TAP | Yes (in-process scan) | Yes (wrapper scans `output` data per `tp`) |
| Test points for unrun recipes on dep failure | Yes | Yes (wrapper infers from missing `recipe_complete`) |
| Recipe doc comments as test point comments | Yes | Yes (`doc` field on `recipe_start`) |
| YAML output blocks | Yes (in-process) | Yes (wrapper assembles from `output` events) |
| Parallel recipe execution | Yes | Yes (wrapper handles interleaved `tp`-tagged events) |
| `just-me` argv[0] aliasing | Yes | No (wrapper invocation: `tap-just <recipe>`) |
| Output goes directly to terminal when interactive | Yes | No (wrapper is sole presenter; mid-loss flagged in RFC) |

## Phases

Each phase is a separate PR, landed in order.

### Phase 0 — Build fix (urgent, can land independently)

- Repoint `tap-dancer` git URL from `amarbel-llc/bob` to
  `amarbel-llc/tap` in `Cargo.toml`. Single-commit PR. Restores the
  fork's buildability while the larger resync is pending.

### Phase 1 — Plan review (this PR)

- `docs/rfcs/0002-just-events-fd-stream.md` (the wire-format RFC, to
  be moved to `amarbel-llc/tap` once finalized)
- `docs/plans/2026-05-24-upstream-resync-events-fd.md` (this document)
- No code changes. Review forum for the design before implementation.

### Phase 2 — Upstream patch

- Reset `master` to upstream `casey/just` v1.51.0 (`50dd0ab`),
  preserving the current state under tag `pre-events-fd-resync` for
  recovery.
- Apply the `--events-fd` patch as a single commit.
- Add bats tests for the wire-format contract under `zz-tests_bats/`.
- Open as draft PR; submit upstream PR to `casey/just` in parallel.

### Phase 3 — Wrapper

- Build `tap-just` in `amarbel-llc/tap/go/cmd/tap-just` (or
  `rust/src/bin/tap-just` if preferred), consuming the event stream
  and emitting TAP-14 with the same fidelity the in-process fork
  provided.
- Document `just ... | tap-just | tap-dancer format-ndjson` as the
  canonical agent-consumable pipeline.

### Phase 4 — Cleanup

- Once `tap-just` is functional, retire the fork's plan docs under
  `docs/plans/2026-0[2-3]-*` (they describe the abandoned approach;
  git history preserves them).
- Optional: rename `just-us` → `just` if the fork goes to ~0 delta and
  becomes just a Nix-flake + tooling wrapper. Out of scope here.

## Recovery / rollback

- Tag `pre-resync-backup` exists on the current pre-squash fork tip.
  `git reset --hard pre-resync-backup` restores the pre-pivot world.
- Tag `pre-rebase-backup` exists on the squashed-but-not-rebased state.
- This PR makes no destructive changes. Phase 2 is the first
  destructive operation; that PR will tag again before resetting.

## Open questions

- Should `--events-fd` also emit events for non-recipe operations
  (parse errors, dotenv loads, completion subcommand activity)? RFC
  draft scopes to recipe execution only; consumers wanting full
  observability would need a v2 extension. Recommend: keep narrow for
  v1, add later if needed.
- Branch-name convention for `tap-just`: live in `tap` repo's `rust/`
  or `go/`? Go has the existing `tap-dancer` ecosystem; Rust has
  better PTY libraries. Recommend: Go side for consistency.
- If upstream rejects `--events-fd`, do we maintain it as a long-term
  carry or upstream a smaller variant (e.g., `--json-events`
  stdout-mode without PTY capture)? Defer until upstream feedback.

## References

- RFC 0002 (this PR): `docs/rfcs/0002-just-events-fd-stream.md`
- RFC 0001 (in `amarbel-llc/tap`): TAP Test-Result NDJSON Schema
- Upstream v1.51.0: <https://github.com/casey/just/tree/50dd0ab>
- Existing fork plan docs: `docs/plans/2026-0[2-3]-*` (superseded
  by this plan)
