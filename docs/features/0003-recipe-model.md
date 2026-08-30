---
status: experimental
date: 2026-08-30
promotion-criteria: the conformist `justfile-*` linters read this model
  instead of reconstructing recipe structure from `--dump-format json`
  with jq, and conformist#85 (module-qualifier stripping) and #89
  (module recursion) are fixed by consuming the model's `name`/`module`
  split and flat `recipes` list — with no breaking revision to the v1
  schema during that rollout
---

# `--dump-format model`: a normalized recipe model for policy consumers

## Problem Statement

conformist's `justfile-*` linters reconstruct recipe structure from
`just --dump --dump-format json` with fragile jq, re-deriving what the
parser already computed and discarded. Two structural bugs follow
directly from that reconstruction:

- **conformist#85** — the linters extract a recipe's verb by splitting
  its name on `-`. A `mod`-imported recipe's fully-qualified path is
  `explore::debug-foo`, so the "verb" comes out `explore::debug` and
  never matches an allowlist. Every recipe in a module fails, and the
  whole-tree linters offer no escape hatch.
- **conformist#89** — five of the seven dump-based linters read only the
  root `.recipes` and never recurse into `.modules`, so every
  module-imported recipe is silently unlinted.

A third defect is latent in the raw dump itself: a dependency serializes
as the depended-on recipe's **bare** name (`keyed::serialize` →
`Recipe::key()`), dropping any `mod::` qualifier, so ownership matching
is by ambiguous bare name.

The decision (from sasha, dispatched via eng/loud-sycamore): **lift the
data extraction into the fork.** just-us already diverged once to expose
parser data jq cannot see — the `doc_prelude` field
([FDR 0002](0002-doc-prelude.md)). This generalizes that move: a native,
complete, normalized recipe **model** that conformist consumes by field
lookup. conformist stays the policy engine (verb allowlists, lifecycle
groups, the aggregate/leaf taxonomy); it stops reconstructing data.

The long-term horizon — explicitly **not** built here — is `just --lint`
with policy declared via `set` directives at the top of a justfile. The
model is shaped so that future is reachable: it is pure DATA, and an
internal linter would read the same data plus policy from directives.

## Interface

The model is a new `--dump-format` variant, emitted through the existing
dump path:

    just --dump --dump-format model

The payload is JSON. Its shape is a versioned **contract**: consumers pin
`version` and tolerate additive growth. The format token `model` is
stable — the schema version rides inside the payload, never in the token.

### Envelope

| field | type | meaning |
|---|---|---|
| `schema` | string | Stable identifier: `"just-us.recipe-model"`. |
| `version` | int | Schema version (currently `1`). Additive fields do not bump it; a breaking change does. |
| `root_default` | string \| null | Namepath of the ROOT justfile's default recipe (the one bare `just` runs), or null. |
| `recipes` | array | Every recipe, across the root and all modules, **flattened**, sorted by `namepath`. |
| `modules` | array | Every submodule, as data (the root is not included), sorted by `path`. |

### Per-recipe fields

