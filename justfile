#!/usr/bin/env -S just --justfile
# ^ A shebang isn't required, but allows a justfile to be executed
#   like a script, with `./justfile test`, for example.
#
# Structured per eng-design_patterns-justfile(7): verb-noun leaf
# recipes, body-less aggregates, lifecycle groups. The `demo` group is
# upstream (casey/just) heritage and is intentionally left as-is.

alias t := test

log := "warn"

export JUST_LOG := log

# Flake output system tuple, portable across linux/darwin hosts.
nix-system := arch() + "-" + if os() == "macos" { "darwin" } else { "linux" }

# CI-equivalent entrypoint (and the spinclass pre-merge lane), aggregates only:
# if bare `just` passes, the tree is mergeable
default: validate lint build test

# everything CI checks beyond `default`: clippy, forbid, lockfile, book build
ci: lint lint-clippy lint-forbid validate-lockfile test build-book

[group: 'pre-build']
validate: validate-devshell

# Catches flake/devshell breakage that the cargo build can mask. No
# store output kept (--no-link) --- it's a build-check, not an artifact.
# verify the devShell evaluates and builds without errors
[group: 'pre-build']
validate-devshell:
  nix build --no-link .#devShells.{{ nix-system }}.default

# verify Cargo.lock is in sync with Cargo.toml
[group: 'pre-build']
validate-lockfile:
  cargo update --locked --package just

# lint-clippy and lint-forbid hang off `ci` instead because clippy is
# red on fork code (just-us#17) and forbid needs rg from the devshell;
# fold them back into this aggregate once #17 lands.
# hermetic read-only lint gate (conformist only, for now)
[group: 'pre-build']
lint: lint-fmt

# everyone's favorite animate paper clip
[group: 'pre-build']
lint-clippy:
  cargo lclippy --all --all-targets --all-features -- --deny warnings

# Builds the conformist `checks.formatting` derivation, which runs every
# formatter and linter (rustfmt, nixfmt, shellcheck, eng conformance)
# against a /nix/store snapshot of the tree and fails if anything would
# change. Does NOT modify the worktree --- `codemod-fmt` is the
# modifying twin.
# read-only formatting/linting gate via conformist
[group: 'pre-build']
lint-fmt:
  nix build .#checks.{{ nix-system }}.formatting --no-link --print-build-logs

[group: 'pre-build']
lint-forbid:
  ./bin/forbid

# Not in the `lint` aggregate: this is upstream-maintenance, not a
# per-merge gate.
# check that GitHub Actions pins in the workflows are current
[group: 'pre-build']
lint-action-versions:
  cargo lrun --package action-versions

[group: 'build']
build: build-cargo

[group: 'build']
build-cargo:
  cargo build

[group: 'build']
build-book:
  cargo lrun --package generate-book
  mdbook build book/en
  mdbook build book/zh

[group: 'build']
build-man:
  mkdir -p man
  cargo lrun -- --man > man/just.1

[group: 'post-build']
test: test-cargo test-bats

# XDG_CONFIG_HOME is scrubbed because upstream's tests/global.rs unix
# test isolates HOME but not XDG_CONFIG_HOME, so a real
# ~/.config/just/justfile leaks in and fails the suite.
# run the cargo test suite
[group: 'post-build']
test-cargo *args='--all':
  env -u XDG_CONFIG_HOME cargo test {{args}}

# Authoritative bats lane — nix sandbox, every file_tag.
[group: 'post-build']
test-bats:
  nix build .#bats-default --no-link --print-build-logs

# Bats lane filtered to a single file_tag.
[group: 'post-build']
test-bats-tags *tags:
  nix build .#bats-{{tags}} --no-link --print-build-logs

# Run `just build-cargo` first to populate target/debug/just.
# fast bats iteration against the locally-built binary in target/debug
[group: 'post-build']
test-bats-local *targets='*.bats':
  JUST_BIN=$(realpath ./target/debug/just) \
    BATS_TEST_TIMEOUT=10 \
    bats --jobs $(nproc) zz-tests_bats/{{targets}}

# only run cargo tests matching `PATTERN`
[group: 'post-build']
test-filter PATTERN:
  cargo ltest {{PATTERN}}

[group: 'post-build']
test-fuzz:
  cargo +nightly fuzz run fuzz-compiler

[group: 'post-build']
test-completions:
  #!/usr/bin/env bash
  rm -rf tmp/complete
  mkdir -p tmp/complete/bin
  cargo lbuild
  cp target/debug/just tmp/complete/bin
  ./tmp/complete/bin/just --completions bash > tmp/complete/just.bash
  cat > tmp/complete/justfile << EOF
  alias hello := foo::bar
  mod foo
  EOF
  echo 'bar:' > tmp/complete/foo.just
  cd tmp/complete && PATH="`realpath bin`:$PATH" bash --init-file just.bash

[group: 'codemod']
codemod-fmt: codemod-fmt-conformist

# rewrite the worktree with every formatter and linter repair (nix fmt)
[group: 'codemod']
codemod-fmt-conformist:
  nix fmt

