# bats file_tags=mcp
#
# `just --mcp`'s `tools` capability (docs/features/0006):
# `list_recipes`/`show_recipe`/`run_recipe` on the same stdio MCP server
# that already answers `prompts/get system-prompt-append`
# (docs/features/0005). `run_recipe` reuses the `--events-fd` capture
# path (RFC 0002) with an in-memory sink instead of a real fd, so a
# recipe's child stdout/stderr never leaks onto the server's own
# stdout — the JSON-RPC channel.

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
