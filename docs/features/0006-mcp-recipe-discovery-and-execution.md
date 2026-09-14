---
status: proposed
date: 2026-09-10
promotion-criteria:
---

# MCP recipe discovery and execution

## Problem Statement

FDR 0005 gave just-us's `just --mcp` server exactly one capability: a
static `prompts/get` roster (name + doc line) injected into the agent
system prompt at launch. That is read-only discovery with no way to
actually *run* anything — an agent still has to fall back to the
`just-us-agents` moxy moxin (a wrapper maintained outside this repo) or a
raw shell `just <recipe>` to execute a recipe. This FDR implements the
`tools` facet of FDR 0004 (list/show/run recipes over MCP, plus
variable/dump parity with the moxin) on the same `just --mcp` server. FUSE
and MCP-based recipe *editing* (FDR 0004's other two facets) stay out of
scope here.

## Interface

The same stdio MCP server FDR 0005 introduced now also advertises the
`tools` capability (`initialize.capabilities.tools = {}`, alongside
`prompts`) and answers `tools/list` / `tools/call`.

- **`list_recipes { verbose?: bool, max_depth?: number }`** — every
  public recipe (`ModelRecipe::private == false`, same visibility
  contract as the system-prompt roster and `--list`). **Compact by
  default**: `{namepath, doc, parameters, groups}` per recipe — a large
  multi-justfile repo (hundreds of recipes) blows past the MCP
  inline-result size limit with the full model, spilling to a file the
  caller then has to `jq`/`grep` instead of just reading a list.
  `verbose: true` returns the full FDR 0003 model per recipe instead
  (`doc_prelude`, `dependencies`, `source`, `line`, ...).
  Also walks the repo tree for other, separate justfiles
  (`find`-equivalent: depth 2 through `max_depth` — default `3` —
  below the server's working directory, pruning
  `.git`/`.worktrees`/`.claude` — parity with the retired
  `just-us-agents` moxin's own `list-recipes` script, with the depth
  limit now overridable for repos nested deeper than the moxin ever
  handled) and includes their public recipes too, each `namepath`
  prefixed `"<relative-dir>/"` (e.g. `services/foo/build`) — `/`, not
  `::`, since these are wholly separate justfiles, not `mod`-imports of
  the one the server started with. A child justfile that fails to
  compile is silently skipped, not surfaced as an error: this is
  best-effort discovery, not a guarantee every justfile in the repo is
  valid.
- **`show_recipe { recipe: string, max_depth?: number }`** — the same
  full model entry for one namepath, always full detail regardless of
  `list_recipes`'s compact/verbose split (a single named lookup has no
  output-size problem). Resolves recipes in other justfiles too — the
  same `dir/recipe` namepath `list_recipes` reports, subject to the same
  `max_depth` walk (also overridable here, in case the target recipe is
  nested deeper than the default reaches). Also `!private`-gated, for
  consistency: nothing reachable through `list_recipes` or the
  system-prompt roster is separately reachable by naming it directly.
  Unknown or private name → a tool result with `isError: true`, not a
  JSON-RPC protocol error.
- **`run_recipe { recipe: string, args?: string[], impure?: bool, timeout?: string, async?: bool }`**
  — runs the recipe as a real subprocess. `args` are positional, in
  declared-parameter order — `just` has no named-argument CLI syntax, so
  there is no richer mapping to invent. `impure`/`timeout`/`async`
  restore parity with the retired `just-us-agents` moxin's devshell
  wrapping and add a real background/timeout story; see "Execution
  model" below. Returns the recipe's captured stdout/stderr as text
  content, and sets `isError: true` (with the formatted error appended
  as an extra text block) when the recipe fails, is unknown, or times
  out.
- **`dump_justfile`** (no input) — the full compiled justfile, serialized
  the same way `just --dump --dump-format json` does (`Justfile`'s own
  `Serialize` impl, unfiltered — this is the raw AST-level dump, not the
  `list_recipes`/`show_recipe` model projection).
