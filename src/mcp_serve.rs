use super::*;

use {crate::recipe_model::ModelRecipe, std::io::BufRead};

/// The clown plugin protocol's fixed prompt name for dynamic
/// system-prompt contribution (RFC 0002 §5, docs/features/0005). MUST
/// match spinclass's own `sysprompt.PromptName` — a wire constant shared
/// across implementations, not free to rename independently.
const SYSTEM_PROMPT_NAME: &str = "system-prompt-append";

/// Serve recipe metadata and execution on stdio as a minimal, hand-rolled
/// MCP surface: newline-delimited JSON-RPC, one request per line.
/// Deliberately stateless — clown's stdio bridge may issue `prompts/get`
/// as the very first message, with no preceding `initialize`
/// (docs/features/0005) — so every request is answered independently of
/// what came before it. JSON-RPC notifications (no `id`) are read and
/// silently dropped: this server has no method that produces a side
/// effect worth acting on without a reply.
pub(crate) fn run(
  config: &Config,
  search: &Search,
  compilation: Compilation,
) -> RunResult<'static> {
  let roster = roster(&compilation.justfile);

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
      Some("tools/list") => ok(id, tools_list_result()),
      Some("tools/call") => tools_call(id, &request, config, search, &compilation),
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
/// justfile and all modules. No further filtering: no group exclusion, no
/// truncation. Shared source of truth for the system-prompt roster and
/// the `list_recipes`/`show_recipe` tools — all three expose the same
/// "what's reachable through discovery" contract.
fn public_recipes(justfile: &Justfile) -> Vec<ModelRecipe> {
  RecipeModel::new(justfile)
    .recipes
    .into_iter()
    .filter(|recipe| !recipe.private)
    .collect()
}

/// `"<namepath>  <doc>"` lines, the same shape `--list` shows a human,
/// lifted verbatim (docs/features/0005).
fn roster(justfile: &Justfile) -> String {
  public_recipes(justfile)
    .into_iter()
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
    "capabilities": { "prompts": {}, "tools": {} },
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

fn tools_list_result() -> serde_json::Value {
  serde_json::json!({
    "tools": [
      {
        "name": "list_recipes",
        "description": "List every public recipe with its namepath, doc, group, and parameters.",
        "inputSchema": { "type": "object", "properties": {} },
      },
      {
        "name": "show_recipe",
        "description": "Show one public recipe's full model entry by namepath.",
        "inputSchema": {
          "type": "object",
          "properties": {
            "recipe": {
              "type": "string",
              "description": "Recipe namepath, e.g. \"build\" or \"module::recipe\".",
            },
          },
          "required": ["recipe"],
        },
      },
      {
        "name": "run_recipe",
        "description": "Run one justfile recipe and return its captured stdout/stderr.",
        "inputSchema": {
          "type": "object",
          "properties": {
            "recipe": {
              "type": "string",
              "description": "Recipe namepath to run.",
            },
            "args": {
              "type": "array",
              "items": { "type": "string" },
              "description": "Positional arguments, in declared-parameter order (just has no named-argument CLI syntax).",
            },
          },
          "required": ["recipe"],
        },
      },
    ],
  })
}

fn tools_call(
  id: serde_json::Value,
  request: &serde_json::Value,
  config: &Config,
  search: &Search,
  compilation: &Compilation,
) -> serde_json::Value {
  let Some(params) = request.get("params") else {
    return error(id, -32602, "missing params");
  };

  let Some(name) = params.get("name").and_then(serde_json::Value::as_str) else {
    return error(id, -32602, "missing tool name");
  };

  let arguments = params.get("arguments").cloned().unwrap_or_default();

  match name {
    "list_recipes" => ok(
      id,
      tool_result_json(
        serde_json::to_value(public_recipes(&compilation.justfile))
          .unwrap_or(serde_json::Value::Null),
      ),
    ),
    "show_recipe" => {
      let Some(recipe) = arguments.get("recipe").and_then(serde_json::Value::as_str) else {
        return error(id, -32602, "missing \"recipe\" argument");
      };

      match public_recipes(&compilation.justfile)
        .into_iter()
        .find(|model| model.namepath == recipe)
      {
        Some(model) => ok(
          id,
          tool_result_json(serde_json::to_value(model).unwrap_or(serde_json::Value::Null)),
        ),
        None => ok(id, tool_error_text(format!("unknown recipe: {recipe}"))),
      }
    }
    "run_recipe" => {
      let Some(recipe) = arguments.get("recipe").and_then(serde_json::Value::as_str) else {
        return error(id, -32602, "missing \"recipe\" argument");
      };

      let args = arguments
        .get("args")
        .and_then(serde_json::Value::as_array)
        .map(|values| {
          values
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>()
        })
        .unwrap_or_default();

      ok(id, run_recipe(config, search, compilation, recipe, &args))
    }
    _ => error(id, -32601, "unknown tool"),
  }
}

fn tool_result_json(value: serde_json::Value) -> serde_json::Value {
  serde_json::json!({
    "content": [{ "type": "text", "text": value.to_string() }],
  })
}

fn tool_error_text(message: String) -> serde_json::Value {
  serde_json::json!({
    "content": [{ "type": "text", "text": message }],
    "isError": true,
  })
}

/// A `Write` sink over a shared buffer, so `run_recipe` can hand
/// `EventSink::from_writer` something it owns while keeping a handle to
/// read the captured bytes back afterward.
#[derive(Clone, Default)]
struct CaptureBuffer(Arc<Mutex<Vec<u8>>>);

impl io::Write for CaptureBuffer {
  fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
    self.0.lock().unwrap().extend_from_slice(buf);
    Ok(buf.len())
  }

  fn flush(&mut self) -> io::Result<()> {
    Ok(())
  }
}

