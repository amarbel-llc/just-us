# AGENTS.md

Operating notes for agents working in this repository. Repo-wide eng
conventions live in the `eng-*(7)` manpages — consult those first;
this file covers only what is specific to just-us.

## What this repository is

just-us is a fork of [casey/just](https://github.com/casey/just) (the
command runner) maintained at `amarbel-llc/just-us`. Its reason to
exist is the `--events-fd` feature: a machine-readable recipe-event
stream for agent harnesses, specified in `docs/rfcs/0002-*` (wire
format) and `docs/features/0001-*` (flag design). The feature is
**fork-only**: upstream has the feature request
[casey/just#3370](https://github.com/casey/just/issues/3370), and no
PR is planned unless upstream engages. Don't burn cycles preparing
upstreamable patch series.

A second, much smaller fork-only addition rides along:
`Recipe.doc_prelude` (`docs/features/0002-*`), a field in
`just --dump --dump-format json` carrying the comment lines above a
recipe's doc-comment line that `just --list` silently discards. It is
a few dozen lines of parser plumbing serving one linter (below), not
a second reason for the fork to exist — but it is fork-only on the
same terms as `--events-fd`, and it is strictly additive: `doc`,
`--list`, and `--fmt` are unchanged, and the JSON key is omitted when
empty.

A third fork-only addition builds on `doc_prelude`: the recipe
**model**, `just --dump --dump-format model`
(`docs/features/0003-*`). It is a normalized, flat projection of the
compiled justfile — bare recipe names plus module paths, all module
recipes flattened, resolved dependency namepaths, `doc_prelude`,
groups, and source positions — built so the conformist `justfile-*`
linters read facts by field lookup instead of reconstructing them
from `--dump-format json` with jq (conformist#85, #89). It is a pure
projection in a NEW module (`src/recipe_model.rs`) with zero new
fields on the churny AST structs, so it adds little resync burden;
the schema is a versioned cross-repo contract conformist pins.

A fourth fork-only addition builds on the recipe model: `just --mcp`
(`src/mcp_serve.rs`, `docs/features/0005-*`), a minimal stdio MCP
server that answers the clown plugin protocol's dynamic
system-prompt-contribution prompt (`system-prompt-append`) with every
public recipe's namepath + doc line, unfiltered. Packaged as a clown
plugin under `plugins/just-us/` (the `just-us-clown-plugin` flake
output). This is a narrow slice of a broader, still-**proposed**
recipe-editing MCP surface (FUSE + MCP tool calls, `docs/features/0004-*`)
that has not been implemented yet.

## Versioning

The fork versions independently of upstream (its own `0.x` line, not
upstream's `1.x`), per eng-versioning(7):

- `version.env` is the single source of truth (`JUST_US_VERSION`).
- `Cargo.toml`'s `package.version` must agree; `build.rs` fails the
  build on drift. Never edit the version by hand — run
  `just bump-version <new>`, which rewrites `version.env`,
  `Cargo.toml`, and `Cargo.lock` together.
- Releases: `just release <new>` (bump, commit, signed `v*` tag,
  `gh release create`). Tags are annotated and signed.

## Justfile

Structured per eng-design_patterns-justfile(7): verb-noun leaves,
body-less aggregates, lifecycle groups. Bare `just` is the CI lane and
the spinclass pre-merge hook — if it passes, the tree is mergeable.
Notable lanes:

- `just test-bats` — authoritative bats suite in the nix sandbox;
  `just test-bats-local` for fast iteration against `target/debug`.
- `just debug-events-fd` — hand-driven `--events-fd` smoke loop.
- The `demo` group (quine, polyglot, …) is upstream heritage, kept
  as-is deliberately.
- `cargo l*` invocations (`lclippy`, `lrun`, `ltest`) need cargo-limit
  (in the devShell).

## Formatting and linting (conformist)

Formatting/linting is multiplexed through
[conformist](https://code.linenisgreat.com/conformist) (flake input;
self-describing config in `flake.nix`): rustfmt + nixfmt formatters,
shellcheck scoped to `www/install.sh`, and the eng conformance
linters (agents-md, justfile-default, justfile-orphan-summary).

- `just lint-fmt` — read-only gate (`checks.formatting`, sandboxed);
  runs in the `default` merge lane.
- `just codemod-fmt` — `nix fmt`, the modifying twin.
- rustfmt uses a native `check-command` to dodge conformist#28
  (sandbox-copy checks lose `rustfmt.toml`); drop the override when
  fixed.
- `linters.eng-versioning` stays off until conformist#29 (go.mod-only
  key derivation) supports Rust repos.
- `lint-clippy` is red on fork code (just-us#17) and lives only in the
  `ci` aggregate until clean.
- The **eight `justfile-*` conformist linters live in this repo**, not
  in conformist (`nix/linters/justfile-*.nix`): the seven that read the
  recipe model (`--dump-format model`) plus `justfile-orphan-summary`
  (`doc_prelude`). They were transplanted here because they need the
  fork's binary; conformist takes no just-us input and instead
  fixed-output-fetches just-us source to self-lint. The fork exports
  them as `lib.conformistLinters.justfile-<name>` and a
  `lib.conformistPresets.justfile` roster; the eng POLICY (the
  aggregate/leaf taxonomy, verb list) lives in `nix/justfile-model.nix`
  over the model's DATA. All eight read one shared, mandatory
  `linters.justfile-common.justPackage` (`nix/justfile-common.nix`) —
  it MUST be a just-us build or the checks fail loudly / (for
  orphan-summary) pass vacuously. See `docs/features/0003-recipe-model.md`
  ("Consumers live in the fork"); the in-tree paths there are a
  consumption contract for conformist's FOD. This repo's own `flake.nix`
  dogfoods the exported roster (`lib.conformistPresets.justfile`) with
  the fork's binary: `justfile-orphan-summary` and `justfile-default`
  (which the fork's justfile passes) stay enforced; the other six are
  opted out (`enable = false`) because the justfile carries upstream
  heritage (the `demo` group) and many test/maintenance utility recipes
  that don't fit those rules without an editorial sweep (tracked as
  just-us#23). The six rules'
  behavior is proven by `nix/justfile-linter-fixtures.nix` (33 fixtures,
  wired into `checks` + `just test-linter-fixtures`), not the dogfood.

## Upstream resyncs

Upstream is merged in periodically (`git pull` from casey/just
master). Known permanent conflict surface, resolve keeping the fork's
side unless upstream changed something load-bearing:

- `justfile` (restructured; upstream's release/publish recipes were
  removed on purpose)
- `Cargo.toml` `version` line and `Cargo.lock`'s `just` entry (fork
  semver — never take upstream's)
- `build.rs` (carries the version.env drift guard alongside
  upstream's Windows stack-size logic)
- `.github/workflows/release.yaml` (trigger defanged to
  `workflow_dispatch`; upstream's pipeline targets casey/just and must
  not fire on fork tags)
- `src/` events-fd implementation and `zz-tests_bats/` (fork-only
  feature code)
- `src/parser.rs`, `src/recipe.rs`, `src/unresolved_recipe.rs` (the
  `doc_prelude` field and the parser's prelude capture — fork-only,
  and in three files upstream churns constantly; the `Recipe` struct
  literals in particular conflict on any upstream field addition)
- `src/recipe_model.rs` and `tests/model.rs` (the recipe model —
  fork-only, new files, so they don't conflict; but `src/lib.rs`
  (module declaration + re-export), `src/dump_format.rs` (the `Model`
  variant), and `src/subcommand.rs` (the dump match arm) each carry a
  small fork insertion that can conflict on upstream churn)

## CI / workflows

`.github/workflows/ci.yaml` invokes cargo directly (not justfile
recipes), so recipe renames don't affect it. `release.yaml` is
manual-dispatch only — fork releases don't ship artifacts; consumers
build via the flake (`packages.default`).
