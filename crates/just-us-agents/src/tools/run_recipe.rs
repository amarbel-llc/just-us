use crate::cache::{self, ResultCache};
use crate::helpers::run_just;
use async_trait::async_trait;
use mcp_server::{Context, Tool, ToolError, ToolResult};
use serde_json::{json, Value};
use std::sync::Arc;

const INLINE_THRESHOLD: usize = 50;
const SUMMARY_LINES: usize = 5;

pub(crate) fn recipe_input_schema() -> Value {
  json!({
      "type": "object",
      "properties": {
          "recipe": {
              "type": "string",
              "description": "Name of the recipe to run"
          },
          "arguments": {
              "type": "array",
              "items": { "type": "string" },
              "description": "Positional arguments to pass to the recipe"
          },
          "overrides": {
              "type": "object",
              "additionalProperties": { "type": "string" },
              "description": "Variable overrides as key=value pairs"
          },
          "dry_run": {
              "type": "boolean",
              "description": "If true, show what would be executed without running it"
          },
          "working_directory": {
              "type": "string",
              "description": "Working directory to search for the justfile"
          },
          "justfile": {
              "type": "string",
              "description": "Path to a specific justfile"
          }
      },
      "required": ["recipe"],
      "dependentRequired": {
          "working_directory": ["justfile"]
      }
  })
}

pub(crate) async fn execute_recipe(
  just_binary: &str,
  arguments: &Value,
  result_cache: &ResultCache,
) -> Result<ToolResult, ToolError> {
  let recipe = arguments
    .get("recipe")
    .and_then(|v| v.as_str())
    .ok_or_else(|| ToolError::InvalidArguments("recipe is required".into()))?;

  let working_dir = arguments.get("working_directory").and_then(|v| v.as_str());
  let justfile = arguments.get("justfile").and_then(|v| v.as_str());

  let dry_run = arguments
    .get("dry_run")
    .and_then(|v| v.as_bool())
    .unwrap_or(false);

  let recipe_args: Vec<String> = arguments
    .get("arguments")
    .and_then(|v| v.as_array())
    .map(|arr| {
      arr
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect()
    })
    .unwrap_or_default();

  let overrides: Vec<String> = arguments
    .get("overrides")
    .and_then(|v| v.as_object())
    .map(|obj| {
      obj
        .iter()
        .map(|(k, v)| format!("{}={}", k, v.as_str().unwrap_or_default()))
        .collect()
    })
    .unwrap_or_default();

  let mut args: Vec<&str> = Vec::new();

  if dry_run {
    args.push("--dry-run");
  }

  let override_refs: Vec<&str> = overrides.iter().map(|s| s.as_str()).collect();
  args.extend(&override_refs);

  args.push(recipe);

  let arg_refs: Vec<&str> = recipe_args.iter().map(|s| s.as_str()).collect();
  args.extend(&arg_refs);

  let output = run_just(just_binary, &args, working_dir, justfile)
    .await
    .map_err(ToolError::ExecutionFailed)?;

  let full_output = json!({
      "stdout": output.stdout,
      "stderr": output.stderr,
      "success": output.success,
  });

  let full_text = serde_json::to_string_pretty(&full_output)
    .unwrap_or_else(|_| format!("stdout: {}\nstderr: {}", output.stdout, output.stderr));

  let line_count = output.stdout.lines().count();

  if line_count < INLINE_THRESHOLD {
    if output.success {
      return Ok(ToolResult::text(full_text));
    } else {
      return Ok(ToolResult::error(full_text));
    }
  }

  // Long output: store in cache, return summary + resource link
  let git_commit = cache::git_commit_short(working_dir).await;
  let cache_path = result_cache.cache_path(working_dir, justfile, &git_commit, &args);

  result_cache
    .store(&cache_path, &full_text)
    .map_err(|e| ToolError::ExecutionFailed(format!("failed to cache result: {e}")))?;

  let path_digest = cache_path
    .parent()
    .and_then(|p| p.file_name())
    .and_then(|n| n.to_str())
    .unwrap_or("unknown");

  let filename = cache_path
    .file_name()
    .and_then(|n| n.to_str())
    .unwrap_or("unknown");

  let uri = cache::cache_uri(path_digest, filename);

  let status = if output.success { "succeeded" } else { "failed" };
  let lines: Vec<&str> = output.stdout.lines().collect();

  let first_lines: String = lines
    .iter()
    .take(SUMMARY_LINES)
    .copied()
    .collect::<Vec<_>>()
    .join("\n");

  let last_lines: String = lines
    .iter()
    .rev()
    .take(SUMMARY_LINES)
    .rev()
    .copied()
    .collect::<Vec<_>>()
    .join("\n");

  let summary = format!(
    "Recipe `{recipe}` {status} ({line_count} lines).\n\
     First {n} lines:\n{first_lines}\n\n\
     Last {n} lines:\n{last_lines}\n\n\
     Full output: {uri}",
    n = SUMMARY_LINES,
  );

  let result = if output.success {
    ToolResult::text(summary)
  } else {
    ToolResult::error(summary)
  };

  Ok(result)
}

pub struct RunRecipeTool {
  pub just_binary: String,
  pub cache: Arc<ResultCache>,
}

#[async_trait]
impl Tool for RunRecipeTool {
  fn name(&self) -> &str {
    "run-recipe"
  }

  fn description(&self) -> &str {
    "Execute an always-allowed recipe from the justfile with optional arguments, variable overrides, and dry-run mode"
  }

  fn input_schema(&self) -> Value {
    recipe_input_schema()
  }

  async fn execute(&self, arguments: Value, _ctx: &Context) -> Result<ToolResult, ToolError> {
    let recipe = arguments
      .get("recipe")
      .and_then(|v| v.as_str())
      .ok_or_else(|| ToolError::InvalidArguments("recipe is required".into()))?;

    let working_dir = arguments.get("working_directory").and_then(|v| v.as_str());
    let justfile = arguments.get("justfile").and_then(|v| v.as_str());

    let permission =
      crate::helpers::get_agent_permission(&self.just_binary, recipe, working_dir, justfile).await;

    match permission.as_str() {
      "never-allowed" => {
        return Ok(ToolResult::error(format!(
                    "Recipe `{recipe}` has attribute `[agents(\"never-allowed\")]` and cannot be run by agents"
                )));
      }
      "per-request" => {
        return Ok(ToolResult::error(format!(
          "Recipe `{recipe}` requires user confirmation (agents attribute is `per-request`). \
                     Use `run-recipe-request` to execute per-request recipes."
        )));
      }
      _ => {}
    }

    execute_recipe(&self.just_binary, &arguments, &self.cache).await
  }
}
