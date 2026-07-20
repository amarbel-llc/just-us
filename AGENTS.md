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
linters (agents-md, justfile-default).

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

## CI / workflows

`.github/workflows/ci.yaml` invokes cargo directly (not justfile
recipes), so recipe renames don't affect it. `release.yaml` is
manual-dispatch only — fork releases don't ship artifacts; consumers
build via the flake (`packages.default`).
