bats_load_library bats-support
bats_load_library bats-assert
bats_load_library bats-emo

require_bin JUST_US_AGENTS_BIN just-us-agents

# Send a sequence of JSON-RPC messages to the MCP server and capture all responses.
# Filters out notification responses (id=null). Returns one JSON response per line.
mcp_session() {
  local bin="${JUST_US_AGENTS_BIN:-just-us-agents}"
  local input=""
  for msg in "$@"; do
    input+="$msg"$'\n'
  done
  echo "$input" |
    JUST_US_AGENTS_CACHE_DIR="$CACHE_DIR" "$bin" mcp 2>/dev/null |
    jq -c 'select(.id != null)'
}

_mcp_init_request() {
  jq -cn '{
    "jsonrpc":"2.0",
    "id":1,
    "method":"initialize",
    "params":{
      "protocolVersion":"2024-11-05",
      "capabilities":{},
      "clientInfo":{"name":"bats-test","version":"0.1.0"}
    }
  }'
}

_mcp_init_notify() {
  jq -cn '{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}'
}

_mcp_call_tool() {
  local id="$1"
  local name="$2"
  local args="${3:-\{\}}"
  jq -cn \
    --argjson id "$id" \
    --arg name "$name" \
    --argjson args "$args" \
    '{"jsonrpc":"2.0","id":$id,"method":"tools/call","params":{"name":$name,"arguments":$args}}'
}

_mcp_read_resource() {
  local id="$1"
  local uri="$2"
  jq -cn \
    --argjson id "$id" \
    --arg uri "$uri" \
    '{"jsonrpc":"2.0","id":$id,"method":"resources/read","params":{"uri":$uri}}'
}

# Run an MCP session with initialize + a single tool call.
# Returns just the tool call response (id=2).
mcp_tool_call() {
  local name="$1"
  local args="${2:-\{\}}"
  mcp_session \
    "$(_mcp_init_request)" \
    "$(_mcp_init_notify)" \
    "$(_mcp_call_tool 2 "$name" "$args")" |
    jq -c 'select(.id == 2)'
}

# Run an MCP session with initialize + tool call + resource read.
# Outputs two lines: tool response (id=2), resource response (id=3).
mcp_tool_then_resource() {
  local name="$1"
  local args="${2:-\{\}}"
  local uri="$3"
  mcp_session \
    "$(_mcp_init_request)" \
    "$(_mcp_init_notify)" \
    "$(_mcp_call_tool 2 "$name" "$args")" \
    "$(_mcp_read_resource 3 "$uri")" |
    jq -c 'select(.id == 2 or .id == 3)'
}
