# bats file_tags=mcp
#
# `just --mcp`'s `tools` capability (docs/features/0006):
# `list_recipes`/`show_recipe`/`run_recipe`/`dump_justfile`/`list_variables`
# on the same stdio MCP server that already answers `prompts/get
# system-prompt-append` (docs/features/0005). `run_recipe` always
# executes as a real subprocess (wrapped in `nix develop -c` when a
# flake.nix is present) rather than in-process, so it can support
# `impure`/`timeout`/`async` uniformly — see the 0006 addendum. A real
# `nix develop` build and full async job-completion/wake behavior have
# real environmental dependencies (network, a live clown session) that
# don't fit this hermetic sandbox well; they are verified by manual
# smoke test instead (see the addendum's Verification section). What's
# covered here: sync execution still works, timeout kills a
# long-running recipe, async returns a job id promptly via a real
# `ringmaster start` call (on PATH in this sandbox — see bats.nix)
# without waiting for the recipe to finish, and — via a stub `nix` on
# PATH that records its own argv and execs straight through to the real
# `just` binary — *which directory* `run_recipe` picks as the devshell
# installable for a child-justfile recipe, without needing a real flake
# evaluation to prove it.

setup() {
  load "$(dirname "$BATS_TEST_FILE")/common.bash"
  setup_test_home
}

# Shadows `nix` on PATH with a stub that records its own argv (`develop
# <flake_dir> [--impure] -c just <invocation>`) to $NIX_STUB_LOG, then
# execs straight through to the real just binary — so run_recipe's
# devshell-selection logic (which directory it hands `nix develop` as
# the installable) can be asserted without a real flake evaluation,
# which needs network and doesn't fit this hermetic sandbox.
install_nix_stub() {
  export JUST_BIN="${JUST_BIN:-just}"
  mkdir -p fakebin
  export NIX_STUB_LOG="$PWD/nix-stub.log"

  # `#!/usr/bin/env bash` doesn't resolve inside the nix build sandbox
  # (bats-mcp), which has no /usr/bin/env -- use $BASH (this test is
  # itself running under the very bash that's already on PATH here) as
  # an absolute, always-resolvable shebang instead.
  { echo "#!$BASH"; cat <<'STUB'
printf '%s\n' "$*" > "$NIX_STUB_LOG"
shift            # drop "develop"
shift            # drop the flake_dir installable
if [[ $1 == --impure ]]; then shift; fi
shift            # drop "-c"
shift            # drop the literal "just" recipe_command hardcodes
exec "${JUST_BIN:-just}" "$@"
STUB
  } > fakebin/nix

  chmod +x fakebin/nix
  export PATH="$PWD/fakebin:$PATH"
}

@test "--mcp: tools/list advertises list_recipes, show_recipe, run_recipe" {
  cat > justfile <<'EOF'
build:
    @echo build
EOF

  run timeout --preserve-status 5s bash -c '"$0" --mcp <<<"$1"' "${JUST_BIN:-just}" \
    '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'
  assert_success

  [[ $output == *'"list_recipes"'* ]] || fail "tools/list missing list_recipes: $output"
  [[ $output == *'"show_recipe"'* ]] || fail "tools/list missing show_recipe: $output"
  [[ $output == *'"run_recipe"'* ]] || fail "tools/list missing run_recipe: $output"
  [[ $output == *'"dump_justfile"'* ]] || fail "tools/list missing dump_justfile: $output"
  [[ $output == *'"list_variables"'* ]] || fail "tools/list missing list_variables: $output"
}

@test "--mcp: dump_justfile returns the full compiled justfile" {
  cat > justfile <<'EOF'
build:
    @echo build
EOF

  run timeout --preserve-status 5s bash -c '"$0" --mcp <<<"$1"' "${JUST_BIN:-just}" \
    '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"dump_justfile"}}'
  assert_success
  [[ $output == *'\"recipes\"'* ]] || fail "dump_justfile missing recipes key: $output"
  [[ $output == *'\"build\"'* ]] || fail "dump_justfile missing the build recipe: $output"
}

