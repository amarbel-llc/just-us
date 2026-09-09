---
status: proposed
date: 2026-09-09
promotion-criteria:
---

# MCP recipe discovery and execution (`list_recipes` / `show_recipe` / `run_recipe`)

## Problem Statement

FDR 0005 gave just-us's `just --mcp` server exactly one capability: a
static `prompts/get` roster (name + doc line) injected into the agent
system prompt at launch. That is read-only discovery with no way to
actually *run* anything — an agent still has to fall back to the
`just-us-agents` moxy moxin (a wrapper maintained outside this repo) or a
raw shell `just <recipe>` to execute a recipe. This FDR implements the
`tools` facet of FDR 0004 (list/show/run recipes over MCP) on the same
`just --mcp` server. FUSE and MCP-based recipe *editing* (FDR 0004's other
two facets) stay out of scope here.

## Interface

The same stdio MCP server FDR 0005 introduced now also advertises the
`tools` capability (`initialize.capabilities.tools = {}`, alongside
`prompts`) and answers `tools/list` / `tools/call`.

- **`list_recipes`** (no input) — every public recipe
  (`ModelRecipe::private == false`, same visibility contract as the
  system-prompt roster and `--list`), serialized as the full FDR 0003
  recipe model: `namepath`, `doc`, `doc_prelude`, `groups`, `parameters`,
  `dependencies`, `source`, `line`.
- **`show_recipe { recipe: string }`** — the same model entry for one
  namepath. Also `!private`-gated, for consistency: nothing reachable
  through `list_recipes` or the system-prompt roster is separately
  reachable by naming it directly. Unknown or private name → a tool
  result with `isError: true`, not a JSON-RPC protocol error.
- **`run_recipe { recipe: string, args?: string[] }`** — runs the recipe
  through the exact same `Justfile::run` path `just <recipe>` itself
  uses. `args` are positional, in declared-parameter order — `just` has
  no named-argument CLI syntax, so there is no richer mapping to invent.
  Returns the recipe's captured stdout/stderr as text content, and sets
  `isError: true` (with the formatted error appended as an extra text
  block) when the recipe fails or is unknown.

Tool-call protocol errors (unknown tool name, missing/malformed
arguments) are JSON-RPC errors (the existing `error()` response shape).
A recipe that *runs* but exits non-zero is a **successful** JSON-RPC
response with `result.isError: true` — consistent with the MCP tools
convention that execution failure is tool-result data, not a transport
error, and it preserves whatever output was captured before the failure.

### How `run_recipe` avoids corrupting the JSON-RPC stream

This server's real stdout is the MCP JSON-RPC channel — a recipe's child
process stdout/stderr must never be inherited from it. `run_recipe` reuses
the RFC 0002 (`--events-fd`) output-capture path instead of building a new
one: `src/recipe.rs`'s capture branch is gated on `EventSink::is_active()`,
not on `config.events_fd` being set, and `EventSink::from_writer` accepts
any `Write + Send` sink, not only a real fd. So `run_recipe` builds an
`EventSink::from_writer` over an in-memory buffer, calls `Justfile::run`
exactly as `Subcommand::Run` does (with a cloned `Config` whose
`subcommand` is swapped to `Run { arguments }` so `Justfile::run`'s own
invocation-parsing branch fires), and the recipe's entire captured
stdout/stderr — as `--events-fd`'s own NDJSON `Event::Output` records —
lands in that buffer instead of on the server's real stdout. No subprocess
re-invocation of the `just` binary; this is `--events-fd` semantics with
the "fd" being an in-memory pipe relayed back over MCP.

## Examples

    --> {"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"run_recipe","arguments":{"recipe":"greet","args":["world"]}}}
    <-- {"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"stdout:\nhello world\n"}]}}

    --> {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"run_recipe","arguments":{"recipe":"fail"}}}
    <-- {"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"error: recipe `fail` failed on line 5 with exit code 3"}],"isError":true}}

## Limitations

- No per-call variable overrides (`--set`): `run_recipe` reuses whatever
  overrides the server was launched with.
- `just`'s own command-echo lines (`eprintln!` before each shell command,
  printed unless the recipe is quiet or `--quiet` is set) go to the
  server's real stderr, not into the captured content — they are just's
  own diagnostic output, not the recipe's child-process stdout/stderr, and
  `--events-fd`'s capture path never touched them either.
- Buffered, not streamed: like `--events-fd` itself, a `run_recipe` result
  only appears after the recipe finishes. No partial/live output for a
  long-running recipe.
- Still no Rust MCP SDK: `tools/list`/`tools/call` are hand-parsed
  JSON-RPC, same as FDR 0005's `prompts/*`. Revisit if/when FDR 0004's
  FUSE/editing facets need something richer.

## More Information

- FDR 0004 (`0004-clown-plugin-mcp-and-recipe-fuse.md`) — the parent
  design; this FDR implements its facet 1 (MCP server). Facets 2/3 (FUSE,
  MCP-based editing) remain open there.
- FDR 0005 (`0005-dynamic-system-prompt-recipe-roster.md`) — the
  `prompts/get system-prompt-append` capability this server already had;
  `tools` is added alongside it on the same server.
- FDR 0003 (`0003-recipe-model.md`) — the `RecipeModel`/`ModelRecipe`
  projection `list_recipes`/`show_recipe` serialize.
- `docs/rfcs/0002-just-events-fd-stream.md` — the NDJSON event stream and
  the "Suppressing Inherited stdout/stderr" capture mechanism `run_recipe`
  reuses via an in-memory `EventSink` instead of a real fd.
