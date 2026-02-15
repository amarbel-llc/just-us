use crate::helpers::run_just;
use async_trait::async_trait;
use mcp_server::{Context, Tool, ToolError, ToolResult};
use serde_json::{json, Value};

pub struct ListVariablesTool {
    pub just_binary: String,
}

#[async_trait]
impl Tool for ListVariablesTool {
    fn name(&self) -> &str {
        "list_variables"
    }

    fn description(&self) -> &str {
        "List all variables defined in the justfile with their values and export status"
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

        // Get variable metadata from --dump --dump-format json
        let dump_output = run_just(
            &self.just_binary,
            &["--dump", "--dump-format", "json"],
            working_dir,
            justfile,
        )
        .await
        .map_err(|e| ToolError::ExecutionFailed(e))?;

        if !dump_output.success {
            return Ok(ToolResult::error(format!(
                "just --dump failed: {}",
                dump_output.stderr
            )));
        }

        let dump: Value = serde_json::from_str(&dump_output.stdout)
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to parse JSON: {e}")))?;

        // Get evaluated values
        let eval_output = run_just(
            &self.just_binary,
            &["--evaluate"],
            working_dir,
            justfile,
        )
        .await
        .map_err(|e| ToolError::ExecutionFailed(e))?;

        let mut evaluated_values: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();

        if eval_output.success {
            for line in eval_output.stdout.lines() {
                if let Some((name, value)) = line.split_once(" := ") {
                    evaluated_values.insert(
                        name.trim().to_string(),
                        value.trim_matches('"').to_string(),
                    );
                }
            }
        }

        let assignments = dump.get("assignments").cloned().unwrap_or(json!({}));
        let mut result = Vec::new();

        if let Some(obj) = assignments.as_object() {
            for (name, assignment) in obj {
                let mut entry = json!({
                    "name": name,
                });

                if let Some(export) = assignment.get("export") {
                    entry["exported"] = export.clone();
                }

                if let Some(value) = evaluated_values.get(name.as_str()) {
                    entry["value"] = json!(value);
                }

                result.push(entry);
            }
        }

        let output_json =
            serde_json::to_string_pretty(&result).unwrap_or_else(|_| "[]".to_string());

        Ok(ToolResult::text(output_json))
    }
}
