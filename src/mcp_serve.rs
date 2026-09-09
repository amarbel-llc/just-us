use super::*;

use {
  crate::recipe_model::ModelRecipe,
  std::{io::BufRead, io::Read, time::Duration},
};

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

/// `list_recipes`'s default (non-`verbose`) shape: just what's needed to
/// browse and pick a recipe, not the full FDR 0003 model. At a few
/// hundred recipes (a real large multi-justfile repo) the full model is
/// large enough to blow past the MCP inline-result limit and spill to a
/// file the caller then has to `jq`/`grep` — defeating "list recipes so
/// an agent can just read them".
fn compact_recipe(recipe: &ModelRecipe) -> serde_json::Value {
  serde_json::json!({
    "namepath": recipe.namepath,
    "doc": recipe.doc,
    "parameters": recipe.parameters,
    "groups": recipe.groups,
  })
}

/// Directories `find_child_justfiles` never descends into, matching the
/// retired `just-us-agents` moxin's own `list-recipes` script exactly
/// (its `find` invocation pruned these three by name).
const CHILD_JUSTFILE_SKIP: &[&str] = &[".git", ".worktrees", ".claude"];
/// Excludes only the root's own justfile from being double-counted as a
/// "child" — structural, not a real tuning knob, unlike max depth.
const CHILD_JUSTFILE_MIN_DEPTH: usize = 2;
/// Default `max_depth` when a tool call omits it — matches the retired
/// moxin's own hardcoded limit. Overridable per call (`list_recipes`/
/// `show_recipe`'s `max_depth` param) for repos nested deeper than the
/// moxin ever handled.
const CHILD_JUSTFILE_DEFAULT_MAX_DEPTH: usize = 3;

/// Every other, separate justfile in the tree under `root` — reproducing
/// the retired moxin's `find . -mindepth 2 -maxdepth <max_depth> -name
/// justfile` (case-sensitive, exact name only; not just's own broader
/// `search::JUSTFILE_NAMES`). `root` itself is depth 0, so a file here is
/// found at depth `dir_depth + 1`; a directory is only worth recursing
/// into while that would still be `< max_depth` (a directory at depth
/// `max_depth` can only contain files one level deeper, already out of
/// range).
fn find_child_justfiles(root: &Path, max_depth: usize) -> Vec<PathBuf> {
  let mut found = Vec::new();
  find_child_justfiles_at(root, 0, max_depth, &mut found);
  found.sort();
  found
}

fn find_child_justfiles_at(
  dir: &Path,
  dir_depth: usize,
  max_depth: usize,
  found: &mut Vec<PathBuf>,
) {
  let Ok(entries) = fs::read_dir(dir) else {
    return;
  };

  let file_depth = dir_depth + 1;

  for entry in entries.flatten() {
    let Ok(file_type) = entry.file_type() else {
      continue;
    };

    let name = entry.file_name();

    if file_type.is_dir() {
      if CHILD_JUSTFILE_SKIP.iter().any(|skip| name == *skip) {
        continue;
      }

      if file_depth < max_depth {
        find_child_justfiles_at(&entry.path(), file_depth, max_depth, found);
      }
    } else if file_type.is_file()
      && (CHILD_JUSTFILE_MIN_DEPTH..=max_depth).contains(&file_depth)
      && name == "justfile"
    {
      found.push(entry.path());
    }
  }
}

/// `public_recipes` for the root justfile, plus every public recipe from
/// every child justfile `find_child_justfiles` turns up under `root`
/// (docs/features/0006 addendum — parity with the retired moxin's own
/// `list-recipes`). Each child recipe's `namepath` is rewritten to
/// `"<relative-dir>/<namepath>"` — `/`, not `::`, since these are wholly
/// separate justfiles, not `mod`-imports of the one the server started
/// with. A child justfile that fails to compile is silently skipped:
/// this is best-effort repo-wide discovery, not something one unrelated
/// broken subdirectory justfile should be able to fail entirely.
///
/// Backs both `list_recipes` and `show_recipe` — a discovered child
/// recipe is resolvable by `show_recipe`'s `dir/recipe` namepath and
/// runnable via `run_recipe`'s positional form, so there is no longer an
/// asymmetry between what's discoverable and what's addressable.
fn all_public_recipes(
  config: &Config,
  root: &Path,
  root_justfile: &Justfile,
  max_depth: usize,
) -> Vec<ModelRecipe> {
  let mut recipes = public_recipes(root_justfile);

  for path in find_child_justfiles(root, max_depth) {
    let loader = Loader::new();

    let Ok(compilation) = Compiler::compile(config, &loader, &path) else {
      continue;
    };

    let relative_dir = path
      .strip_prefix(root)
      .unwrap_or(&path)
      .parent()
      .unwrap_or(Path::new(""));

    let prefix = relative_dir.to_string_lossy();

    recipes.extend(
      public_recipes(&compilation.justfile)
        .into_iter()
        .map(|mut recipe| {
          recipe.namepath = format!("{prefix}/{}", recipe.namepath);
          recipe
        }),
    );
  }

  recipes
}

