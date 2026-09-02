use super::*;

use std::io::BufRead;

/// The clown plugin protocol's fixed prompt name for dynamic
/// system-prompt contribution (RFC 0002 §5, docs/features/0005). MUST
/// match spinclass's own `sysprompt.PromptName` — a wire constant shared
/// across implementations, not free to rename independently.
const SYSTEM_PROMPT_NAME: &str = "system-prompt-append";

/// Serve recipe metadata on stdio as a minimal, hand-rolled MCP surface:
/// newline-delimited JSON-RPC, one request per line. Deliberately
/// stateless — clown's stdio bridge may issue `prompts/get` as the very
/// first message, with no preceding `initialize` (docs/features/0005) —
/// so every request is answered independently of what came before it.
/// JSON-RPC notifications (no `id`) are read and silently dropped: this
/// server has no method that produces a side effect worth acting on
/// without a reply.
pub(crate) fn run(justfile: &Justfile) -> RunResult<'static> {
  let roster = roster(justfile);

  let stdin = io::stdin();
  let mut stdout = io::stdout();

  for line in stdin.lock().lines() {
    let line = line.map_err(|io_error| Error::McpIo { io_error })?;
    let line = line.trim();

    if line.is_empty() {
      continue;
    }

    let Ok(request) = serde_json::from_str::<serde_json::Value>(line) else {
      continue;
    };

    let Some(id) = request.get("id").cloned() else {
      continue;
    };

    let method = request.get("method").and_then(serde_json::Value::as_str);

    let response = match method {
      Some("initialize") => ok(id, initialize_result()),
      Some("prompts/list") => ok(id, prompts_list_result()),
      Some("prompts/get") => prompts_get(id, &request, &roster),
      _ => error(id, -32601, "method not found"),
    };

    writeln!(stdout, "{response}").map_err(|io_error| Error::McpIo { io_error })?;
    stdout
      .flush()
      .map_err(|io_error| Error::McpIo { io_error })?;
  }

  Ok(())
}

/// Every public recipe (`ModelRecipe::private == false`), across the root
/// justfile and all modules, as `"<namepath>  <doc>"` lines. No further
/// filtering: no group exclusion, no truncation — the same "name + doc"
/// shape `--list` shows a human, lifted verbatim (docs/features/0005).
fn roster(justfile: &Justfile) -> String {
  RecipeModel::new(justfile)
    .recipes
    .into_iter()
    .filter(|recipe| !recipe.private)
    .map(|recipe| format!("{}  {}", recipe.namepath, recipe.doc.unwrap_or_default()))
    .collect::<Vec<_>>()
    .join("\n")
}

fn ok(id: serde_json::Value, result: serde_json::Value) -> serde_json::Value {
  serde_json::json!({
    "jsonrpc": "2.0",
    "id": id,
    "result": result,
  })
}

fn error(id: serde_json::Value, code: i32, message: &str) -> serde_json::Value {
  serde_json::json!({
    "jsonrpc": "2.0",
    "id": id,
    "error": { "code": code, "message": message },
  })
}

fn initialize_result() -> serde_json::Value {
  serde_json::json!({
    "protocolVersion": "2024-11-05",
    "capabilities": { "prompts": {} },
    "serverInfo": { "name": "just-us", "version": env!("CARGO_PKG_VERSION") },
  })
}

fn prompts_list_result() -> serde_json::Value {
  serde_json::json!({
    "prompts": [{
      "name": SYSTEM_PROMPT_NAME,
      "description": "Public recipe roster (name + doc line) for this justfile.",
    }],
  })
}

fn prompts_get(
  id: serde_json::Value,
  request: &serde_json::Value,
  roster: &str,
) -> serde_json::Value {
  let name = request
    .get("params")
    .and_then(|params| params.get("name"))
    .and_then(serde_json::Value::as_str);

  if name != Some(SYSTEM_PROMPT_NAME) {
    return error(id, -32602, "unknown prompt name");
  }

  ok(
    id,
    serde_json::json!({
      "description": "Public recipe roster (name + doc line) for this justfile.",
      "messages": [{
        "role": "user",
        "content": { "type": "text", "text": roster },
      }],
    }),
  )
}
