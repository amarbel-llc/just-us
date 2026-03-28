#! /usr/bin/env bats

setup() {
  load "$(dirname "$BATS_TEST_FILE")/mcp_common.bash"

  TEST_DIR="$BATS_TEST_TMPDIR/project"
  export CACHE_DIR="$BATS_TEST_TMPDIR/cache"
  mkdir -p "$TEST_DIR"

  cat > "$TEST_DIR/justfile" <<'JUSTFILE'
[agents("always-allowed")]
hello:
  echo "hello world"
JUSTFILE
}

tool_args() {
  jq -cn \
    --arg wd "$TEST_DIR" \
    --arg jf "$TEST_DIR/justfile" \
    --arg recipe "$1" \
    '{"recipe":$recipe,"working_directory":$wd,"justfile":$jf}'
}

function progress_notifications_sent_during_recipe { # @test
  # Use FIFO to capture all output lines including progress notifications
  local fifo="$BATS_TEST_TMPDIR/mcp_input"
  mkfifo "$fifo"

  local bin="${JUST_US_AGENTS_BIN:-just-us-agents}"
  local output_file="$BATS_TEST_TMPDIR/mcp_output"

  JUST_US_AGENTS_CACHE_DIR="$CACHE_DIR" "$bin" mcp < "$fifo" > "$output_file" 2>/dev/null &
  local server_pid=$!

  exec 7>"$fifo"
  echo "$(_mcp_init_request)" >&7
  echo "$(_mcp_init_notify)" >&7
  sleep 0.3
  echo "$(_mcp_call_tool 2 "run-recipe" "$(tool_args hello)")" >&7
  sleep 1

  exec 7>&-
  wait "$server_pid" 2>/dev/null || true
  rm -f "$fifo"

  # Should contain at least one progress notification
  local progress_count
  progress_count=$(grep -c 'notifications/progress' "$output_file")
  (( progress_count >= 2 ))

  # Progress should appear before the tool result (id=2)
  local first_progress_line
  first_progress_line=$(grep -n 'notifications/progress' "$output_file" | head -1 | cut -d: -f1)

  local result_line
  result_line=$(grep -n '"id":2' "$output_file" | tail -1 | cut -d: -f1)

  (( first_progress_line < result_line ))
}

function progress_includes_recipe_name { # @test
  local fifo="$BATS_TEST_TMPDIR/mcp_input"
  mkfifo "$fifo"

  local bin="${JUST_US_AGENTS_BIN:-just-us-agents}"
  local output_file="$BATS_TEST_TMPDIR/mcp_output"

  JUST_US_AGENTS_CACHE_DIR="$CACHE_DIR" "$bin" mcp < "$fifo" > "$output_file" 2>/dev/null &
  local server_pid=$!

  exec 7>"$fifo"
  echo "$(_mcp_init_request)" >&7
  echo "$(_mcp_init_notify)" >&7
  sleep 0.3
  echo "$(_mcp_call_tool 2 "run-recipe" "$(tool_args hello)")" >&7
  sleep 1

  exec 7>&-
  wait "$server_pid" 2>/dev/null || true
  rm -f "$fifo"

  # Progress token should contain the recipe name
  grep 'notifications/progress' "$output_file" | head -1 | jq -e '.params.progressToken == "just-hello"'

  # Progress message should mention the recipe
  grep 'notifications/progress' "$output_file" | head -1 | jq -e '.params.message | test("hello")'
}