/// Run one recipe via the exact same `Justfile::run` path
/// `Subcommand::Run` uses, with an `EventSink` backed by an in-memory
/// buffer instead of a real fd. `EventSink::is_active()` — not
/// `config.events_fd` — is what gates the RFC 0002 §Suppressing
/// Inherited stdout/stderr capture path (`src/recipe.rs`), so an active
/// writer-backed sink is enough to keep the recipe's child stdout/stderr
/// out of this server's own stdout, which is the MCP JSON-RPC channel.
fn run_recipe(
  config: &Config,
  search: &Search,
  compilation: &Compilation,
  recipe: &str,
  args: &[String],
) -> serde_json::Value {
  let mut arguments = Vec::with_capacity(args.len() + 1);
  arguments.push(recipe.to_owned());
  arguments.extend(args.iter().cloned());

  let mut run_config = config.clone();
  run_config.subcommand = Subcommand::Run {
    arguments: arguments.clone(),
  };

  let buffer = CaptureBuffer::default();
  let events = EventSink::from_writer(buffer.clone());

  let result = compilation.justfile.run(
    &run_config,
    &events,
    search,
    &arguments,
    &compilation.overrides,
  );

  let captured = buffer.0.lock().unwrap().clone();
  let mut content = decode_captured_output(&captured);

  if let Err(run_error) = result {
    let message = run_error.color_display(Color::never()).to_string();
    content.push(serde_json::json!({ "type": "text", "text": message }));
    serde_json::json!({ "content": content, "isError": true })
  } else {
    serde_json::json!({ "content": content })
  }
}

/// Pull `output` events back out of the captured NDJSON stream, grouped
/// by stream. Current recipe execution always emits `OutputDataFormat::
/// Utf8` (`src/recipe.rs`'s `capture_with_events` lossy-converts before
/// emitting) — `Base64` is a wire-format provision with no producer yet,
/// so it is read the same way rather than decoded.
fn decode_captured_output(captured: &[u8]) -> Vec<serde_json::Value> {
  let mut stdout = String::new();
  let mut stderr = String::new();

  for line in captured.split(|&byte| byte == b'\n') {
    if line.is_empty() {
      continue;
    }

    let Ok(event) = serde_json::from_slice::<serde_json::Value>(line) else {
      continue;
    };

    if event.get("type").and_then(serde_json::Value::as_str) != Some("output") {
      continue;
    }

    let Some(data) = event.get("data").and_then(serde_json::Value::as_str) else {
      continue;
    };

    match event.get("stream").and_then(serde_json::Value::as_str) {
      Some("stderr") => stderr.push_str(data),
      _ => stdout.push_str(data),
    }
  }

  let mut content = Vec::new();

  if !stdout.is_empty() {
    content.push(serde_json::json!({ "type": "text", "text": format!("stdout:\n{stdout}") }));
  }

  if !stderr.is_empty() {
    content.push(serde_json::json!({ "type": "text", "text": format!("stderr:\n{stderr}") }));
  }

  content
}
