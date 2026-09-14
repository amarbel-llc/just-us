---
status: proposed
date: 2026-09-14
promotion-criteria:
---

# `just-edit`: a structural justfile editor (the nixedit analog)

## Problem Statement

Agents and one-off fleet-wide corrections currently edit justfiles the only
way anyone can: open the file and rewrite it by hand (or have an LLM
regenerate the whole thing), with no guarantee the result still parses, and
no guarantee it still satisfies conformist's `justfile-*` conventions
(`conformist-justfile(7)`). FDR 0004 sketched two agent-facing edit surfaces
(FUSE, MCP tool calls) that both assumed some validated "write core" would
exist underneath them, but never designed it. This FDR is that write core.

The trigger is `~/eng` FDR 0015's cross-repo design session (2026-09-01),
decision #7: divergence from upstream `just` is accepted policy (upstream is
no longer accepting contributions), so the fork grows a structural justfile
editor — the direct analog of doppelgang's `internal/0/nixedit`, which
performs targeted, byte-preserving splices into `flake.nix` rather than
regenerating the file. `nixedit` is itself lifting into igloo (decision #6)
so every repo can reach it as a dependency; this FDR is the same move for
justfiles, staying in just-us because only just-us's own parser can compile
one authoritatively (unlike Nix, an external language nixedit's shallow PEG
grammar approximates, `just`'s own compiler already IS the ground truth).

The end goal (operator, this session): agents stop editing justfiles
directly, full stop — every edit goes through tooling that always leaves the
file both **valid** (parses) and **conformant** (passes the repo's
conformist profile). That end state is reached incrementally. This FDR
scopes the first increment: a tool that guarantees validity for a narrow set
of single-field edits. Conformance-checking is a deliberately separate,
later layer (see "Relationship to conformist's changer verb" below) — not
because it doesn't matter, but because coupling this tool to profile
resolution now would be solving eng FDR 0015's still-`exploring` profile
vision inside a `proposed` FDR that doesn't need it to be useful.

## Interface

### A new binary, not a `just` subcommand

Ships as **`just-edit`**, a separate binary in this workspace
(`crates/just-edit/`), not a flag or subcommand on `just` itself (operator
decision, this session). `just` stays the runner; `just-edit` is the writer.
This mirrors how `nixedit` is a package independent of the `nix` CLI, not a
`nix` subcommand, and keeps `just`'s own argument surface — already large,
upstream-inherited — untouched by an editing feature only the fork has.

`just-edit` depends on the `just` library crate as a workspace path
dependency (`crates/just-edit/Cargo.toml` → `just = { path = "../.." }`),
the same way `crates/action-versions` etc. are workspace members — except
those three existing `crates/*` are self-contained dev-tool xtasks with no
dependency on `just` itself; `just-edit` would be the first. It reuses
`just`'s real `Loader`/`Compiler`/`Parser`/`Ast`/`Recipe`/`Token` types
rather than writing a second, narrower justfile grammar — the thing
`nixedit` had to do because Nix isn't its repo's language, and just-us
doesn't have that excuse. This requires promoting the specific types listed
below from `pub(crate)` to `pub` behind the same `#[doc(hidden)]`,
no-semver-guarantee tier `src/lib.rs` already uses for `Arguments`/
`Response`/`INIT_JUSTFILE` — not a new public API commitment, just widening
an existing "internal-but-reachable" door.

### v1 scope: single-field edits on an existing recipe

One edit per invocation, targeting one recipe by its resolved `namepath`
(FDR 0003's model field — globally unique across root + all modules, so
targeting by it is unambiguous whenever the file compiles at all):

    just-edit set-field --recipe <namepath> --field <doc|doc_prelude|body|parameters> --value <text>

Explicitly OUT of v1 scope (deferred, not rejected): `create-recipe`,
`delete-recipe`, reordering, module-level edits, attribute edits other than
`[doc(...)]`. FDR 0004 called `create`/`delete` "naturally a single MCP
call," but nixedit's own precedent is narrow too — it only ever splices
`follows` bindings into an existing `inputs` attrset, never restructures a
flake.nix — and matching that narrowness keeps this FDR's first deliverable
honest to "designs the subcommand surface" rather than "designs the whole
editor."

### Edit mechanism: byte-preserving span splice, not reserialization

For `body`/`parameters` (and any attribute-backed field): the target span is
already fully recoverable from data the parser already computes — each body
`Line`'s `Fragment::Text` carries its original `Token { offset, length, .. }`
(`src/token.rs`), so the byte range from the first body line's first token
to the last body line's last token's end is exact, with zero new parser
computation. `just-edit` splices the new field's rendered text into that
byte range and leaves every other byte in the file untouched — no incidental
reformatting of unrelated recipes, no comment reflow, matching nixedit's own
`Apply(src, lines) -> (out, applied, err)` shape (`internal/0/nixedit/nixedit.go`).

For `doc`/`doc_prelude`: `Recipe.doc`/`doc_prelude` are stored as already-
*cooked* `String`/`Vec<String>` with no retained span (`src/recipe.rs`) —
this is a real gap, not a design choice, and needs one of:

- capture the token span alongside the cooked text when the parser first
  builds `doc_prelude` (the "contiguous comment run" scan already exists;
  it would need to also record where that run starts/ends in bytes), or
- reconstruct the span later by re-running that same contiguous-comment-run
  scan backward from the recipe's `name.line`, independently of the cooked
  strings.

The first is more principled (span computed once, at the source of truth);
the second avoids touching the hot parse path for an edit-only concern.
Undecided — see Open Questions.

### Validated write: refuse rather than corrupt

After splicing, `just-edit` feeds the resulting bytes back through `just`'s
own `Loader`/`Compiler` before writing anything to disk. If the result
doesn't compile, the edit is refused and the original file is untouched —
no partial write, matching flakeclobber's all-or-nothing contract (eng FDR
0015 point #8) and nixedit's own `ErrUnparseable` fallback
("apply nothing... report-only"). This is the "always valid" half of the
end goal, and it is fully in scope for v1: it costs one extra compile pass
`just-edit` already needs a `Compiler` for, per the library dependency
above.

Ambiguity refusal for v1's narrow scope is mostly: recipe not found by
namepath (refuse, list near-matches by edit distance the way `just`'s own
"Did you mean" suggestions work — `src/suggestion.rs`), or `field` not
applicable to the recipe's actual shape (e.g. `doc` targeted on a recipe
using `[doc(...)]` — the model's `attributes` field already reports this;
`just-edit` would need to edit the attribute's argument instead of a
comment line, a distinct code path from the comment-line case, or simply
refuse until that path is built).

### Relationship to conformist's changer verb

"Always conformant," per the operator's stated end goal, is explicitly
**not** this FDR's job. Per eng FDR 0015 point #8, conformist is growing a
third verb (name open: change/modify/edit/mutate) for one-off migrations
with a done-state and refuse-on-ambiguity semantics, generalizing
flakeclobber. That verb's migrations "name a profile-delivered tool
(nixedit, the just-us editor, ast-grep-like tools)" — i.e. `just-edit` is
exactly the kind of tool conformist's changer will invoke and then re-lint,
retrying or refusing on failure. `just-edit` itself stays profile-ignorant:
it guarantees the file still compiles, and conformist's (future, still
`exploring`) changer layer is what turns that into "and still passes
`justfile-recipe-descriptions`." Building profile-awareness into `just-edit`
now would be solving a design that hasn't landed yet inside one that has a
narrower, already-justified reason to exist.

