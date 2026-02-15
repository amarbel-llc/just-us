use crate::helpers::run_just;
use async_trait::async_trait;
use mcp_server::{Context, Tool, ToolError, ToolResult};
use serde_json::{json, Value};

pub struct DumpJustfileTool {
    pub just_binary: String,
}

#[async_trait]
impl Tool for DumpJustfileTool {
    fn name(&self) -> &str {
        "dump_justfile"
    }

    fn description(&self) -> &str {
        "Dump the full JSON representation of the justfile including all recipes, variables, settings, and modules"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "working_directory": {
                    "type": "string",
                    "description": "Working directory to search for the justfile"
                },
                "justfile": {
                    "type": "string",
                    "description": "Path to a specific justfile"
                }
            }
        })
    }

    async fn execute(&self, arguments: Value, _ctx: &Context) -> Result<ToolResult, ToolError> {
        let working_dir = arguments.get("working_directory").and_then(|v| v.as_str());
        let justfile = arguments.get("justfile").and_then(|v| v.as_str());

        let output = run_just(
            &self.just_binary,
            &["--dump", "--dump-format", "json"],
            working_dir,
            justfile,
        )
        .await
        .map_err(|e| ToolError::ExecutionFailed(e))?;

        if !output.success {
            return Ok(ToolResult::error(format!(
                "just --dump failed: {}",
                output.stderr
            )));
        }

        Ok(ToolResult::text(output.stdout))
    }
}
