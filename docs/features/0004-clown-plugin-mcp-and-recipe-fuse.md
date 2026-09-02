---
status: proposed
date: 2026-09-02
promotion-criteria:
---

# Clown plugin: real MCP server + recipe editing (FUSE and MCP tools)

## Problem Statement

Agents currently reach just-us's own justfile through the generic
`just-us-agents` moxy moxin — a wrapper shipped by the separate `moxy`
repo (`moxins/just-us-agents/`), not by just-us itself, that shells out
to `just --dump` / `just --evaluate` / `just --show` / positional
`just <recipe>`. This has three costs: (1) just-us has no first-class
agent-facing surface of its own — every consuming repo depends on a
justfile-agnostic wrapper maintained outside this repo, one that
cannot take advantage of just-us's own fork features (`doc_prelude`,
`--dump-format model`); (2) moxy "moxins" are being phased out
org-wide in favor of real clown plugins — `madder`, `dodder`, and
`cutting-garden` already ship `<name>-clown-plugin` MCP servers
consumed by eng's `lib/circus.nix`, and just-us is the outlier still
going through the generic moxin path; (3) hand-editing a justfile has
no structural guardrails — recipe order is whatever the author left
it in, and nothing enforces that an edit round-trips through
formatters/linters before it lands.

## Interface

