# `edit_recipe` / `delete_recipe`: MCP-based recipe editing — design

## Context

`just --mcp` (FDR 0005/0006) currently gives an agent discovery
(`list_recipes`/`show_recipe`/`dump_justfile`/`list_variables`) and
execution (`run_recipe`), but no way to *change* a justfile — an agent
still has to hand-edit the file directly. This is the "MCP-based recipe
editing" facet of FDR 0004 (still `proposed`), which also sketches a FUSE
filesystem alternative; this design covers the MCP-tool path only, per
explicit scoping in the brainstorm that produced it.

## Interface

Two new tools on the same stdio MCP server:

- `edit_recipe { op: "create"|"update", recipe: string, justfile?: string, doc_prelude?: string[], doc?: string, groups?: string[], parameters?: Parameter[], body?: string }`
  where `Parameter = { name: string, default?: string, variadic?: bool, export?: bool }`.
- `delete_recipe { recipe: string, justfile?: string }`

### Semantics

- **Strict create/update, no upsert.** `op: "create"` errors (`isError`,
  not a JSON-RPC protocol error) if `recipe` already exists; `op:
  "update"` errors if it doesn't. Matches the `create_node`/`put_node`
  split already used elsewhere in this org (cutting-garden) — a typo'd
  recipe name becomes a clear error instead of a silent overwrite or a
  silent no-op.
- **Partial update.** `update` only touches fields actually supplied;
  an omitted field keeps its current value (read-modify-write against
  the current `RecipeModel` entry, then re-serialize). `create` requires
  enough to produce a valid recipe (a name at minimum; `body` may be
  legitimately empty for an aggregate recipe).
- **`justfile` targeting.** Optional relative path; omitted → the
  server's own root justfile (today's only target). When supplied, it
  must resolve to — and stay within — the directory tree rooted at
  wherever `just --mcp` itself found its justfile (`Search::search`);
  `..`-escapes and absolute paths outside that tree are rejected. This
  is for **separate, non-`mod`-imported justfiles elsewhere in the repo**
  (e.g. a monorepo's `services/foo/justfile`), not `mod`-imported
  submodules — a namepath containing `::` is rejected with an error
  pointing at that distinction. No discovery tool for other justfiles
  yet (a `list_justfiles` tool is a tracked v2 addition); the caller is
  expected to already know the target path.
- **`delete_recipe` is a distinct tool**, not "update to empty body" —
  an empty-body recipe is a legitimate aggregate/lifecycle-group shape in
  this repo's own conventions (`eng-design_patterns-justfile(7)`), so
  overloading "no body" as "gone" would collide with real usage.

### Write mechanism (shared by create/update/delete)

Build the *entire* new source of the target justfile in memory:

1. Locate the recipe's current byte range in the source (for
   update/delete) or its lexically-sorted insertion point (for create).
2. Splice in the re-serialized recipe (replace / remove / insert).
3. `Compiler::compile` the resulting source in-process — the same call
   `just` itself makes to load a justfile — to confirm it still parses
   and type-checks.
4. Run the conformist `justfile-*` linters against the result.
5. Only if both 3 and 4 succeed, write the new content to disk.

A failure at step 3 or 4 returns the compiler/linter diagnostic as
`isError` tool content, exactly like `run_recipe`'s failure path — and
**nothing is written to disk**. This is a structural guarantee (nothing
touches the file until it's known-good), not a write-then-rollback
scheme, so there's no window where the real justfile is broken and no
backup/restore logic to get right.

Recipes in a justfile touched by any of these three tools are re-emitted
in lexical name order afterward — order is no longer author-controlled.

### Body validation

Beyond recompilation, when a recipe's body has no shebang or a
`#!/usr/bin/env {bash,sh,dash}` shebang, run it through shellcheck
(conformist already wires this in, currently scoped to `www/install.sh`)
and surface findings the same way as a compile/lint rejection. Other
shebang interpreters (python, node, ...) get structural-only validation
for now — no content linting — which is a **documented limitation**, not
a silent gap.

## Examples

    --> {"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"edit_recipe",
          "params":{"name":"edit_recipe","arguments":{
            "op":"create","recipe":"greet",
            "doc":"says hello",
            "parameters":[{"name":"name","default":"world"}],
            "body":"echo \"hello {{name}}\""
          }}}}
    <-- {"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"created"}]}}

    --> {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"edit_recipe",
          "arguments":{"op":"update","recipe":"greet","body":"exit 1"}}}
    <-- {"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"error: shellcheck SC2xxx: ..."}],"isError":true}}
    (justfile on disk is unchanged)

## Limitations

- No `dependencies`/`attributes` fields in v1 — tracked toward a v2 that
  adds them; `dependencies` in particular needs real design work
  (resolving namepaths, cycles, cross-module refs) before it's safe to
  expose as a settable field.
- `mod`-imported module creation/editing is out of scope; only the root
  justfile and other, separate (non-`mod`) justfiles in the repo tree.
- Non-shell shebang bodies (python, node, ...) get no content linting,
  only structural (does-it-compile) validation.
- Lexical-only recipe ordering is a real, visible cost for a justfile
  like this repo's own, which is organized into
  `eng-design_patterns-justfile(7)` lifecycle groups rather than pure
  alphabetical order — using `edit_recipe`/`delete_recipe` against such a
  file will reshuffle it. Not resolved here; worth revisiting once real
  usage shows whether that's actually acceptable.
- Justfile-immutability enforcement (should the raw file stop being
  human-editable once these tools exist?) is explicitly **not** decided
  by this design — it's FDR 0004's own open question, orthogonal to
  whether these two tools work correctly.

## Tuning Levers

| Lever | Current | Rationale | Change signal |
|---|---|---|---|
| Shellcheck-eligible shebangs | `bash`/`sh`/`dash` only | Matches what conformist already wires in; smallest safe extension | Real usage shows recipes in other interpreters (python, node) commonly break in ways structural validation misses |
| `justfile` path scope | Sandboxed to the server's own root tree | Prevents an edit reaching outside the project the server was launched for | A real multi-repo use case needs to reach further, with an explicit, reviewed boundary change |

## Rollback

Purely additive — no existing behavior changes. Rollback is reverting the
commit that adds these two tools (removes their `tools/list` entries and
`tools/call` dispatch arms); `list_recipes`/`show_recipe`/`run_recipe`/
`dump_justfile`/`list_variables` are unaffected. Because a rejected edit
never touches disk, there's no separate "undo a bad edit" story needed
for the write path itself — a *successful* edit is a normal git change to
the justfile, revertable the same way any other commit is.

## More Information

- FDR 0004 (`docs/features/0004-clown-plugin-mcp-and-recipe-fuse.md`) —
  parent design; this is its "MCP-based recipe editing" facet, FUSE
  excluded.
- FDR 0006 (`docs/features/0006-mcp-recipe-discovery-and-execution.md`) —
  the discovery/execution tools this shares a server with.
- FDR 0003 (`docs/features/0003-recipe-model.md`) — `RecipeModel`, the
  read side this write side must stay consistent with.
