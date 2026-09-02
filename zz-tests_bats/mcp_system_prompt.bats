# bats file_tags=mcp
#
# `just --mcp`'s dynamic system-prompt roster (docs/features/0005): a
# minimal, hand-rolled MCP surface on stdio that answers `prompts/get
# system-prompt-append` with every PUBLIC recipe's namepath + doc line,
# no further filtering (debug/explore groups included).

setup() {
  load "$(dirname "$BATS_TEST_FILE")/common.bash"
  setup_test_home
}

@test "--mcp: prompts/get system-prompt-append lists public recipes, excludes private ones" {
  cat > justfile <<'EOF'
# builds the thing
build:
    @echo build

_hidden:
    @echo hidden

[private]
also-hidden:
    @echo also-hidden

# a debug-group recipe, still surfaced (no filtering)
[group("debug")]
debug-thing:
    @echo debug
EOF

  requests=$(printf '%s\n%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' \
    '{"jsonrpc":"2.0","id":2,"method":"prompts/get","params":{"name":"system-prompt-append"}}')

  run timeout --preserve-status 5s bash -c '"$0" --mcp <<<"$1"' "${JUST_BIN:-just}" "$requests"
  assert_success

  first_reply=$(echo "$output" | sed -n '1p')
  second_reply=$(echo "$output" | sed -n '2p')

  [[ $first_reply == *'"id":1'* ]] || fail "first reply missing id 1: $first_reply"
  [[ $first_reply == *'"prompts"'* ]] || fail "initialize reply missing prompts capability: $first_reply"

  [[ $second_reply == *'"id":2'* ]] || fail "second reply missing id 2: $second_reply"
  [[ $second_reply == *'build'* ]] || fail "roster missing public recipe 'build': $second_reply"
  [[ $second_reply == *'debug-thing'* ]] || \
    fail "roster missing debug-group recipe (no filtering expected): $second_reply"
  [[ $second_reply != *'_hidden'* ]] || fail "roster leaked underscore-private recipe: $second_reply"
  [[ $second_reply != *'also-hidden'* ]] || fail "roster leaked [private]-attributed recipe: $second_reply"
}

@test "--mcp: prompts/get for an unknown name returns a JSON-RPC error, not a crash" {
  cat > justfile <<'EOF'
build:
    @echo build
EOF

  run timeout --preserve-status 5s bash -c '"$0" --mcp <<<"$1"' "${JUST_BIN:-just}" \
    '{"jsonrpc":"2.0","id":1,"method":"prompts/get","params":{"name":"nope"}}'
  assert_success
  [[ $output == *'"error"'* ]] || fail "expected a JSON-RPC error for an unknown prompt name: $output"
}