- **`list_variables`** (no input) — every public top-level variable as
  `{name, value}` pairs with values fully resolved, equivalent to
  `just --evaluate`. Backed by a new `Justfile::evaluate_all` (`src/
  justfile.rs`) that reuses the same `evaluation_target`/`evaluate_scopes`
  setup `Subcommand::Evaluate` already does, but collects into a `Vec`
  instead of `println!`-ing — the CLI path prints directly to this
  process's own stdout, which here is the JSON-RPC channel, so it can't
  be called as-is.

Tool-call protocol errors (unknown tool name, missing/malformed
arguments) are JSON-RPC errors (the existing `error()` response shape).
A recipe that *runs* but exits non-zero is a **successful** JSON-RPC
response with `result.isError: true` — consistent with the MCP tools
convention that execution failure is tool-result data, not a transport
error, and it preserves whatever output was captured before the failure.

### Execution model: always a real subprocess

`run_recipe` originally executed in-process via `Justfile::run` (an
`EventSink` capture trick borrowed from `--events-fd`, avoiding a
subprocess re-invocation of `just`). That changed with a cutover contract
from `circus` (the consumer replacing the `just-us-agents` moxin for
production devshell-dependent recipes — e.g. `nixos-rebuild`-style jobs):
devshell parity, real timeouts, and real backgrounding all fundamentally
need a killable, independently-schedulable OS process, which a plain
in-process function call can't safely provide in Rust. `run_recipe` now
**always** spawns a subprocess:

- If a `flake.nix` backs the recipe: spawns
  `nix develop [--impure] <flake_dir> -c just <recipe> <args...>` —
  restores devshell tools (matching the moxin's own conditional wrapping
  and its `JUST_US_AGENTS_IMPURE=1` env-var opt-in, now a real `impure`
  parameter). `<flake_dir>` is passed as an explicit path installable
  (`nix develop <path> -c ...`, not relying on `nix develop`'s own
  cwd-based resolution), because *which* directory backs the devshell is
  resolved independently of where the recipe itself lives — see below.
- Otherwise: re-invokes this same `just` binary directly
  (`std::env::current_exe`) — guarantees the exact version already
  serving this MCP session, no PATH-resolution ambiguity.

Either way, the spawned process's own cwd is always the repo root
(`search.working_directory`), and `recipe` is passed to `just` exactly
as given — including a `dir/` prefix, unstripped. This isn't an
oversight: `just` already has its own native support for a `dir/recipe`
argument (`Positional::from_values` splits it at the last `/` into a
search directory and a recipe name; `Search::justfile` then does `just`'s
ordinary upward search starting from there, setting the recipe's own
working directory). Verified directly — `just a/b/c/recipe`, invoked
from a directory whose only justfile at that path is `a/b/c/justfile`,
finds and runs it entirely unaided. So `run_recipe` for a `dir/recipe`
namepath (the same one `list_recipes`/`show_recipe` report, prefixed by
directory — see above) needs no filesystem walk or depth limit of its
own to get `just` to run the right recipe from the right directory.

