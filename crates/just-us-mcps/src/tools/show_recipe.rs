use crate::helpers::run_just;
use async_trait::async_trait;
use mcp_server::{Context, Tool, ToolError, ToolResult};
use serde_json::{json, Value};

pub struct ShowRecipeTool {
    pub just_binary: String,
}

#[async_trait]
impl Tool for ShowRecipeTool {
    fn name(&self) -> &str {
        "show_recipe"
    }

    fn description(&self) -> &str {
        "Show the source code of a specific recipe"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "recipe": {
                    "type": "string",
                    "description": "Name of the recipe to show"
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

        let output = run_just(
            &self.just_binary,
            &["--show", recipe],
            working_dir,
            justfile,
        )
        .await
        .map_err(|e| ToolError::ExecutionFailed(e))?;

        if !output.success {
            return Ok(ToolResult::error(format!(
                "just --show failed: {}",
                output.stderr
            )));
        }

        Ok(ToolResult::text(output.stdout))
    }
}