@test "--mcp: list_variables resolves public top-level variable values" {
  cat > justfile <<'EOF'
foo := "bar"

_hidden := "nope"

build:
    @echo build
EOF

  run timeout --preserve-status 5s bash -c '"$0" --mcp <<<"$1"' "${JUST_BIN:-just}" \
    '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_variables"}}'
  assert_success
  [[ $output == *'\"name\":\"foo\"'* ]] || fail "list_variables missing foo: $output"
  [[ $output == *'\"value\":\"bar\"'* ]] || fail "list_variables did not resolve foo's value: $output"
  [[ $output != *'_hidden'* ]] || fail "list_variables leaked a private variable: $output"
}

@test "--mcp: list_recipes/show_recipe exclude private recipes" {
  cat > justfile <<'EOF'
build:
    @echo build

_hidden:
    @echo hidden
EOF

  requests=$(printf '%s\n%s\n%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_recipes"}}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"show_recipe","arguments":{"recipe":"build"}}}' \
    '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"show_recipe","arguments":{"recipe":"_hidden"}}}')

  run timeout --preserve-status 5s bash -c '"$0" --mcp <<<"$1"' "${JUST_BIN:-just}" "$requests"
  assert_success

  first_reply=$(echo "$output" | sed -n '1p')
  second_reply=$(echo "$output" | sed -n '2p')
  third_reply=$(echo "$output" | sed -n '3p')

  [[ $first_reply == *'\"namepath\":\"build\"'* ]] || fail "list_recipes missing build: $first_reply"
  [[ $first_reply != *'_hidden'* ]] || fail "list_recipes leaked private recipe: $first_reply"

  [[ $second_reply == *'\"namepath\":\"build\"'* ]] || fail "show_recipe(build) missing model: $second_reply"

  [[ $third_reply == *'"isError":true'* ]] || fail "show_recipe(_hidden) should error: $third_reply"
}