What `just`'s own resolution *can't* help with is picking a devshell:
`nix develop` needs an explicit installable before `just` even starts,
so it has no way to defer to `just`'s internal search. `run_recipe`
resolves this with a **bounded, two-candidate fallback** — mirroring
`positional.rs`'s own last-`/` split purely to find the directory a
`dir/` prefix points at, then: that directory's own `flake.nix` if it
has one, else the repo root's, never an arbitrary intermediate ancestor.
This was chosen over an unbounded upward walk after checking it against
every actual child-justfile shape across this fleet's repos: it covers
every case (a child with its own flake.nix, or one with none that's
meant to share the root's) without the failure mode an unbounded walk
introduces — picking up an unrelated intermediate `flake.nix` that
happens to sit between the child and the root but wasn't intended as its
devshell. The result reports both `cwd` (the directory a `dir/` prefix
resolves to — a derived hint matching `just`'s own split, not
independently re-verified against the actual justfile that ends up
running) and `devshell` (the `flake.nix` path that backed it, or `none`)
so a caller can always tell which devshell a recipe ran under — the
original ergonomic gap this design closes (a `dir/recipe` invocation was
silently wrapped in the *caller's* devshell rather than the recipe's
own, or the directory's nearest available one).

`dump_justfile`/`list_variables` are unaffected — they still read the
already-compiled in-process `Compilation`. `list_recipes`/`show_recipe`
also read that in-process `Compilation` for the root justfile, but
additionally compile any *discovered child* justfiles independently
(see below); `run_recipe`'s own execution is the only thing that moved
to subprocess spawning.

**`timeout`** (e.g. `"25m"`, `"90s"`, `"2h"` — single-unit only, no
compound forms like `"1h30m"` in this slice) polls the child and kills
it on expiry, reporting `isError: true` with a "timed out" message and
whatever output was captured before the kill.

**`async: true`** makes `run_recipe` a real **ringmaster job producer**
(RFC-0009/0010/0011 — `code.linenisgreat.com/clown`), not a wrapper
around moxy's `async` (just-us is clown-native and has no access to
moxy's meta-tool). Concretely: `ringmaster start --source just-us --label
<recipe>` allocates the job and returns its id immediately as the tool
result (no waiting for the recipe); the recipe subprocess's stdout/stderr
are redirected **directly to the job's output spool**
(`ringmaster spool-path`) at spawn time, so `ringmaster tail -f` and
moxy's own `async-result` see live output exactly the way they already
do for any other clown job — no polling/buffering code needed, the OS
does the incremental writing; a detached background thread waits for the
subprocess (applying `timeout` the same way as the sync path) and calls
`ringmaster done --state succeeded|failed` on completion, which sends
the wake. Session targeting needs no explicit parameter: `ringmaster
start` resolves the session to wake from `CLOWN_SESSION_ID`, inherited
from the clown stdio-bridge that spawned this process. If the
`ringmaster` binary can't be found at all, `async: true` fails clearly
(`isError`) rather than silently falling back to sync.

The `ringmaster` binary itself is a build-time pin, not a PATH lookup: a
nix-built `just` embeds clown's `ringmaster` package's exact store path
(`RINGMASTER_BIN`, set on the `just` derivation in `flake.nix`, forwarded
by `build.rs` via `option_env!`) — confirmed by Nix's own reference
scanner picking it up into `just`'s runtime closure automatically. A
plain `cargo build` (no `RINGMASTER_BIN` set) falls back to a PATH
lookup, so the dev-loop doesn't need the `clown` flake input.

## Examples

    --> {"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"run_recipe","arguments":{"recipe":"greet","args":["world"]}}}
    <-- {"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"stdout:\nhello world\n"}]}}

    --> {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"run_recipe","arguments":{"recipe":"fail"}}}
    <-- {"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"error: recipe `fail` failed on line 5 with exit code 3"}],"isError":true}}

    --> {"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"run_recipe","arguments":{"recipe":"nixos-rebuild","async":true,"impure":true,"timeout":"25m"}}}
    <-- {"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"{\"job_id\":\"nixos-rebuild-9f3c1a2b\",\"cwd\":\"/home/user/repo/infra/hosts\",\"devshell\":\"/home/user/repo/infra/hosts/flake.nix\"}"}]}}
    (the caller then uses ringmaster's own job_wait/job_status/tail to observe completion)

    --> {"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"run_recipe","arguments":{"recipe":"infra/lint"}}}
    <-- {"jsonrpc":"2.0","id":4,"result":{"content":[{"type":"text","text":"resolved: cwd=/home/user/repo/infra, devshell=none"},{"type":"text","text":"stdout:\nlint ok\n"}]}}
    (infra/ has no flake.nix of its own, so this fell back to the repo root's -- none exists there either, so it ran with no devshell at all)

## Limitations

- No per-call variable overrides (`--set`): `run_recipe` reuses whatever
  overrides the target justfile/environment already carries.
- `timeout` only supports a single unit (`"25m"`, not `"1h30m"`) — no
  compound-duration parsing in this slice.
- Cooperative cancellation (`ringmaster cancel`/`job_cancel`) is wired up
  (just-us#33): `run_recipe_async` runs a second thread blocking in
  `ringmaster wait --on-cancel` alongside the recipe, and on an observed
  `cancel-requested` sends SIGTERM (then SIGKILL after a short grace
  period) to the recipe's whole process group — unix only; on other
  platforms an async job still can't be torn down early. A crashed or
  killed `just --mcp` process still orphans any in-flight async job the
  same way any ringmaster producer crash does (documented in
  `ringmaster(1)`'s CAVEATS) — the journal is garbage-collected after
  the retention window, not reaped; only a cooperative cancel via the
  journal is handled.
- `async`'s live output relies on the subprocess's stdout/stderr being
  redirected straight to the job's spool file — the sync path (no
  `async`) is buffered-until-exit, same as before.
- Devshell wrapping (`nix develop -c`) and full async job-completion/wake
  behavior have real environmental dependencies (a real `flake.nix` +
  network, a live clown session) that don't fit the hermetic bats-in-nix-
  sandbox lane well; `zz-tests_bats/mcp_tools.bats` covers sync execution,
  timeout, and the async dispatch call itself (job id returned promptly),
  but not devshell-wrapping or a full async completion+wake round trip —
  those were verified by manual smoke test instead.
- Still no Rust MCP SDK: `tools/list`/`tools/call` are hand-parsed
  JSON-RPC, same as FDR 0005's `prompts/*`. Revisit if/when FDR 0004's
  FUSE/editing facets need something richer.
- The exact `impure`/`timeout`/`async` parameter shape (flat fields on
  `run_recipe`) may not scale cleanly as more modifiers accumulate —
  tracked as a followup to explore a more structured shape
  (`forge.starbrandshoes.com/linenisgreat/just-us#28`).
- No caching: `list_recipes`/`show_recipe` recompile every discovered
  child justfile on every call. Fine at the scale this was verified
  against (a few hundred recipes across a handful of child justfiles);
  revisit if a repo's child-justfile count makes this measurably slow.
- Compact `list_recipes` reduces output size but doesn't bound it —
  a repo with enough recipes could still exceed the inline-result limit
  even in compact form. No pagination in this slice.
- `run_recipe`'s devshell selection is a bounded two-candidate fallback
  (the recipe's own directory, else the repo root's) — not an
  arbitrary-depth upward walk. A `flake.nix` at an intermediate ancestor
  (neither the recipe's own directory nor the root) is never picked up
  automatically. Deliberate: verified against every real child-justfile
  shape across the fleet at the time this was written, and an unbounded
  walk trades that predictability for the risk of picking up an
  unrelated ancestor flake never intended as the recipe's devshell.
  Structures that don't fit either candidate (e.g. a repo with several
  `flake.nix` files that aren't ancestors of the child justfiles that
  need them) aren't resolved by this or any directory-walk strategy;
  that's a separate, harder problem left for whenever it's actually
  reported.

## More Information

- FDR 0004 (`0004-clown-plugin-mcp-and-recipe-fuse.md`) — the parent
  design; this FDR implements its facet 1 (MCP server). Facets 2/3 (FUSE,
  MCP-based editing) remain open there.
- FDR 0005 (`0005-dynamic-system-prompt-recipe-roster.md`) — the
  `prompts/get system-prompt-append` capability this server already had;
  `tools` is added alongside it on the same server.
- FDR 0003 (`0003-recipe-model.md`) — the `RecipeModel`/`ModelRecipe`
  projection `list_recipes`/`show_recipe` serialize.
- `code.linenisgreat.com/clown` `ringmaster(1)` — the job-platform CLI
  `run_recipe`'s async mode shells out to (RFC-0009/0010/0011); its
  EXAMPLES section shows `spinclass`'s own pre-merge-hook job using the
  exact same `start`/`done` pattern this FDR follows.
- The retired `just-us-agents` moxy moxin (`amarbel-llc/moxy` repo,
  `moxins/just-us-agents/`) — the `run-recipe`/`run-recipe-default`
  devshell-wrapping and `JUST_US_AGENTS_IMPURE=1` behavior `run_recipe`'s
  `impure` parameter restores parity with.