[group: 'codemod']
codemod-replace FROM TO:
  sd '{{FROM}}' '{{TO}}' src/*.rs

[group: 'inspection']
view-man: build-man
  man man/just.1

[group: 'inspection']
list-outdated:
  cargo outdated -R

[group: 'inspection']
list-unused:
  cargo +nightly udeps --workspace

[group: 'inspection']
list-readme-constants:
  cargo ltest constants::tests::readme_table -- --nocapture

# Runs target/debug/just (see build-cargo) against a self-provisioned
# scratch justfile; events drain to stdout (fd 3), child output and
# chrome go to stderr.
# RFC 0002 smoke loop for agents: exercise --events-fd by hand
[group: 'debug']
debug-events-fd *args='hello':
  @mkdir -p .tmp/events-test
  @printf 'hello: dep\n  @echo hello-from-recipe\n\ndep:\n  @echo dep-output\n' > .tmp/events-test/justfile
  bash -c 'exec 3>&1 1>&2; ./target/debug/just --events-fd 3 --justfile .tmp/events-test/justfile {{args}}'

[group: 'debug']
watch-cargo +args='ltest':
  cargo watch --clear --exec '{{ args }}'

[group: 'debug']
run:
  cargo lrun

# build.rs guards against drift between these three files.
# rewrite the fork version in version.env, Cargo.toml, and Cargo.lock
[group: 'maintenance']
bump-version new_version:
  sed -E -i 's/^(export JUST_US_VERSION)=.*/\1={{ new_version }}/' version.env
  sed -E -i '0,/^version = ".*"$/s//version = "{{ new_version }}"/' Cargo.toml
  cargo update --workspace

# create and push a signed annotated v* tag from version.env
[group: 'maintenance']
tag $message:
  #!/usr/bin/env bash
  set -euo pipefail
  . version.env
  tag="v${JUST_US_VERSION:?missing JUST_US_VERSION in version.env}"
  git tag -s -m "$message" "$tag"
  echo "created tag: $tag"
  git push origin "$tag"
  echo "pushed $tag"
  git tag -v "$tag"

# The changelog is generated BEFORE bump-version so the release-bump
# commit doesn't appear in its own changelog.
# cut a fork release: bump, commit, tag, gh release create
[group: 'maintenance']
release new_version:
  #!/usr/bin/env bash
  set -euo pipefail
  branch=$(git rev-parse --abbrev-ref HEAD)
  if [[ "$branch" != "master" ]]; then
    echo "release only allowed from master (on '$branch')" >&2
    exit 1
  fi
  prev=$(git tag --sort=-v:refname -l 'v*' | head -1)
  header="release v{{ new_version }}"
  if [[ -n "$prev" ]]; then
    summary=$(git log --format='- %s' "$prev"..HEAD)
    msg="$header"$'\n\n'"$summary"
  else
    msg="$header"
  fi
  just bump-version "{{ new_version }}"
  git add version.env Cargo.toml Cargo.lock
  git commit -m "$header"
  just tag "$msg"
  gh release create "v{{ new_version }}" --title "$header" --notes "$msg"

# add git log messages to changelog
[group: 'maintenance']
update-changelog:
  echo >> CHANGELOG.md
  git log --pretty='format:- %s' >> CHANGELOG.md

[group: 'maintenance']
update-contributors:
  cargo lrun --release --package update-contributors

# install development dependencies
[group: 'operational']
install-dev-deps:
  rustup install nightly
  rustup update nightly
  cargo +nightly install cargo-fuzz
  cargo install cargo-limit
  cargo install cargo-watch
  cargo install --locked mdbook@0.4.52
  cargo install --locked mdbook-linkcheck@0.7.7

# run all polyglot recipes
[group: 'demo']
polyglot: _python _js _perl _sh _ruby

_python:
  #!/usr/bin/env python3
  print('Hello from python!')

_js:
  #!/usr/bin/env node
  console.log('Greetings from JavaScript!')

_perl:
  #!/usr/bin/env perl
  print "Larry Wall says Hi!\n";

_sh:
  #!/usr/bin/env sh
  hello='Yo'
  echo "$hello from a shell script!"

_nu:
  #!/usr/bin/env nu
  let hellos = ["Greetings", "Yo", "Howdy"]
  $hellos | each {|el| print $"($el) from a nushell script!" }

_ruby:
  #!/usr/bin/env ruby
  puts "Hello from ruby!"

[group: 'demo']
test-quine:
  cargo lrun -- quine

# make a quine, compile it, and verify it
[group: 'demo']
quine:
  mkdir -p tmp
  @echo '{{quine-text}}' > tmp/gen0.c
  cc tmp/gen0.c -o tmp/gen0
  ./tmp/gen0 > tmp/gen1.c
  cc tmp/gen1.c -o tmp/gen1
  ./tmp/gen1 > tmp/gen2.c
  diff tmp/gen1.c tmp/gen2.c
  rm -r tmp
  @echo 'It was a quine!'

quine-text := '
  int printf(const char*, ...);

  int main() {
    char *s =
      "int printf(const char*, ...);"
      "int main() {"
      "   char *s = %c%s%c;"
      "  printf(s, 34, s, 34);"
      "  return 0;"
      "}";
    printf(s, 34, s, 34);
    return 0;
  }
'

# Print working directory, for demonstration purposes!
[group: 'demo']
pwd:
  echo {{invocation_directory()}}

[group: 'demo']
rule110:
  just -f examples/rule110.just

# Local Variables:
# mode: makefile
# End:
