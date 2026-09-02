---
status: proposed
date: 2026-09-02
promotion-criteria:
---

# Dynamic system-prompt recipe roster (`system-prompt-append`)

## Problem Statement

An agent working in this repo currently has no idea what recipes exist
until it runs `just --list` (or the moxy `just-us-agents` moxin's
`list-recipes` tool, FDR 0004) — a discovery step it has to remember to
take. Every public recipe's name and one-line doc comment is small,
static-per-checkout data that costs nothing to surface up front. The
clown plugin protocol's dynamic system-prompt contribution mechanism
(RFC 0002 §5) exists for exactly this: computing a fragment at session
launch from live repo state, folded into the agent's system prompt
before it ever asks.

This is a narrower slice of FDR 0004 (`0004-clown-plugin-mcp-and-recipe-fuse.md`):
just the roster, as one MCP prompt, with none of that FDR's tool/FUSE/edit
surface. It was called out as pre-emptive work ahead of the rest, following
an ownership split: just-us now owns surfacing its own recipes into the
agent system prompt; spinclass (which previously had an exploratory,
cost-estimation-only recipe for this — `explore-recipe-prompt-cost`,
spinclass#287/#286) keeps only its own scratch-justfile plumbing, not a
generic cross-repo injector.

## Interface

just-us's stdio MCP server (`just --mcp` — just's CLI is flag-based, not
subcommand-based, so this rides the same `--dump`/`--edit`/`--fmt` style
as every other mode; see FDR 0004) advertises the
`prompts` capability with exactly one prompt, the protocol's well-known
fixed name:

    system-prompt-append

A `prompts/get` for that name returns one line per **public** recipe
(`private == false` in the FDR 0003 recipe model — i.e. not
underscore-prefixed and not `[private]`-attributed), across the root
justfile and every module, flattened and namepath-sorted:

    <namepath>  <doc>

`<doc>` is the recipe's single `--list` doc-comment line (empty string
when absent). There is no further filtering: no group exclusion (debug/
explore recipes are included), no truncation, no `doc_prelude` context —
just the same "name + doc line" shape `--list` already shows a human,
lifted verbatim into the prompt.

Packaged via `clown.json`'s `stdioServers.just-us.systemPrompt = true`
(clown-plugin-protocol(7); mirrors spinclass's own
`internal/sysprompt` / `clown.json` wiring). The fetch is best-effort on
clown's side — a timeout or non-conforming response degrades to no
fragment and never blocks the session launch.

## Examples

    $ just --mcp
    (blocked on stdin, speaking newline-delimited JSON-RPC)

    --> {"jsonrpc":"2.0","id":1,"method":"initialize","params":{...}}
    <-- {"jsonrpc":"2.0","id":1,"result":{"capabilities":{"prompts":{}}}}

    --> {"jsonrpc":"2.0","id":2,"method":"prompts/get","params":{"name":"system-prompt-append"}}
    <-- {"jsonrpc":"2.0","id":2,"result":{"messages":[{"role":"user","content":{"type":"text","text":
          "build  build the fork\ntest  cargo test --workspace\ntest-bats  authoritative bats suite in the nix sandbox\n..."
        }}]}}

## Limitations

- One fixed prompt, no `tools`/`resources` capability — this is not yet
  the recipe-discovery/execution MCP surface FDR 0004 describes; an agent
  still needs those tools (or the moxy moxin, until retired) to actually
  run a recipe.
- Static per launch: the roster reflects the justfile at the moment
  clown's stdio bridge issues its one `prompts/get`, before the session
  starts. A recipe added mid-session is invisible until the next launch —
  the same snapshot-at-startup limitation spinclass's own dynamic fragment
  documents.
- No Rust MCP SDK: `initialize`/`prompts/get` are hand-parsed JSON-RPC over
  stdio, not built on a general-purpose MCP crate. Fine for two methods;
  revisit when FDR 0004's `tools` capability lands.

## More Information

- FDR 0004 (`0004-clown-plugin-mcp-and-recipe-fuse.md`) — the fuller MCP
  server + FUSE/MCP recipe-editing surface this is a slice of.
- FDR 0003 (`0003-recipe-model.md`) — `RecipeModel`/`ModelRecipe`, the data
  source (`namepath`, `doc`, `private`) this prompt renders from.
- `amarbel-llc/clown` `docs/rfcs/0002-clown-plugin-protocol.md` §5,
  `man/man7/clown-plugin-protocol.7` ("DYNAMIC SYSTEM-PROMPT
  CONTRIBUTION"), `man/man5/clown-json.5` (`systemPrompt`,
  `systemPromptPath`) — the normative protocol.
- `amarbel-llc/spinclass` `internal/sysprompt/sysprompt.go` — the existing
  in-org implementation of this exact mechanism (`PromptName =
  "system-prompt-append"`), the direct template this follows.