Three facets of one underlying capability (list/read/run/edit recipes
in the project's justfile), shipped as a single clown plugin package
following the `<name>-clown-plugin` pattern `cutting-garden`, `madder`,
and `dodder` already use (clown-plugin-protocol(7), clown-json(5)).
Editing has two independent entry points — FUSE and MCP tool calls —
but both MUST route through the same internal validated-write core
(re-serialize the recipe, run it through conformist, reject on
failure) so the two surfaces can never disagree about what counts as
a legal edit.

### 1. MCP server (replaces the moxy moxin)

A real, stdio-speaking MCP server — not a wrapper that shells out to
`just` — exposing recipe discovery/execution as MCP tools, e.g.
`list_recipes`, `show_recipe`, `run_recipe`, and variable/dump parity
with the current moxin. `list_recipes`/`show_recipe` read from
`--dump-format model` (FDR 0003) rather than re-parsing
`--dump-format json`, so `doc_prelude`, groups, and resolved
dependency namepaths ride along for free.

Packaged the way `cutting-garden`'s MCP server is: a
`plugins/just-us/.claude-plugin/plugin.json` + `clown.json.in`
(substituted with the built binary's store path at Nix build time),
exposed from `flake.nix` as a `just-us-clown-plugin` package output,
consumed by eng's `lib/circus.nix` `basePlugins` list the same way
`cuttingGardenPlugin`/`dodderPlugin`/`madderPlugin` are — retiring the
`just-us-agents` moxin registration from the moxy `MOXIN_PATH`.

### 2. Recipe FUSE (new)

A FUSE filesystem exposing each recipe as a directory of files:

    .tmp/just-us-agents/justfile/
      <recipe>/
        doc_prelude
        description
        args
        body
      <recipe>/
        ...

Each file is a bidirectional view onto one field of the recipe in the
real justfile. Writing any file re-serializes that recipe back into
the justfile and runs the result through the project's
formatters/linters (conformist); the write fails (surfaced as a
filesystem write error) if formatting/linting rejects it, so a
malformed edit never lands.

The real justfile stops being directly editable once FUSE/MCP are the
sanctioned edit paths (enforcement mechanism TBD — see Open
Questions). Recipe order in the rendered justfile is no longer
author-controlled — recipes are automatically sorted lexically by
name.

### 3. MCP-based recipe editing (alternative to FUSE)

The same field-level edits FUSE exposes are also reachable as MCP
tool calls, for a client that would rather make one RPC than mount a
filesystem — e.g. `set_recipe_field(recipe, field, value)` (`field`
one of `doc_prelude` / `description` / `args` / `body`), or dedicated
`set_recipe_body` / `set_recipe_doc_prelude` / etc. tools if a
per-field schema reads better than a stringly-typed `field` parameter
(open question below). A `create_recipe` / `delete_recipe` pair could
plausibly live here too, since MCP tool calls (unlike the FUSE
directory listing, which mirrors whatever recipes already exist) can
naturally express "add a new one" as a single call. Whichever field a
tool changes goes through the exact same validated-write core as a
FUSE write: the recipe is re-serialized into the justfile, run through
conformist, and the tool call fails with the formatter/linter's
diagnostic if that rejects it — an agent gets the failure back as a
normal MCP tool error instead of an opaque filesystem `EIO`.

## Examples

    $ ls .tmp/just-us-agents/justfile/
    build/  bump-version/  release/  test/  test-bats/  ...
    $ cat .tmp/just-us-agents/justfile/test-bats/doc_prelude
    # authoritative bats suite in the nix sandbox
    $ echo 'cargo test --workspace' > .tmp/just-us-agents/justfile/test/body
    # fails if `nix fmt`/conformist would reject the resulting justfile

    // equivalent edit via the MCP tool instead of the FUSE file:
    set_recipe_field({ recipe: "test", field: "body", value: "cargo test --workspace" })
    // => same conformist validation; a rejection comes back as a tool
    //    error instead of a failed write(2)

## Limitations

- FUSE needs a userspace FUSE implementation on the host (macFUSE on
  darwin, libfuse on Linux) — a dependency class no existing
  clown plugin in this org uses today.
- Only single-field granularity per recipe (`doc_prelude`,
  `description`, `args`, `body`) is exposed; multi-recipe structural
  changes (adding/removing/reordering recipes, `[group]`/other
  attributes, module imports) are out of scope for the FUSE surface
  as drafted.

## Open Questions

- **No prior MCP implementation found in this fork's history.** I
  searched `amarbel-llc/just-us` commit messages, branches, and tags
  for "MCP"/"Model Context Protocol" and found none. The closest
  artifact is an unrelated, unmerged branch
  (`claude/ndjson-crap-protocol-968ovp`, 3 ahead / 14 behind master)
  implementing the **CRAP attach protocol** (`amarbel-llc/crap` — a
  different, ambient resource-attachment protocol, not MCP). If
  there's a specific commit/branch/repo in mind, point me at it before
  I scope the MCP implementation from scratch.
- **Rust MCP SDK choice.** Every existing clown-plugin MCP server in
  the org (`cutting-garden`, and presumably `madder`/`dodder`) is Go,
  built on `code.linenisgreat.com/purse-first/libs/go-mcp`. just-us is
  Rust — there's no existing Rust MCP precedent in this org to copy; a
  crate (e.g. `rmcp`, the official Rust SDK) needs to be selected.
- **"mkSpinclass/mkClown" naming.** Checked `~/eng/lib/circus.nix`:
  `mkSpinclass` is real (`inputs.spinclass.lib.${system}.mkSpinclass
  {...}`, builds the spinclass binary itself with build-time pins),
  but there is no `mkClown` function — `cutting-garden`, `madder`, and
  `dodder` each hand-build their own `<name>-clown-plugin` derivation
  via `pkgs.runCommand` following the clown-plugin-protocol spec, and
  eng's `lib/circus.nix` adds a `{flake; dirs;}` entry to
  `basePlugins` for each. There's no shared "mkClown" nix helper to
  integrate against — just-us would follow the same hand-rolled
  pattern as its three precedents, not a generic builder.
- **Justfile-immutability enforcement.** Unspecified: filesystem
  permissions (chmod the justfile read-only), a conformist lint gate
  that rejects any diff touching the justfile outside a
  FUSE/MCP-attributed commit, or purely a documented agent
  convention?
- **Per-field tools vs. one generic tool.** Is recipe editing over MCP
  a single `set_recipe_field(recipe, field, value)` tool, or one
  dedicated tool per field (`set_recipe_body`, `set_recipe_args`,
  ...)? The generic form is less surface area to maintain; dedicated
  tools give each field its own typed schema (e.g. `args` as a
  structured list rather than a raw string) and clearer per-tool
  descriptions for a client's tool picker.
- **Is FUSE still worth building once MCP editing exists?** If most
  agent clients will just call the MCP tools, FUSE's userspace
  dependency (see Limitations) may not earn its keep as anything more
  than a human-ergonomics nicety (`$EDITOR` on a virtual file). Worth
  revisiting once it's clear who the FUSE surface is actually for.
- **FUSE mount lifecycle.** When does
  `.tmp/just-us-agents/justfile/` get mounted/unmounted — a
  `just-us mount` subcommand driven by the clown plugin's lifecycle
  hooks, on-demand at first access, or something else?
- **Upstreaming to circus.** The scope of what
  `amarbel-llc/circus` would own (vs. what stays in just-us/eng) needs
  to be nailed down before filing the tracking issue there.

## More Information

- FDR 0003 (`docs/features/0003-recipe-model.md`) — the
  `--dump-format model` projection `list_recipes`/`show_recipe` should
  read from.
- `amarbel-llc/cutting-garden` FDR 0015
  (`docs/features/0015-mcp-resource-server.md`) — the closest existing
  precedent for an MCP-server-as-clown-plugin in this org; the
  packaging pattern (`clown.json.in`, `plugin.json.in`,
  `<name>-clown-plugin` flake output) this FDR intends to mirror.
- `amarbel-llc/clown` `docs/rfcs/0002-clown-plugin-protocol.md`,
  `man/man5/clown-json.5`, `man/man7/clown-plugin-protocol.7` — the
  plugin manifest/lifecycle spec.
- `~/eng/lib/circus.nix` — where `basePlugins` is assembled; this
  plugin's consumption point.
- The current `just-us-agents` moxy moxin (`amarbel-llc/moxy` repo,
  `moxins/just-us-agents/`) — what this plugin replaces.
