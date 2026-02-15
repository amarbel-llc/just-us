use crate::helpers::run_just;
use async_trait::async_trait;
use mcp_server::{Context, Tool, ToolError, ToolResult};
use serde_json::{json, Value};

pub struct RunRecipeTool {
    pub just_binary: String,
}

#[async_trait]
impl Tool for RunRecipeTool {
    fn name(&self) -> &str {
        "run_recipe"
    }

    fn description(&self) -> &str {
        "Execute a recipe from the justfile with optional arguments, variable overrides, and dry-run mode"
    }

    fn input_schema(&self) -> Value {
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
            "required": ["recipe"]
        })
    }

    async fn execute(&self, arguments: Value, _ctx: &Context) -> Result<ToolResult, ToolError> {
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
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let overrides: Vec<String> = arguments
            .get("overrides")
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
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

        let output = run_just(&self.just_binary, &args, working_dir, justfile)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e))?;

        let result = json!({
            "stdout": output.stdout,
            "stderr": output.stderr,
            "success": output.success,
        });

        let result_text = serde_json::to_string_pretty(&result)
            .unwrap_or_else(|_| format!("stdout: {}\nstderr: {}", output.stdout, output.stderr));

        if output.success {
            Ok(ToolResult::text(result_text))
        } else {
            Ok(ToolResult::error(result_text))
        }
    }
}
