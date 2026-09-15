use super::*;

use {
  crate::recipe_model::ModelRecipe,
  std::{
    io::BufRead,
    io::Read,
    sync::{
      atomic::{AtomicBool, Ordering},
      mpsc,
    },
    time::Duration,
  },
};

// `nix::sys::signal::Signal` is deliberately not imported under its bare
// name: `use super::*` already brings `crate::signal::Signal` (the
// `--events-fd`/interrupted-error enum, which has no SIGKILL variant) into
// scope, so every reference below is fully qualified instead of aliased,
// matching `src/signals.rs`'s own style. `std::os::unix::process::CommandExt`
// is imported as `_`, not by name, for the same reason: `use super::*`
// already brings the crate's OWN `command_ext::CommandExt` (which provides
// `Command::resolve`) into scope, and a second `use` of a same-named item
// would shadow it outright rather than merely collide; `as _` imports the
// trait's methods (`process_group`) for method-call resolution without
// binding a name at all, so both traits' methods stay reachable.
#[cfg(unix)]
use {nix::unistd::Pid, std::os::unix::process::CommandExt as _};

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
        "description": "Run one justfile recipe as a subprocess, including a recipe in another justfile found elsewhere in the repo tree (dir/recipe) -- just's own native search-directory resolution finds it, no depth limit. Wrapped in `nix develop` when a flake.nix backs it: that directory's own flake.nix if it has one, else the repo root's. Reports which directory the recipe resolves to and which devshell (if any) backed it, and returns its captured stdout/stderr.",
        "inputSchema": {
          "type": "object",
          "properties": {
            "recipe": {
              "type": "string",
              "description": "Recipe namepath to run, e.g. \"build\", \"module::recipe\", or \"dir/recipe\" for a recipe in another justfile.",
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

      let run_dir = search.working_directory.clone();
      let recipe_dir = flake_target_dir(&run_dir, recipe);
      let flake_dir = resolve_flake_dir(&recipe_dir, &run_dir);

      let mut invocation = Vec::with_capacity(args.len() + 1);
      invocation.push(recipe.to_owned());
      invocation.extend(args);

      ok(
        id,
        if want_async {
          run_recipe_async(
            run_dir, recipe_dir, flake_dir, impure, invocation, recipe, timeout,
          )
        } else {
          let command = recipe_command(run_dir, flake_dir.clone(), impure, &invocation);
          run_recipe_sync(command, recipe_dir, flake_dir, timeout)
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
/// A non-empty `JUST_US_RINGMASTER_BIN` in the environment wins outright
/// (a runtime hook so a test can stand in a stub for the real binary —
/// just-us#34's hung-`ringmaster start` regression test needs one that
/// never returns). Otherwise prefers the build-time pin
/// (`RINGMASTER_BIN`, set by flake.nix's `just` derivation from clown's
/// `ringmaster` package — see build.rs) so a nix-built `just` never
/// depends on `ringmaster` being ambiently on PATH; falls back to a plain
/// PATH lookup so an ad-hoc `cargo build` dev-loop still works without
/// that input.
fn ringmaster_command() -> Command {
  if let Some(path) = env::var_os("JUST_US_RINGMASTER_BIN").filter(|path| !path.is_empty()) {
    return Command::new(path);
  }

  match option_env!("RINGMASTER_BIN") {
    Some(path) => Command::new(path),
    None => Command::resolve("ringmaster"),
  }
}

/// Bound on each `ringmaster` CLI call the async dispatch path makes
/// (`start`, `spool-path`, `done`). Every one of them is a local journal
/// write that should take milliseconds; one that hangs (a wedged nudge
/// socket, a stalled filesystem) used to block the tool call — and, this
/// server being serial, every request queued behind it — indefinitely
/// (just-us#34). `async: true` must either create the job or error.
const RINGMASTER_CALL_TIMEOUT: Duration = Duration::from_secs(10);

/// Run `ringmaster <args>` to completion under `RINGMASTER_CALL_TIMEOUT`
/// and return its trimmed stdout. `Err` carries the tool-facing message
/// for every failure mode: binary not found, timed out (and killed), or
/// a non-zero exit (with its stderr).
fn ringmaster_call(args: &[&str]) -> Result<String, String> {
  let subcommand = args.first().copied().unwrap_or_default();
  let mut command = ringmaster_command();
  command.args(args);

  let captured = run_captured(command, Some(RINGMASTER_CALL_TIMEOUT))
    .map_err(|io_error| format!("ringmaster is not available: {io_error}"))?;

  match captured.status {
    Ok(Some(status)) if status.success() => {
      Ok(String::from_utf8_lossy(&captured.stdout).trim().to_owned())
    }
    Ok(Some(_)) => Err(format!(
      "ringmaster {subcommand} failed: {}",
      String::from_utf8_lossy(&captured.stderr).trim()
    )),
    Ok(None) => Err(format!(
      "ringmaster {subcommand} timed out after {}s and was killed",
      RINGMASTER_CALL_TIMEOUT.as_secs()
    )),
    Err(io_error) => Err(format!("ringmaster {subcommand}: wait failed: {io_error}")),
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

/// Where `run_recipe`'s devshell selection should look for a
/// `flake.nix` when `recipe` has a `dir/` prefix. `just` itself already
/// natively resolves a `dir/recipe` argument: a bare recipe argument
/// containing `/` is split at the *last* `/` into a search directory and
/// a recipe name (`Positional::from_values`), and `Search::justfile`
/// then does `just`'s ordinary upward search starting from there,
/// setting the recipe's own working directory accordingly — entirely on
/// its own, with no help needed from this server (verified directly:
/// `just a/b/c/recipe`, invoked from a directory whose only justfile at
/// that path is `a/b/c/justfile`, finds and runs it unaided). The one
/// thing `just`'s own resolution can't do anything about is which
/// devshell backs it: `nix develop` needs an explicit installable
/// *before* `just` even starts, so it has no way to defer to `just`'s
/// internal search. This mirrors that same last-`/` split purely to keep
/// devshell selection pointed at wherever `just` will actually end up
/// running — no filesystem walk, no depth limit, since `just`'s own
/// search has neither.
fn flake_target_dir(root: &Path, recipe: &str) -> PathBuf {
  match recipe.rfind('/') {
    Some(index) => root.join(&recipe[..index]),
    None => root.to_owned(),
  }
}

/// Which directory's `flake.nix` backs `run_recipe`'s devshell for a
/// recipe whose `flake_target_dir` is `recipe_dir`: `recipe_dir`'s own,
/// if it has one, otherwise `root`'s — never an arbitrary intermediate
/// ancestor. A bounded two-candidate fallback (not an unbounded upward
/// walk) covers every directory shape seen across the fleet's
/// justfiles: a child with its own flake.nix, or one with none that's
/// meant to share the root's. An unbounded walk would additionally risk
/// picking up an unrelated intermediate flake.nix that happens to sit
/// between the child and the root but wasn't intended as its devshell.
fn resolve_flake_dir(recipe_dir: &Path, root: &Path) -> Option<PathBuf> {
  if recipe_dir.join("flake.nix").is_file() {
    Some(recipe_dir.to_owned())
  } else if root.join("flake.nix").is_file() {
    Some(root.to_owned())
  } else {
    None
  }
}

/// Build the `Command` that actually runs a recipe. `run_dir` is always
/// the repo root (`search.working_directory`) — never a directory
/// derived from `recipe`'s own `dir/` prefix — because `just`'s own
/// native search-directory resolution needs `invocation_directory` (the
/// new process's real OS-level cwd) to be the root for its relative-path
/// splitting of `recipe` to land in the right place; see
/// `flake_target_dir`. When `flake_dir` is `Some`, wraps in
/// `nix develop [--impure] <flake_dir> -c just <invocation>`, passing
/// `flake_dir` as an explicit path installable rather than relying on
/// `nix develop`'s own cwd-based flake resolution — the two can
/// legitimately differ (a child justfile with no `flake.nix` of its own
/// borrows the root's, per `resolve_flake_dir`) and `nix develop` has no
/// way to pick a different flake than whatever's at its own cwd
/// otherwise. When `flake_dir` is `None`, falls back to a direct
/// re-invocation of this same `just` binary (`env::current_exe`,
/// falling back to a PATH-resolved `just` if that fails) — `just`
/// resolves `recipe` itself either way.
fn recipe_command(
  run_dir: PathBuf,
  flake_dir: Option<PathBuf>,
  impure: bool,
  invocation: &[String],
) -> Command {
  match flake_dir {
    Some(flake_dir) => {
      let mut command = Command::resolve("nix");
      command.arg("develop").arg(flake_dir);

      if impure {
        command.arg("--impure");
      }

      command.arg("-c").arg("just").args(invocation);
      command.current_dir(run_dir);
      command
    }
    None => {
      let program = env::current_exe().unwrap_or_else(|_| PathBuf::from("just"));
      let mut command = Command::new(program);
      command.args(invocation);
      command.current_dir(run_dir);
      command
    }
  }
}

/// The single-line `"resolved: cwd=..., devshell=..."` text block
/// `run_recipe_sync` prepends to its content array — the sync path's own
/// idiom (plain text blocks, not a JSON object) for answering "which
/// devshell ran this recipe" (the ergonomic gap that motivated this
/// resolution logic in the first place). `recipe_dir` (from
/// `flake_target_dir`) reports where `recipe`'s own `dir/` prefix points
/// — the directory `just`'s native search-directory resolution will
/// land in for any recipe actually reachable via `list_recipes`/
/// `show_recipe`'s discovery, though it's a derived hint rather than a
/// literal `Command::current_dir` (that stays the repo root either way;
/// see `recipe_command`).
fn resolved_info_text(recipe_dir: &Path, flake_dir: &Option<PathBuf>) -> String {
  format!(
    "resolved: cwd={}, devshell={}",
    recipe_dir.display(),
    flake_dir.as_ref().map_or_else(
      || "none".to_owned(),
      |dir| dir.join("flake.nix").display().to_string()
    )
  )
}

/// Send `signal` to the process group whose id is `pid` — i.e. to `pid`
/// and every descendant it spawned, given the child was started with
/// `process_group(0)` (which makes its pid its pgid). Best-effort: an
/// already-empty group (ESRCH) or an unrepresentable pid is ignored.
#[cfg(unix)]
fn signal_process_group(pid: u32, signal: nix::sys::signal::Signal) {
  let Ok(pid) = i32::try_from(pid) else {
    return;
  };

  let _ = nix::sys::signal::kill(Pid::from_raw(-pid), signal);
}

/// Tear down `child` and everything it spawned, then reap it. Unix: the
/// whole process group gets SIGTERM, up to `CANCEL_GRACE_PERIOD` to exit
/// on its own, then SIGKILL. Elsewhere only the immediate child can be
/// killed. Killing just the immediate child (`just`, or `nix develop`)
/// was the wedge in just-us#34: its per-recipe-line `sh` children and
/// anything they backgrounded survived, still holding the captured
/// stdout/stderr pipes, so the readers never saw EOF and the tool call
/// never returned.
fn kill_process_tree(child: &mut process::Child) {
  #[cfg(unix)]
  {
    signal_process_group(child.id(), nix::sys::signal::Signal::SIGTERM);

    let deadline = Instant::now() + CANCEL_GRACE_PERIOD;

    while Instant::now() < deadline {
      if matches!(child.try_wait(), Ok(Some(_))) {
        break;
      }

      thread::sleep(Duration::from_millis(50));
    }

    signal_process_group(child.id(), nix::sys::signal::Signal::SIGKILL);
  }

  #[cfg(not(unix))]
  {
    let _ = child.kill();
  }

  let _ = child.wait();
}

/// Block on `child`, killing it (and its process tree — see
/// `kill_process_tree`) if `timeout` elapses first. `Ok(None)` means it
/// was killed for timing out; the caller distinguishes that from a
/// normal exit status.
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
      kill_process_tree(child);
      return Ok(None);
    }

    thread::sleep(Duration::from_millis(50));
  }
}

/// How long `run_captured` keeps reading a finished process's stdout/
/// stderr before returning with whatever has arrived. The process itself
/// has already exited (or been killed) by then, so all that can still be
/// in flight is the pipe buffer's tail — unless a descendant it left
/// running (a backgrounded daemon the recipe deliberately started, say)
/// still holds the write end, in which case EOF never comes and waiting
/// for it wedged the call (just-us#34).
const PIPE_DRAIN_GRACE: Duration = Duration::from_secs(2);

/// What `run_captured` observed: the wait outcome (`Ok(None)` = timed out
/// and killed), everything read from each pipe, and whether both pipes
/// actually reached EOF (`drained: false` means a descendant still held
/// one open when `PIPE_DRAIN_GRACE` ran out and later output was dropped).
struct Captured {
  status: io::Result<Option<ExitStatus>>,
  stdout: Vec<u8>,
  stderr: Vec<u8>,
  drained: bool,
}

/// Read `pipe` to EOF on a background thread, forwarding each chunk as it
/// arrives so the caller can stop collecting at a deadline and still keep
/// everything read up to that point (a single `read_to_end` would hold
/// it all hostage until EOF). The channel closing is the EOF signal.
fn drain_in_background<R: Read + Send + 'static>(pipe: Option<R>) -> mpsc::Receiver<Vec<u8>> {
  let (sender, receiver) = mpsc::channel();

  thread::spawn(move || {
    let Some(mut pipe) = pipe else {
      return;
    };

    let mut chunk = [0u8; 8192];

    loop {
      match pipe.read(&mut chunk) {
        Ok(0) | Err(_) => return,
        Ok(read) => {
          if sender.send(chunk[..read].to_vec()).is_err() {
            return;
          }
        }
      }
    }
  });

  receiver
}

/// Collect chunks from `receiver` until it closes (EOF — `true`) or
/// `deadline` passes (`false`), returning what arrived either way.
fn collect_until(receiver: &mpsc::Receiver<Vec<u8>>, deadline: Instant) -> (Vec<u8>, bool) {
  let mut collected = Vec::new();

  loop {
    let remaining = deadline.saturating_duration_since(Instant::now());

    match receiver.recv_timeout(remaining) {
      Ok(chunk) => collected.extend(chunk),
      Err(mpsc::RecvTimeoutError::Disconnected) => return (collected, true),
      Err(mpsc::RecvTimeoutError::Timeout) => return (collected, false),
    }
  }
}

/// Run `command` to completion in its own process group (unix),
/// capturing stdout/stderr, honoring `timeout` via `wait_with_timeout`,
/// and — crucially — always returning: once the process is gone the
/// pipes are drained for at most `PIPE_DRAIN_GRACE`, never "until EOF".
/// `Err` is a spawn failure. Backs both `run_recipe`'s sync path and the
/// async path's `ringmaster` CLI calls.
fn run_captured(mut command: Command, timeout: Option<Duration>) -> io::Result<Captured> {
  command.stdout(Stdio::piped());
  command.stderr(Stdio::piped());

  #[cfg(unix)]
  command.process_group(0);

  let mut child = command.spawn()?;

  let stdout_receiver = drain_in_background(child.stdout.take());
  let stderr_receiver = drain_in_background(child.stderr.take());

  let status = match timeout {
    Some(timeout) => wait_with_timeout(&mut child, timeout),
    None => child.wait().map(Some),
  };

  let deadline = Instant::now() + PIPE_DRAIN_GRACE;
  let (stdout, stdout_drained) = collect_until(&stdout_receiver, deadline);
  let (stderr, stderr_drained) = collect_until(&stderr_receiver, deadline);

  Ok(Captured {
    status,
    stdout,
    stderr,
    drained: stdout_drained && stderr_drained,
  })
}

/// Run `command` to completion, capturing stdout/stderr the plain way
/// (no `--events-fd`/`EventSink` involved — `run_recipe` now always
/// executes as a real subprocess, so plain pipe capture is sufficient
/// and there's no in-process stdout to protect).
fn run_recipe_sync(
  command: Command,
  recipe_dir: PathBuf,
  flake_dir: Option<PathBuf>,
  timeout: Option<Duration>,
) -> serde_json::Value {
  let Captured {
    status,
    stdout,
    stderr,
    drained,
  } = match run_captured(command, timeout) {
    Ok(captured) => captured,
    Err(io_error) => return tool_error_text(format!("failed to start recipe: {io_error}")),
  };

  let mut content = vec![serde_json::json!({
    "type": "text",
    "text": resolved_info_text(&recipe_dir, &flake_dir),
  })];

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

  if !drained {
    content.push(serde_json::json!({
      "type": "text",
      "text": format!(
        "note: the recipe's stdout/stderr pipes are still held open by a process it left running (a backgrounded daemon?); output is shown up to {}s after the recipe exited and anything written later was dropped",
        PIPE_DRAIN_GRACE.as_secs()
      ),
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

/// How long to wait after SIGTERM before escalating to SIGKILL, for a
/// recipe process group that hasn't exited on its own after an observed
/// `cancel-requested` (just-us#33).
#[cfg(unix)]
const CANCEL_GRACE_PERIOD: Duration = Duration::from_secs(2);

/// Ringmaster job states that end the job (RFC-0018 §2). `wait --on-cancel`
/// only ever stops for one of these or a `cancel-requested` record, so a
/// non-terminal state it returns means the latter was observed.
#[cfg(unix)]
const RINGMASTER_TERMINAL_STATES: &[&str] = &["succeeded", "failed", "aborted", "interrupted"];

/// Watch `job_id` for a cooperative cancel (ringmaster RFC-0018) and, if
/// one arrives before the job ends by any other path, kill `pid`'s whole
/// process group: SIGTERM first, escalating to SIGKILL after
/// `CANCEL_GRACE_PERIOD` if anything in the group is still alive. Sets
/// `cancelled` before signaling, so `run_recipe_async`'s own wait on the
/// same child reports the outcome as an observed cancel rather than an
/// ordinary failure once the signal reaps it (just-us#33: previously
/// nothing observed `cancel-requested` at all, so a cancelled recipe ran
/// to completion).
///
/// `run_recipe_async` spawns its child with `process_group(0)` (the
/// unix-only `CommandExt` extension), so the child's pid IS its process
/// group id — signaling `-pid` reaches it and every descendant it spawns
/// (e.g. `nix develop -c just ...`'s own per-recipe-line child processes),
/// not just the immediate `nix`/`just` process.
///
/// `ringmaster wait <job_id> --on-cancel --json --timeout 0` blocks until
/// EITHER a `cancel-requested` record or any terminal record is written —
/// whichever comes first — so this call also returns (harmlessly) once
/// `run_recipe_async` writes the job's own terminal record on ordinary
/// completion; the terminal-state check below is what tells the two cases
/// apart. A `ringmaster` predating RFC-0018 (rejects `--on-cancel`), or any
/// other failure, is treated the same as "nothing to observe" — this is
/// best-effort cancellation, not a hard dependency of the recipe running.
#[cfg(unix)]
fn spawn_cancel_observer(job_id: String, pid: u32, cancelled: Arc<AtomicBool>) {
  thread::spawn(move || {
    let Ok(output) = ringmaster_command()
      .args(["wait", &job_id, "--on-cancel", "--json", "--timeout", "0"])
      .output()
    else {
      return;
    };

    if !output.status.success() {
      return;
    }

    let Ok(status) = serde_json::from_slice::<serde_json::Value>(&output.stdout) else {
      return;
    };

    let state = status
      .get("state")
      .and_then(serde_json::Value::as_str)
      .unwrap_or_default();

    if RINGMASTER_TERMINAL_STATES.contains(&state) {
      return;
    }

    cancelled.store(true, Ordering::SeqCst);

    signal_process_group(pid, nix::sys::signal::Signal::SIGTERM);

    thread::sleep(CANCEL_GRACE_PERIOD);

    signal_process_group(pid, nix::sys::signal::Signal::SIGKILL);
  });
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
  run_dir: PathBuf,
  recipe_dir: PathBuf,
  flake_dir: Option<PathBuf>,
  impure: bool,
  invocation: Vec<String>,
  recipe: &str,
  timeout: Option<Duration>,
) -> serde_json::Value {
  let job_id = match ringmaster_call(&["start", "--source", "just-us", "--label", recipe]) {
    Ok(job_id) => job_id,
    Err(message) => return tool_error_text(message),
  };

  if job_id.is_empty() {
    return tool_error_text("ringmaster start produced no job id".to_owned());
  }

  let spool_path = ringmaster_call(&["spool-path", &job_id])
    .ok()
    .filter(|path| !path.is_empty())
    .map(PathBuf::from);

  let done_job_id = job_id.clone();
  #[cfg(unix)]
  let cancel_job_id = job_id.clone();
  let cancelled = Arc::new(AtomicBool::new(false));
  let result_cwd = recipe_dir.display().to_string();
  let result_devshell = flake_dir
    .as_ref()
    .map(|dir| dir.join("flake.nix").display().to_string());

  thread::spawn(move || {
    let mut command = recipe_command(run_dir, flake_dir, impure, &invocation);

    // Own process group (unix only): a cancel needs to reach every
    // descendant this spawns (e.g. `nix develop -c just ...`'s own
    // per-recipe-line children), not just the immediate child
    // (just-us#33). `0` sets the new group's id to the child's own pid,
    // which `spawn_cancel_observer` reconstructs from `child.id()`.
    #[cfg(unix)]
    command.process_group(0);

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
        #[cfg(unix)]
        spawn_cancel_observer(cancel_job_id, child.id(), Arc::clone(&cancelled));

        let status = match timeout {
          Some(timeout) => wait_with_timeout(&mut child, timeout),
          None => child.wait().map(Some),
        };

        if cancelled.load(Ordering::SeqCst) {
          ("aborted", "cancelled via ringmaster job_cancel".to_owned())
        } else {
          match status {
            Ok(Some(status)) if status.success() => ("succeeded", "recipe completed".to_owned()),
            Ok(Some(status)) => ("failed", format!("recipe exited with {status}")),
            Ok(None) => ("failed", "recipe timed out".to_owned()),
            Err(io_error) => ("failed", format!("wait failed: {io_error}")),
          }
        }
      }
      Err(io_error) => ("failed", format!("failed to start recipe: {io_error}")),
    };

    let _ = ringmaster_call(&[
      "done",
      &done_job_id,
      "--state",
      state,
      "--message",
      &message,
    ]);
  });

  tool_result_json(serde_json::json!({
    "job_id": job_id,
    "cwd": result_cwd,
    "devshell": result_devshell,
  }))
}