fn parse_max_depth(arguments: &serde_json::Value) -> usize {
  arguments
    .get("max_depth")
    .and_then(serde_json::Value::as_u64)
    .map_or(CHILD_JUSTFILE_DEFAULT_MAX_DEPTH, |value| value as usize)
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
        "description": "List every public recipe (namepath, doc, parameters, groups) — including other justfiles found elsewhere in the repo tree, namepath-prefixed by their directory. Compact by default; pass verbose for the full model entry per recipe.",
        "inputSchema": {
          "type": "object",
          "properties": {
            "verbose": {
              "type": "boolean",
              "description": "Return the full recipe model (doc_prelude, dependencies, source, line, ...) per recipe instead of the compact {namepath, doc, parameters, groups} shape. Default false.",
            },
            "max_depth": {
              "type": "integer",
              "description": "How many directory levels below the repo root to search for other justfiles. Default 3 (matching the retired just-us-agents moxin); raise for repos nested deeper than that.",
            },
          },
        },
      },
      {
        "name": "show_recipe",
        "description": "Show one public recipe's full model entry by namepath — resolves recipes in other justfiles found elsewhere in the repo tree too (the same dir/recipe namepath list_recipes reports).",
        "inputSchema": {
          "type": "object",
          "properties": {
            "recipe": {
              "type": "string",
              "description": "Recipe namepath, e.g. \"build\", \"module::recipe\", or \"dir/recipe\" for a recipe in another justfile.",
            },
            "max_depth": {
              "type": "integer",
              "description": "How many directory levels below the repo root to search for other justfiles. Default 3; raise if the recipe lives deeper than that.",
            },
          },
          "required": ["recipe"],
        },
      },
      {
        "name": "run_recipe",
        "description": "Run one justfile recipe as a subprocess (wrapped in `nix develop -c` when a flake.nix is present) and return its captured stdout/stderr.",
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
            "impure": {
              "type": "boolean",
              "description": "Pass --impure to `nix develop`. No effect when there is no flake.nix.",
            },
            "timeout": {
              "type": "string",
              "description": "Kill the recipe if it runs longer than this, e.g. \"25m\", \"90s\", \"2h\". No timeout if omitted.",
            },
            "async": {
              "type": "boolean",
              "description": "Return a ringmaster job id immediately instead of blocking until the recipe finishes. The caller observes completion via ringmaster's own job_wait/job_status.",
            },
          },
          "required": ["recipe"],
        },
      },
      {
        "name": "dump_justfile",
        "description": "Dump the full compiled justfile (equivalent to `just --dump --dump-format json`).",
        "inputSchema": { "type": "object", "properties": {} },
      },
      {
        "name": "list_variables",
        "description": "List every public top-level variable with its resolved value (equivalent to `just --evaluate`).",
        "inputSchema": { "type": "object", "properties": {} },
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
    "list_recipes" => {
      let verbose = arguments
        .get("verbose")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

      let recipes = all_public_recipes(
        config,
        &search.working_directory,
        &compilation.justfile,
        parse_max_depth(&arguments),
      );

      let value = if verbose {
        serde_json::to_value(recipes).unwrap_or(serde_json::Value::Null)
      } else {
        serde_json::Value::Array(recipes.iter().map(compact_recipe).collect())
      };

      ok(id, tool_result_json(value))
    }
    "show_recipe" => {
      let Some(recipe) = arguments.get("recipe").and_then(serde_json::Value::as_str) else {
        return error(id, -32602, "missing \"recipe\" argument");
      };

      let recipes = all_public_recipes(
        config,
        &search.working_directory,
        &compilation.justfile,
        parse_max_depth(&arguments),
      );

      match recipes.into_iter().find(|model| model.namepath == recipe) {
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

      let impure = arguments
        .get("impure")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

      let timeout = match arguments.get("timeout").and_then(serde_json::Value::as_str) {
        Some(input) => match parse_duration(input) {
          Ok(duration) => Some(duration),
          Err(message) => return ok(id, tool_error_text(message)),
        },
        None => None,
      };

      let want_async = arguments
        .get("async")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

      let mut invocation = Vec::with_capacity(args.len() + 1);
      invocation.push(recipe.to_owned());
      invocation.extend(args);

      let justfile_dir = search.working_directory.clone();

      ok(
        id,
        if want_async {
          run_recipe_async(justfile_dir, impure, invocation, recipe, timeout)
        } else {
          run_recipe_sync(recipe_command(justfile_dir, impure, &invocation), timeout)
        },
      )
    }
    "dump_justfile" => ok(
      id,
      tool_result_json(
        serde_json::to_value(&compilation.justfile).unwrap_or(serde_json::Value::Null),
      ),
    ),
    "list_variables" => {
      match compilation
        .justfile
        .evaluate_all(config, search, &compilation.overrides)
      {
        Ok(variables) => ok(
          id,
          tool_result_json(serde_json::json!(
            variables
              .into_iter()
              .map(|(name, value)| serde_json::json!({ "name": name, "value": value }))
              .collect::<Vec<_>>()
          )),
        ),
        Err(eval_error) => ok(
          id,
          tool_error_text(eval_error.color_display(Color::never()).to_string()),
        ),
      }
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

/// The `ringmaster` binary to shell out to for `run_recipe`'s async mode.
/// Prefers the build-time pin (`RINGMASTER_BIN`, set by flake.nix's `just`
/// derivation from clown's `ringmaster` package — see build.rs) so a
/// nix-built `just` never depends on `ringmaster` being ambiently on
/// PATH; falls back to a plain PATH lookup so an ad-hoc `cargo build`
/// dev-loop still works without that input.
fn ringmaster_command() -> Command {
  match option_env!("RINGMASTER_BIN") {
    Some(path) => Command::new(path),
    None => Command::resolve("ringmaster"),
  }
}

/// Parse a single-unit duration like `"25m"`, `"90s"`, `"2h"`; a bare
/// integer is seconds. No compound forms (`"1h30m"`) in this slice —
/// tracked as a possible future extension, not needed by the current
/// contract (`docs/plans/...run-recipe...`).
fn parse_duration(input: &str) -> Result<Duration, String> {
  let input = input.trim();

  let (digits, unit) = match input.find(|c: char| !c.is_ascii_digit()) {
    Some(split) => input.split_at(split),
    None => (input, "s"),
  };

  let value: u64 = digits
    .parse()
    .map_err(|_| format!("invalid duration: {input:?}"))?;

  let seconds = match unit {
    "s" => value,
    "m" => value * 60,
    "h" => value * 60 * 60,
    _ => {
      return Err(format!(
        "invalid duration unit in {input:?} (expected s, m, or h)"
      ));
    }
  };

  Ok(Duration::from_secs(seconds))
}

/// Build the `Command` that actually runs a recipe: wrapped in
/// `nix develop [--impure] -c just <invocation>` when `justfile_dir` has
/// its own `flake.nix` (restores devshell tools for recipes that need
/// them, matching the retired `just-us-agents` moxin's own behavior),
/// otherwise a direct re-invocation of this same `just` binary
/// (`env::current_exe`, falling back to a PATH-resolved `just` if that
/// fails) so there is no ambiguity about which `just` runs a plain
/// recipe.
fn recipe_command(justfile_dir: PathBuf, impure: bool, invocation: &[String]) -> Command {
  if justfile_dir.join("flake.nix").is_file() {
    let mut command = Command::resolve("nix");
    command.arg("develop");

    if impure {
      command.arg("--impure");
    }

    command.arg("-c").arg("just").args(invocation);
    command.current_dir(justfile_dir);
    command
  } else {
    let program = env::current_exe().unwrap_or_else(|_| PathBuf::from("just"));
    let mut command = Command::new(program);
    command.args(invocation);
    command.current_dir(justfile_dir);
    command
  }
}

/// Block on `child`, killing it if `timeout` elapses first. `Ok(None)`
/// means it was killed for timing out; the caller distinguishes that
/// from a normal exit status.
fn wait_with_timeout(
  child: &mut process::Child,
  timeout: Duration,
) -> io::Result<Option<ExitStatus>> {
  let start = Instant::now();

  loop {
    if let Some(status) = child.try_wait()? {
      return Ok(Some(status));
    }

    if start.elapsed() >= timeout {
      let _ = child.kill();
      let _ = child.wait();
      return Ok(None);
    }

    thread::sleep(Duration::from_millis(50));
  }
}

/// Run `command` to completion, capturing stdout/stderr the plain way
/// (no `--events-fd`/`EventSink` involved — `run_recipe` now always
/// executes as a real subprocess, so plain pipe capture is sufficient
/// and there's no in-process stdout to protect).
fn run_recipe_sync(mut command: Command, timeout: Option<Duration>) -> serde_json::Value {
  command.stdout(Stdio::piped());
  command.stderr(Stdio::piped());

  let mut child = match command.spawn() {
    Ok(child) => child,
    Err(io_error) => return tool_error_text(format!("failed to start recipe: {io_error}")),
  };

  let mut stdout_pipe = child.stdout.take();
  let mut stderr_pipe = child.stderr.take();

  let stdout_thread = thread::spawn(move || {
    let mut buffer = Vec::new();
    if let Some(pipe) = &mut stdout_pipe {
      let _ = pipe.read_to_end(&mut buffer);
    }
    buffer
  });

  let stderr_thread = thread::spawn(move || {
    let mut buffer = Vec::new();
    if let Some(pipe) = &mut stderr_pipe {
      let _ = pipe.read_to_end(&mut buffer);
    }
    buffer
  });

  let status = match timeout {
    Some(timeout) => wait_with_timeout(&mut child, timeout),
    None => child.wait().map(Some),
  };

  let stdout = stdout_thread.join().unwrap_or_default();
  let stderr = stderr_thread.join().unwrap_or_default();

  let mut content = Vec::new();

  if !stdout.is_empty() {
    content.push(serde_json::json!({
      "type": "text",
      "text": format!("stdout:\n{}", String::from_utf8_lossy(&stdout)),
    }));
  }

  if !stderr.is_empty() {
    content.push(serde_json::json!({
      "type": "text",
      "text": format!("stderr:\n{}", String::from_utf8_lossy(&stderr)),
    }));
  }

  match status {
    Ok(Some(status)) if status.success() => serde_json::json!({ "content": content }),
    Ok(Some(status)) => {
      content.push(serde_json::json!({ "type": "text", "text": format!("error: recipe exited with {status}") }));
      serde_json::json!({ "content": content, "isError": true })
    }
    Ok(None) => {
      content.push(serde_json::json!({ "type": "text", "text": "error: recipe timed out" }));
      serde_json::json!({ "content": content, "isError": true })
    }
    Err(io_error) => {
      content.push(serde_json::json!({ "type": "text", "text": format!("error: {io_error}") }));
      serde_json::json!({ "content": content, "isError": true })
    }
  }
}

/// Run `invocation` in the background as a real ringmaster job producer
/// (RFC-0009/0010/0011): `ringmaster start` allocates the job and prints
/// its id, the recipe's own stdout/stderr are redirected straight to the
/// job's output spool (`ringmaster spool-path`) so `ringmaster tail -f`
/// and moxy's own `async-result` see live output exactly the way they
/// already do for any other clown job, and `ringmaster done` on
/// completion sends the wake. Session targeting needs no explicit
/// parameter: `ringmaster start` resolves the session to wake from
/// `CLOWN_SESSION_ID`, which this process inherits from the clown
/// stdio-bridge that spawned it.
fn run_recipe_async(
  justfile_dir: PathBuf,
  impure: bool,
  invocation: Vec<String>,
  recipe: &str,
  timeout: Option<Duration>,
) -> serde_json::Value {
  let start = ringmaster_command()
    .args(["start", "--source", "just-us", "--label", recipe])
    .output();

  let job_id = match start {
    Ok(output) if output.status.success() => {
      String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }
    Ok(output) => {
      return tool_error_text(format!(
        "ringmaster start failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
      ));
    }
    Err(io_error) => {
      return tool_error_text(format!("ringmaster is not available: {io_error}"));
    }
  };

  if job_id.is_empty() {
    return tool_error_text("ringmaster start produced no job id".to_owned());
  }

  let spool_path = ringmaster_command()
    .args(["spool-path", &job_id])
    .output()
    .ok()
    .filter(|output| output.status.success())
    .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    .filter(|path| !path.is_empty())
    .map(PathBuf::from);

  let done_job_id = job_id.clone();

  thread::spawn(move || {
    let mut command = recipe_command(justfile_dir, impure, &invocation);

    let spool_file = spool_path.as_ref().and_then(|path| File::create(path).ok());

    match spool_file.as_ref().and_then(|file| file.try_clone().ok()) {
      Some(stdout_target) => command.stdout(stdout_target),
      None => command.stdout(Stdio::null()),
    };

    match spool_file {
      Some(stderr_target) => command.stderr(stderr_target),
      None => command.stderr(Stdio::null()),
    };

    let (state, message) = match command.spawn() {
      Ok(mut child) => {
        let status = match timeout {
          Some(timeout) => wait_with_timeout(&mut child, timeout),
          None => child.wait().map(Some),
        };

        match status {
          Ok(Some(status)) if status.success() => ("succeeded", "recipe completed".to_owned()),
          Ok(Some(status)) => ("failed", format!("recipe exited with {status}")),
          Ok(None) => ("failed", "recipe timed out".to_owned()),
          Err(io_error) => ("failed", format!("wait failed: {io_error}")),
        }
      }
      Err(io_error) => ("failed", format!("failed to start recipe: {io_error}")),
    };

    let _ = ringmaster_command()
      .args([
        "done",
        &done_job_id,
        "--state",
        state,
        "--message",
        &message,
      ])
      .status();
  });

  tool_result_json(serde_json::json!({ "job_id": job_id }))
}