| field | type | meaning | consumer need |
|---|---|---|---|
| `name` | string | BARE recipe name, module qualifier stripped. | verb extraction (conformist#85) |
| `namepath` | string | Fully-qualified stable identity, `::`-joined. | stable id, dependency resolution |
| `module` | [string] | Containing module path components; `[]` for a root recipe. | conformist#85 / #89 |
| `source` | string | Absolute path of the justfile defining the recipe. | file-level findings |
| `line` | int | 1-based line of the recipe name in `source`. | source positions |
| `doc` | string \| null | The single comment line `--list` prints as the description. | recipe-descriptions |
| `doc_prelude` | [string] | Comment lines stranded above `doc`, source order; **always present**, `[]` when none. | orphan-summary |
| `groups` | [string] | `[group(...)]` values, source order; `[]` if none. | debug-recipes, lifecycle |
| `attributes` | [string] | Attribute discriminants present, kebab-case, de-duplicated. | forward-compat escape hatch |
| `private` | bool | Underscore-prefixed OR `[private]`. | descriptions, exemptions |
| `is_default` | bool | Is the default recipe of its OWN justfile scope (see below). | default |
| `has_body` | bool | Has body lines (the leaf signal). | leaf-noun, aggregate-comments |
| `parameters` | [string] | Parameter names, declaration order. | signatures |
| `dependencies` | [{name, namepath}] | Dependencies with RESOLVED identities. | dependency-drop fix |

`doc_prelude`, `groups`, and `dependencies` are **always emitted** (empty
list when none) — the normalized model has no absent-vs-empty ambiguity,
unlike the raw dump which omits an empty `doc_prelude`.

### The normalized fields are canonical; `attributes` is the escape hatch

`private`, `groups`, `is_default`, and `has_body` are the **canonical**
read for the facts they cover; a consumer should not re-derive them from
`attributes`. The raw `attributes` list exists so a consumer can reach an
attribute the normalized fields do not cover (`[confirm]`, `[no-cd]`, a
platform gate) without falling back to `--dump-format json`.

### `is_default` is per-scope

A justfile scope — the root, or any module — has its own default recipe
(the one `just` or `just <module>` runs with no recipe named).
`is_default` is true for that recipe **in its own scope**, so a module's
first recipe reads `is_default: true`. `root_default` names the
unambiguous root one; a consumer wanting "is this the root default" tests
`namepath == root_default`.

### Policy boundary

The model is **DATA only**. It deliberately does not emit a derived
`is_aggregate`, `is_leaf`, or `verb` — those are policy. It carries the
raw signals (`has_body`, `dependencies`, bare `name`, `groups`,
`attributes`) and conformist applies the eng taxonomy. This keeps the
contract stable across policy changes and keeps the `just --lint` horizon
reachable.

## How the three defects become structurally impossible

- **conformist#85.** Every recipe carries its BARE `name` and its
  `module` path separately. Verb extraction splits `name`, which never
  contains a `::` qualifier, so a module recipe's verb is correct without
  any stripping step.
- **conformist#89.** `recipes` is a single FLAT list spanning the root
  and all modules. A consumer iterates it once; there is no `.modules`
  subtree to forget to recurse into.
- **Dependency `mod::` drop.** Each dependency carries the resolved
  `namepath`, not just the bare name, so ownership matching is exact.

## Example

Root justfile importing a submodule:

    # exploration recipes
    mod explore 'zz-explore/justfile'

    # the default aggregate
    # run build then test
    default: build test

    # Build the release binary and
    # strip it
    build:
        @echo building

    # run the tests
    [group('test')]
    test: build
        @echo testing

    _helper:
        @echo helper

`zz-explore/justfile`:

    # poke at internals and
    # see what happens
    debug-foo:
        @echo foo

    # a clean explorer
    explore-bar: debug-foo
        @echo bar

`just --dump --dump-format model` (abridged to the load-bearing entries):

    {
      "schema": "just-us.recipe-model",
      "version": 1,
      "root_default": "default",
      "recipes": [
        {
          "name": "build",
          "namepath": "build",
          "module": [],
          "doc": "strip it",
          "doc_prelude": ["Build the release binary and"],
          "has_body": true,
          "is_default": false,
          "dependencies": []
        },
        {
          "name": "default",
          "namepath": "default",
          "module": [],
          "doc": "run build then test",
          "has_body": false,
          "is_default": true,
          "dependencies": [
            { "name": "build", "namepath": "build" },
            { "name": "test", "namepath": "test" }
          ]
        },
        {
          "name": "debug-foo",
          "namepath": "explore::debug-foo",
          "module": ["explore"],
          "doc": "see what happens",
          "doc_prelude": ["poke at internals and"],
          "is_default": true,
          "dependencies": []
        },
        {
          "name": "explore-bar",
          "namepath": "explore::explore-bar",
          "module": ["explore"],
          "dependencies": [
            { "name": "debug-foo", "namepath": "explore::debug-foo" }
          ]
        },
        {
          "name": "test",
          "namepath": "test",
          "module": [],
          "groups": ["test"],
          "attributes": ["group"],
          "dependencies": [{ "name": "build", "namepath": "build" }]
        }
      ],
      "modules": [
        { "path": ["explore"], "doc": "exploration recipes", "source": ".../zz-explore/justfile" }
      ]
    }

Note `debug-foo` reads `name: "debug-foo"` with `module: ["explore"]`
(conformist#85), appears in the same flat list as the root recipes
(conformist#89), and `explore-bar`'s dependency resolves to
`explore::debug-foo` rather than a bare `debug-foo`.

## Limitations

- **Fork-only.** Upstream `just` has no `model` dump format. A consumer
  must run a just-us build; there is no runtime way to tell stock `just`
  from the fork other than the format being rejected.
- **Platform-disabled recipes are absent.** The model is a projection
  over the compiled `Justfile`, and `Analyzer` filters out recipes
  disabled on the current host (`recipe.enabled()`) before they reach
  `Justfile::recipes`. So `[macos]`-gated recipes are invisible on Linux,
  exactly as they are in `--dump-format json`. Tracked as **just-us#19**;
  its proposed `enabled`/`platforms` marker is the model's forward path.
- **A module DECLARATION carries no `doc_prelude`; a module RECIPE
  does.** These are different things, and only the first is deferred.
  A recipe *defined inside* a module file is a recipe like any other:
  its `recipes[]` entry has a fully **populated** `doc_prelude` (in the
  worked example, `explore::debug-foo` emits
  `["poke at internals and"]`). What is unavailable is the prelude of
  the `mod …` declaration itself — the parser discards it for module
  items, so a `modules[]` entry has a `doc` but no `doc_prelude` field
  at all, and a module whose own `--list` doc is a truncated fragment
  is not detectable here. Tracked as **just-us#21** (a one-call-site
  parser change).
- **No line-level spans for prelude entries.** `doc_prelude` entries are
  trimmed text, not spans; `line` locates the recipe, not each stranded
  comment.

## Tuning Levers

| Lever | Current | Rationale | Change signal |
|---|---|---|---|
| delivery | `--dump-format model` | reuses the dump machinery; one enum variant + one match arm | a consumer needs a mode `--dump` cannot express |
| shape | flat `recipes` list | makes #89 structurally impossible; every entry self-describes its scope | a consumer needs the tree and cannot rebuild it from `module`/`source` |
| version knob | int `version` + stable `schema`; additive is non-breaking | lets conformist pin one integer and tolerate growth | the additive-vs-breaking line proves too coarse |
| normalized + raw | canonical normalized fields AND raw `attributes` | consumers never fall back to `--dump-format json` | a normalized field is found to mislead vs the raw attribute |
| policy boundary | DATA only; no `is_aggregate`/`verb` | keeps the contract stable and the `just --lint` horizon reachable | a policy fact is needed identically by every consumer and never varies |

## More Information

- `src/recipe_model.rs` — the model structs and the projection builder;
  the authoritative field-by-field description.
- `src/dump_format.rs`, `src/subcommand.rs` — the `Model` variant and the
  dump match arm.
- `tests/model.rs` — the mod-import integration tests pinning every claim
  above; `just debug-model` is the hand-driven smoke loop.
- [FDR 0002](0002-doc-prelude.md) — the fork's other parser-data feature
  and the precedent for diverging to expose it; `doc_prelude` is a model
  field.
- conformist#85, conformist#89 — the linter defects the model makes
  structurally impossible.
- eng#280, just-us#19, just-us#21 — the deferred follow-ups.