## Examples

    $ just-edit set-field --recipe test-bats --field doc \
        --value 'authoritative bats suite in the nix sandbox'
    ok: test-bats.doc updated (justfile:111)

    $ just-edit set-field --recipe explore::debug-foo --field body \
        --value $'@echo foo\n@echo bar'
    ok: explore::debug-foo.body updated (zz-explore/justfile:2)

    $ just-edit set-field --recipe nonexistent --field doc --value x
    error: no recipe named 'nonexistent'
    did you mean 'test'?
    (no changes written)

    $ just-edit set-field --recipe test --field body --value '{{ malformed'
    error: splice would not compile: Unterminated interpolation
    (no changes written; original justfile untouched)

## Limitations

- Fork-only, same terms as `--events-fd`/`doc_prelude`/`--dump-format
  model`: no upstream PR planned.
- v1 has no structural ops (create/delete/reorder recipe, module edits) —
  see "v1 scope" above.
- `just-edit` guarantees the result **compiles**; it does not guarantee the
  result satisfies any conformist linter. A caller wanting both must run
  `just-edit` then a lint pass itself until conformist's changer verb exists
  to do that composition.
- One edit per invocation in v1. Multi-edit batching (nixedit's own
  offset-grouped single-pass splice, `internal/0/nixedit/nixedit.go:110-137`)
  is a natural v2 shape once single edits are proven, not built here.

