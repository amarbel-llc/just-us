# bats file_tags=mcp
#
# `just --mcp`'s `tools` capability (docs/features/0006):
# `list_recipes`/`show_recipe`/`run_recipe`/`dump_justfile`/`list_variables`
# on the same stdio MCP server that already answers `prompts/get
# system-prompt-append` (docs/features/0005). `run_recipe` always
# executes as a real subprocess (wrapped in `nix develop -c` when a
# flake.nix is present) rather than in-process, so it can support
# `impure`/`timeout`/`async` uniformly — see the 0006 addendum. Devshell
# wrapping and full async job-completion/wake behavior have real
# environmental dependencies (a real flake.nix + network, a live clown
# session) that don't fit this hermetic sandbox well; they are verified
# by manual smoke test instead (see the addendum's Verification
# section). What's covered here: sync execution still works, timeout
# kills a long-running recipe, and async returns a job id promptly via a
# real `ringmaster start` call (on PATH in this sandbox — see bats.nix)
# without waiting for the recipe to finish.

setup() {
  load "$(dirname "$BATS_TEST_FILE")/common.bash"
  setup_test_home
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