@test "--mcp: list_recipes also enumerates other justfiles in the repo tree" {
  # Reproduces the retired just-us-agents moxin's own list-recipes
  # behavior (find . -mindepth 2 -maxdepth 3 -name justfile, skipping
  # .git/.worktrees/.claude) for retirement parity.
  cat > justfile <<'EOF'
root_recipe:
    @echo root
EOF

  mkdir -p a/b/c .git broken

  cat > a/justfile <<'EOF'
depth2_recipe:
    @echo depth2
EOF

  cat > a/b/justfile <<'EOF'
depth3_recipe:
    @echo depth3

_hidden_depth3:
    @echo hidden
EOF

  # Out of range (depth 4) -- must not appear.
  cat > a/b/c/justfile <<'EOF'
depth4_recipe:
    @echo depth4
EOF

  # Inside a pruned directory -- must never even be looked at.
  cat > .git/justfile <<'EOF'
should_never_appear:
    @echo nope
EOF

  # Fails to compile -- must be skipped, not fail the whole call.
  cat > broken/justfile <<'EOF'
this is not valid just syntax {{{
EOF

  run timeout --preserve-status 5s bash -c '"$0" --mcp <<<"$1"' "${JUST_BIN:-just}" \
    '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_recipes"}}'
  assert_success

  [[ $output == *'\"namepath\":\"root_recipe\"'* ]] || fail "missing root recipe: $output"
  [[ $output == *'\"namepath\":\"a/depth2_recipe\"'* ]] || fail "missing depth-2 child recipe: $output"
  [[ $output == *'\"namepath\":\"a/b/depth3_recipe\"'* ]] || fail "missing depth-3 child recipe: $output"
  [[ $output != *'depth4_recipe'* ]] || fail "depth-4 child recipe should be out of range: $output"
  [[ $output != *'should_never_appear'* ]] || fail ".git should be pruned, never descended into: $output"
  [[ $output != *'_hidden_depth3'* ]] || fail "private recipes in a child justfile should still be excluded: $output"
  [[ $output != *'"isError"'* ]] || fail "a broken child justfile should be skipped, not fail the call: $output"
}

@test "--mcp: list_recipes is compact by default, verbose opts into the full model" {
  cat > justfile <<'EOF'
# builds the thing
build name:
    @echo build {{name}}
EOF

  requests=$(printf '%s\n%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_recipes"}}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_recipes","arguments":{"verbose":true}}}')

  run timeout --preserve-status 5s bash -c '"$0" --mcp <<<"$1"' "${JUST_BIN:-just}" "$requests"
  assert_success

  compact_reply=$(echo "$output" | sed -n '1p')
  verbose_reply=$(echo "$output" | sed -n '2p')

  [[ $compact_reply == *'\"namepath\":\"build\"'* ]] || fail "compact list_recipes missing namepath: $compact_reply"
  [[ $compact_reply == *'\"parameters\":[\"name\"]'* ]] || fail "compact list_recipes missing parameters: $compact_reply"
  [[ $compact_reply != *'doc_prelude'* ]] || fail "compact list_recipes should not include full-model-only fields: $compact_reply"
  [[ $compact_reply != *'\"source\"'* ]] || fail "compact list_recipes should not include source: $compact_reply"

  [[ $verbose_reply == *'\"doc_prelude\":[]'* ]] || fail "verbose list_recipes missing full model fields: $verbose_reply"
  [[ $verbose_reply == *'\"source\":\"justfile\"'* ]] || fail "verbose list_recipes missing source: $verbose_reply"
}

@test "--mcp: list_recipes/show_recipe max_depth is overridable" {
  cat > justfile <<'EOF'
root_recipe:
    @echo root
EOF

  mkdir -p a/b/c

  cat > a/b/c/justfile <<'EOF'
depth4_recipe:
    @echo depth4
EOF

  requests=$(printf '%s\n%s\n%s\n%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_recipes"}}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_recipes","arguments":{"max_depth":4}}}' \
    '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"show_recipe","arguments":{"recipe":"a/b/c/depth4_recipe"}}}' \
    '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"show_recipe","arguments":{"recipe":"a/b/c/depth4_recipe","max_depth":4}}}')

  run timeout --preserve-status 5s bash -c '"$0" --mcp <<<"$1"' "${JUST_BIN:-just}" "$requests"
  assert_success

  default_list=$(echo "$output" | sed -n '1p')
  deep_list=$(echo "$output" | sed -n '2p')
  default_show=$(echo "$output" | sed -n '3p')
  deep_show=$(echo "$output" | sed -n '4p')

  [[ $default_list != *'depth4_recipe'* ]] || fail "default max_depth should not reach depth 4: $default_list"
  [[ $deep_list == *'\"namepath\":\"a/b/c/depth4_recipe\"'* ]] || fail "max_depth:4 should reach the depth-4 recipe: $deep_list"

  [[ $default_show == *'"isError":true'* ]] || fail "show_recipe at default depth should not resolve a depth-4 recipe: $default_show"
  [[ $deep_show == *'\"namepath\":\"a/b/c/depth4_recipe\"'* ]] || fail "show_recipe with max_depth:4 should resolve it: $deep_show"
}

@test "--mcp: show_recipe resolves a recipe in a child justfile" {
  cat > justfile <<'EOF'
root_recipe:
    @echo root
EOF

  mkdir -p a

  cat > a/justfile <<'EOF'
# a child recipe
depth2_recipe name:
    @echo depth2 {{name}}
EOF

  run timeout --preserve-status 5s bash -c '"$0" --mcp <<<"$1"' "${JUST_BIN:-just}" \
    '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"show_recipe","arguments":{"recipe":"a/depth2_recipe"}}}'
  assert_success

  [[ $output == *'\"namepath\":\"a/depth2_recipe\"'* ]] || fail "show_recipe did not resolve the child recipe: $output"
  [[ $output == *'\"doc\":\"a child recipe\"'* ]] || fail "show_recipe missing the child recipe's doc: $output"
  [[ $output != *'"isError"'* ]] || fail "resolving a real child recipe should not be an error: $output"
}

@test "--mcp: run_recipe captures stdout and reports success" {
  cat > justfile <<'EOF'
greet name:
    echo "hello {{name}}"
EOF

  run timeout --preserve-status 5s bash -c '"$0" --mcp <<<"$1"' "${JUST_BIN:-just}" \
    '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"run_recipe","arguments":{"recipe":"greet","args":["world"]}}}'
  assert_success

  [[ $output == *'hello world'* ]] || fail "run_recipe did not capture recipe stdout: $output"
  [[ $output != *'"isError"'* ]] || fail "successful run_recipe should not set isError: $output"
}

@test "--mcp: run_recipe reports a failing recipe as isError without crashing the server" {
  cat > justfile <<'EOF'
fail:
    exit 3

build:
    @echo build
EOF

  requests=$(printf '%s\n%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"run_recipe","arguments":{"recipe":"fail"}}}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"run_recipe","arguments":{"recipe":"build"}}}')

  # `fail`/`build` aren't `@`-quiet, so `just` echoes each recipe's
  # command line to its own real stderr before running it (unrelated to
  # the captured tool-call output). `--separate-stderr` keeps that out of
  # $output, isolating the JSON-RPC stdout stream the way a real MCP
  # client reads it.
  run --separate-stderr timeout --preserve-status 5s bash -c '"$0" --mcp <<<"$1"' "${JUST_BIN:-just}" "$requests"
  assert_success

  first_reply=$(echo "$output" | sed -n '1p')
  second_reply=$(echo "$output" | sed -n '2p')

  [[ $first_reply == *'"isError":true'* ]] || fail "failing recipe should set isError: $first_reply"
  [[ $first_reply == *'exit code 3'* ]] || fail "failure message missing exit code: $first_reply"

  [[ $second_reply == *'"id":2'* ]] || fail "server did not answer the request after a recipe failure: $second_reply"
  [[ $second_reply != *'"isError"'* ]] || fail "unrelated successful run_recipe should not set isError: $second_reply"
}

@test "--mcp: run_recipe against an unknown recipe is isError, not a crash" {
  cat > justfile <<'EOF'
build:
    @echo build
EOF

  run timeout --preserve-status 5s bash -c '"$0" --mcp <<<"$1"' "${JUST_BIN:-just}" \
    '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"run_recipe","arguments":{"recipe":"nope"}}}'
  assert_success
  [[ $output == *'"isError":true'* ]] || fail "unknown recipe should set isError: $output"
}

@test "--mcp: run_recipe timeout kills a long-running recipe" {
  cat > justfile <<'EOF'
slow:
    echo starting
    sleep 5
EOF

  run timeout --preserve-status 10s bash -c '"$0" --mcp <<<"$1"' "${JUST_BIN:-just}" \
    '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"run_recipe","arguments":{"recipe":"slow","timeout":"1s"}}}'
  assert_success
  [[ $output == *'"isError":true'* ]] || fail "timed-out recipe should set isError: $output"
  [[ $output == *'timed out'* ]] || fail "timeout message missing: $output"
}

@test "--mcp: run_recipe async returns a job id promptly without waiting for completion" {
  cat > justfile <<'EOF'
slow:
    sleep 5
EOF

  # Keep stdin open past the response so the server (and its detached
  # async thread) doesn't exit the instant this one line is answered —
  # a process exit kills any in-flight background job, same as any
  # other process. Real clown-hosted usage keeps stdin open for the
  # session's lifetime; this only needs to outlive the tool call itself,
  # not the recipe's full 5s runtime.
  run --separate-stderr timeout --preserve-status 5s bash -c \
    '(printf "%s\n" "$1"; sleep 2) | "$0" --mcp' "${JUST_BIN:-just}" \
    '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"run_recipe","arguments":{"recipe":"slow","async":true}}}'
  assert_success
  [[ $output == *'\"job_id\"'* ]] || fail "async run_recipe did not return a job_id: $output"
  [[ $output != *'"isError"'* ]] || fail "dispatching an async job should not itself be an error: $output"
}

@test "--mcp: run_recipe resolves a child justfile's own directory as cwd" {
  # A dir/recipe namepath must actually run from that directory, not the
  # caller's (originally reported against eng/circus). No flake.nix
  # anywhere in this fixture, so this only exercises the
  # plain-re-invocation path -- the devshell-selection tests below cover
  # the nix-wrapped path via the stub.
  cat > justfile <<'EOF'
root_recipe:
    @echo root
EOF

  mkdir -p a
  echo "child marker" > a/marker.txt

  cat > a/justfile <<'EOF'
show_marker:
    @cat marker.txt
EOF

  run timeout --preserve-status 5s bash -c '"$0" --mcp <<<"$1"' "${JUST_BIN:-just}" \
    '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"run_recipe","arguments":{"recipe":"a/show_marker"}}}'
  assert_success

  [[ $output == *'child marker'* ]] || fail "run_recipe did not run from the child justfile's own directory: $output"
  [[ $output == *'cwd='*'/a, devshell=none'* ]] || fail "run_recipe did not report the resolved cwd/devshell: $output"
}

@test "--mcp: run_recipe wraps a child justfile in its own flake.nix, not the caller's" {
  install_nix_stub

  cat > justfile <<'EOF'
root_recipe:
    @echo root
EOF
  echo '# root flake -- must not be selected' > flake.nix

  mkdir -p a
  cat > a/justfile <<'EOF'
build:
    @echo built-in-child
EOF
  echo '# child flake -- must be selected' > a/flake.nix

  run timeout --preserve-status 5s bash -c '"$0" --mcp <<<"$1"' "$JUST_BIN" \
    '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"run_recipe","arguments":{"recipe":"a/build"}}}'
  assert_success

  [[ $output == *'built-in-child'* ]] || fail "run_recipe did not execute the child recipe: $output"

  flake_arg=$(awk '{print $2}' "$NIX_STUB_LOG")
  [[ $flake_arg == "$PWD/a" ]] || fail "nix should have been given the child's own directory ($PWD/a) as the installable, got: $flake_arg"
}

@test "--mcp: run_recipe falls back to the root's flake.nix when the child justfile has none" {
  install_nix_stub

  cat > justfile <<'EOF'
root_recipe:
    @echo root
EOF
  echo '# root flake -- the only one available' > flake.nix

  mkdir -p a
  echo "child marker" > a/marker.txt
  cat > a/justfile <<'EOF'
show_marker:
    @cat marker.txt
EOF

  run timeout --preserve-status 5s bash -c '"$0" --mcp <<<"$1"' "$JUST_BIN" \
    '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"run_recipe","arguments":{"recipe":"a/show_marker"}}}'
  assert_success

  # The devshell falls back to root, but the recipe itself must still
  # run from the child's own directory -- these are decoupled.
  [[ $output == *'child marker'* ]] || fail "run_recipe did not run from the child justfile's own directory despite the devshell fallback: $output"

  flake_arg=$(awk '{print $2}' "$NIX_STUB_LOG")
  [[ $flake_arg == "$PWD" ]] || fail "nix should have fallen back to the root directory ($PWD) as the installable, got: $flake_arg"
}

@test "--mcp: run_recipe reaches a recipe deeper than list_recipes's default max_depth" {
  # run_recipe has no depth limit of its own -- it defers entirely to
  # just's own native, unbounded search-directory resolution (see
  # flake_target_dir's doc comment) rather than a bounded filesystem
  # walk. list_recipes's max_depth exists only to bound *its own*
  # enumeration output size; it was never meant to gate what run_recipe
  # can execute, so a recipe list_recipes wouldn't surface without
  # raising max_depth should still be directly runnable.
  cat > justfile <<'EOF'
root_recipe:
    @echo root
EOF

  mkdir -p a/b/c
  cat > a/b/c/justfile <<'EOF'
depth4_recipe:
    @echo depth4
EOF

  requests=$(printf '%s\n%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_recipes"}}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"run_recipe","arguments":{"recipe":"a/b/c/depth4_recipe"}}}')

  run timeout --preserve-status 5s bash -c '"$0" --mcp <<<"$1"' "${JUST_BIN:-just}" "$requests"
  assert_success

  list_reply=$(echo "$output" | sed -n '1p')
  run_reply=$(echo "$output" | sed -n '2p')

  [[ $list_reply != *'depth4_recipe'* ]] || fail "list_recipes's default max_depth should not surface the depth-4 recipe: $list_reply"
  [[ $run_reply == *'depth4'* ]] || fail "run_recipe should reach and run the depth-4 recipe despite list_recipes's default depth limit: $run_reply"
  [[ $run_reply != *'"isError"'* ]] || fail "the depth-4 recipe should run successfully: $run_reply"
}