## Open Questions

- **doc/doc_prelude span recovery.** Capture spans during parsing (extend
  wherever `doc_prelude`'s comment-run scan lives) vs. reconstruct later via
  a second backward scan over raw source. Affects whether this FDR's first
  landed slice covers `doc`/`doc_prelude` at all, or ships `body`/
  `parameters` first and adds comment-backed fields once the span question
  is settled.
- **Exact set of types to promote from `pub(crate)` to `pub`.** At minimum
  something reaching `Loader`, `Compiler`, `Justfile`, `Recipe`, `Token`,
  `Position`, `Ast`/`Item` — the full dependency closure needs a pass once
  implementation starts; `unreachable_pub = "deny"` (Cargo.toml lints) means
  this can't be done accidentally, only deliberately.
- **Output/result shape for machine consumers.** The examples above are
  human-readable text; FDR 0004's MCP-editing facet will eventually want
  `just-edit` invoked from `run_recipe`-style tooling with structured JSON
  results (mirroring `run_recipe`'s existing `{cwd, devshell}` pattern) —
  worth deciding now or leaving text-only for v1 and adding `--format json`
  later without a breaking change to the text form.
- **Value input for multi-line fields (`body`, `doc_prelude`).** CLI flags
  don't do multi-line well; likely needs `--value -` (read stdin) or a
  `--value-file` escape hatch, not just a single `--value <string>` arg as
  sketched above.
- **Where does `just-edit`'s binary get built/shipped?** Same
  `just-us-clown-plugin` package as `just --mcp` (FDR 0005/0006), a second
  flake output, or independent of the clown-plugin packaging entirely (a
  plain `packages.just-edit`) since its first consumer is conformist's
  changer verb, not an MCP client? Affects `flake.nix` wiring, not the tool
  itself.

## More Information

- `amarbel-llc/doppelgang` `internal/0/nixedit/nixedit.go` — the named
  analog: byte-preserving splice, idempotent apply, `ErrUnparseable`
  refuse-rather-than-corrupt fallback, offset-grouped multi-edit batching.
- `~/eng` `docs/features/0015-conformist-profile-cache-delivered-linters.md`
  — the umbrella FDR; decisions #6 (nixedit → igloo), #7 (this FDR's
  origin), #8 (conformist's changer verb, this tool's future consumer).
- [FDR 0003](0003-recipe-model.md) — the read side (`--dump-format model`,
  namepath resolution) this FDR's write side is the counterpart to.
- [FDR 0004](0004-clown-plugin-mcp-and-recipe-fuse.md) — the still-open MCP
  editing / FUSE facets this tool's validated-write core is meant to sit
  underneath, once built.
- just-us#25 — the tracking issue this FDR answers.
- `src/token.rs`, `src/line.rs`, `src/recipe.rs` — the existing span data
  (`Token::offset`/`length`) and the gap (`Recipe.doc`/`doc_prelude` as
  cooked strings) this FDR's Interface section is grounded in.
