#!/usr/bin/env -S just --justfile
# ^ A shebang isn't required, but allows a justfile to be executed
#   like a script, with `./justfile test`, for example.

alias t := test

log := "warn"

export JUST_LOG := log

# CI-equivalent entrypoint (and the spinclass pre-merge lane): if bare
# `just` passes, the tree is mergeable. Aggregates only, per
# eng-design_patterns-justfile(7).
default: build test

[group: 'dev']
watch +args='ltest':
  cargo watch --clear --exec '{{ args }}'

[group: 'test']
test: test-direct test-bats

# XDG_CONFIG_HOME is scrubbed because upstream's tests/global.rs unix
# test isolates HOME but not XDG_CONFIG_HOME, so a real
# ~/.config/just/justfile leaks in and fails the suite.
[group: 'test']
test-direct *args='--all':
  env -u XDG_CONFIG_HOME cargo test {{args}}

# Authoritative bats lane — nix sandbox, every file_tag.
[group: 'test']
test-bats:
  nix build .#bats-default --no-link --print-build-logs

# Bats lane filtered to a single file_tag.
[group: 'test']
test-bats-tags *tags:
  nix build .#bats-{{tags}} --no-link --print-build-logs

# debug: run target/debug/just (see build-direct) with --events-fd against a
# self-provisioned scratch justfile; events drain to stdout (fd 3), child
# output and chrome go to stderr. RFC 0002 smoke loop for agents.
[group: 'debug']
debug-events-fd *args='hello':
  @mkdir -p .tmp/events-test
  @printf 'hello: dep\n  @echo hello-from-recipe\n\ndep:\n  @echo dep-output\n' > .tmp/events-test/justfile
  bash -c 'exec 3>&1 1>&2; ./target/debug/just --events-fd 3 --justfile .tmp/events-test/justfile {{args}}'

# Fast iteration — runs against the locally-built binary in target/debug.
# Run `just build-direct` first to populate target/debug/just.
[group: 'test']
test-bats-local *targets='*.bats':
  JUST_BIN=$(realpath ./target/debug/just) \
    BATS_TEST_TIMEOUT=10 \
    bats --jobs $(nproc) zz-tests_bats/{{targets}}

[group: 'check']
ci: test clippy build-book forbid
  cargo fmt --all -- --check
  cargo update --locked --package just

[group: 'check']
fuzz:
  cargo +nightly fuzz run fuzz-compiler

[group: 'misc']
run:
  cargo lrun

# only run tests matching `PATTERN`
[group: 'test']
filter PATTERN:
  cargo ltest {{PATTERN}}

[group: 'misc']
build: build-direct

[group: 'misc']
build-direct:
  cargo build

[group: 'misc']
fmt:
  cargo fmt --all

[group: 'check']
shellcheck:
  shellcheck www/install.sh

[group: 'doc']
man:
  mkdir -p man
  cargo lrun -- --man > man/just.1

[group: 'doc']
view-man: man
  man man/just.1

# add git log messages to changelog
[group: 'release']
update-changelog:
  echo >> CHANGELOG.md
  git log --pretty='format:- %s' >> CHANGELOG.md

[group: 'release']
update-contributors:
  cargo lrun --release --package update-contributors

[group: 'check']
action-versions:
  cargo lrun --package action-versions

[group: 'check']
outdated:
  cargo outdated -R

[group: 'check']
unused:
  cargo +nightly udeps --workspace

# publish current GitHub master branch
[group: 'release']
publish:
  #!/usr/bin/env bash
  set -euxo pipefail
  rm -rf tmp/release
  git clone --depth 1 git@github.com:casey/just.git tmp/release
  cd tmp/release
  ! grep '<sup>master</sup>' README.md
  VERSION=`sed -En 's/version[[:space:]]*=[[:space:]]*"([^"]+)"/\1/p' Cargo.toml | head -1`
  git tag -a $VERSION -m "Release $VERSION"
  git push origin $VERSION
  cargo publish
  cd ../..
  rm -rf tmp/release

[group: 'release']
readme-version-notes:
  grep '<sup>master</sup>' README.md

# clean up feature branch BRANCH
[group: 'dev']
done BRANCH=`git rev-parse --abbrev-ref HEAD`:
  git checkout master
  git diff --no-ext-diff --quiet --exit-code
  git pull --rebase github master
  git diff --no-ext-diff --quiet --exit-code {{BRANCH}}
  git branch -D {{BRANCH}}

# install just from crates.io
[group: 'misc']
install:
  cargo install -f just

# install development dependencies
[group: 'dev']
install-dev-deps:
  rustup install nightly
  rustup update nightly
  cargo +nightly install cargo-fuzz
  cargo install cargo-limit
  cargo install cargo-watch
  cargo install --locked mdbook@0.4.52
  cargo install --locked mdbook-linkcheck@0.7.7

# everyone's favorite animate paper clip
[group: 'check']
clippy:
  cargo lclippy --all --all-targets --all-features -- --deny warnings

[group: 'check']
forbid:
  ./bin/forbid

[group: 'dev']
replace FROM TO:
  sd '{{FROM}}' '{{TO}}' src/*.rs

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

[group: 'check']
build-book:
  cargo lrun --package generate-book
  mdbook build book/en
  mdbook build book/zh

[group: 'dev']
print-readme-constants-table:
  cargo ltest constants::tests::readme_table -- --nocapture

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

# Print working directory, for demonstration purposes!
[group: 'demo']
pwd:
  echo {{invocation_directory()}}

[group: 'test']
test-release-workflow:
  -git tag -d test-release
  -git push origin :test-release
  git tag test-release
  git push origin test-release

[group: 'test']
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

rule110:
  just -f examples/rule110.just

# Local Variables:
# mode: makefile
# End:
