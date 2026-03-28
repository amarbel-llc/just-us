#! /usr/bin/env bats

setup() {
  load "$(dirname "$BATS_TEST_FILE")/mcp_common.bash"

  TEST_DIR="$BATS_TEST_TMPDIR/project"
  export CACHE_DIR="$BATS_TEST_TMPDIR/cache"
  mkdir -p "$TEST_DIR"

  cat > "$TEST_DIR/justfile" <<'JUSTFILE'
[agents("always-allowed")]
short-output:
  echo "line 1"
  echo "line 2"
  echo "line 3"

[agents("always-allowed")]
long-output:
  @seq 1 100
JUSTFILE
}

tool_args() {
  jq -cn \
    --arg wd "$TEST_DIR" \
    --arg jf "$TEST_DIR/justfile" \
    --arg recipe "$1" \
    '{"recipe":$recipe,"working_directory":$wd,"justfile":$jf}'
}

function short_output_returns_inline { # @test
  local response
  response=$(mcp_tool_call "run-recipe" "$(tool_args short-output)")

  local is_error
  is_error=$(echo "$response" | jq '.result.isError // false')
  assert_equal "$is_error" "false"

  # Should have exactly one content item (text, no resource)
  local content_count
  content_count=$(echo "$response" | jq '.result.content | length')
  assert_equal "$content_count" "1"

  local content_type
  content_type=$(echo "$response" | jq -r '.result.content[0].type')
  assert_equal "$content_type" "text"
}

function long_output_returns_summary_and_resource { # @test
  local response
  response=$(mcp_tool_call "run-recipe" "$(tool_args long-output)")

  # Should have one text content item with summary + resource URI
  local content_count
  content_count=$(echo "$response" | jq '.result.content | length')
  assert_equal "$content_count" "1"

  local content_type
  content_type=$(echo "$response" | jq -r '.result.content[0].type')
  assert_equal "$content_type" "text"

  local summary
  summary=$(echo "$response" | jq -r '.result.content[0].text')
  echo "$summary" | grep -q "long-output"
  echo "$summary" | grep -q "lines"
  echo "$summary" | grep -q "just-us://results/"
}

function resource_read_returns_full_output { # @test
  # Run tool call to populate cache and get URI from summary text
  local tool_response
  tool_response=$(mcp_tool_call "run-recipe" "$(tool_args long-output)")

  local summary
  summary=$(echo "$tool_response" | jq -r '.result.content[0].text')

  local uri
  uri=$(echo "$summary" | grep -o 'just-us://results/[^ ]*')
  [[ -n "$uri" ]] || {
    echo "no resource URI in summary: $summary" >&2
    return 1
  }

  # Run a new session with tool call + resource read
  local responses
  responses=$(mcp_tool_then_resource "run-recipe" "$(tool_args long-output)" "$uri")

  local resource_response
  resource_response=$(echo "$responses" | jq -c 'select(.id == 3)')

  local text
  text=$(echo "$resource_response" | jq -r '.result.contents[0].text')
  # Full output should contain the seq numbers
  echo "$text" | grep -qw "1"
  echo "$text" | grep -qw "50"
  echo "$text" | grep -qw "100"
}

function cache_files_exist_during_session { # @test
  # Use a FIFO to keep the server alive while we inspect the filesystem.
  local fifo="$BATS_TEST_TMPDIR/mcp_input"
  mkfifo "$fifo"

  local bin="${JUST_US_AGENTS_BIN:-just-us-agents}"

  JUST_US_AGENTS_CACHE_DIR="$CACHE_DIR" "$bin" mcp < "$fifo" > /dev/null 2>&1 &
  local server_pid=$!

  # Open the FIFO for writing
  exec 7>"$fifo"

  echo "$(_mcp_init_request)" >&7
  echo "$(_mcp_init_notify)" >&7
  sleep 0.3
  echo "$(_mcp_call_tool 2 "run-recipe" "$(tool_args long-output)")" >&7
  sleep 1

  # Cache dir should exist with files while server is still running
  [[ -d "$CACHE_DIR" ]]
  local file_count
  file_count=$(find "$CACHE_DIR" -name "*.just-us-agents-command-result" | wc -l)
  (( file_count >= 1 ))

  # Close the FIFO to trigger server shutdown
  exec 7>&-
  wait "$server_pid" 2>/dev/null || true
  rm -f "$fifo"
}

function cache_cleaned_up_on_exit { # @test
  # Use a FIFO so we can verify cache exists, then close to trigger cleanup.
  local fifo="$BATS_TEST_TMPDIR/mcp_input"
  mkfifo "$fifo"

  local bin="${JUST_US_AGENTS_BIN:-just-us-agents}"

  JUST_US_AGENTS_CACHE_DIR="$CACHE_DIR" "$bin" mcp < "$fifo" > /dev/null 2>&1 &
  local server_pid=$!

  exec 7>"$fifo"
  echo "$(_mcp_init_request)" >&7
  echo "$(_mcp_init_notify)" >&7
  sleep 0.3
  echo "$(_mcp_call_tool 2 "run-recipe" "$(tool_args long-output)")" >&7
  sleep 1

  # Verify cache exists before shutdown
  [[ -d "$CACHE_DIR" ]]

  # Close stdin to trigger cleanup
  exec 7>&-
  wait "$server_pid" 2>/dev/null || true
  rm -f "$fifo"

  # Cache dir should be gone after server exits
  [[ ! -d "$CACHE_DIR" ]]
}
