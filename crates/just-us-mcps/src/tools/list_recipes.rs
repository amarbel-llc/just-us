use crate::helpers::run_just;
use async_trait::async_trait;
use mcp_server::{Context, Tool, ToolError, ToolResult};
use serde_json::{json, Value};

pub struct ListRecipesTool {
    pub just_binary: String,
}

#[async_trait]
impl Tool for ListRecipesTool {
    fn name(&self) -> &str {
        "list_recipes"
    }

    fn description(&self) -> &str {
        "List all recipes in the justfile with their parameters, documentation, groups, and dependencies"
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

        let dump: Value = serde_json::from_str(&output.stdout)
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to parse JSON: {e}")))?;

        let recipes = dump.get("recipes").cloned().unwrap_or(json!({}));

        let mut result = Vec::new();
        if let Some(obj) = recipes.as_object() {
            for (name, recipe) in obj {
                let mut entry = json!({
                    "name": name,
                });

                if let Some(doc) = recipe.get("doc") {
                    entry["doc"] = doc.clone();
                }

                if let Some(params) = recipe.get("parameters") {
                    entry["parameters"] = params.clone();
                }

                if let Some(deps) = recipe.get("dependencies") {
                    entry["dependencies"] = deps.clone();
                }

                if let Some(groups) = recipe.get("attributes").and_then(|a| {
                    a.as_array().map(|attrs| {
                        attrs
                            .iter()
                            .filter_map(|attr| {
                                if let Some(obj) = attr.as_object() {
                                    if obj.contains_key("group") {
                                        return obj.get("group").cloned();
                                    }
                                }
                                None
                            })
                            .collect::<Vec<_>>()
                    })
                }) {
                    if !groups.is_empty() {
                        entry["groups"] = json!(groups);
                    }
                }

                if let Some(priv_val) = recipe.get("private") {
                    entry["private"] = priv_val.clone();
                }

                result.push(entry);
            }
        }

        let output_json =
            serde_json::to_string_pretty(&result).unwrap_or_else(|_| "[]".to_string());

        Ok(ToolResult::text(output_json))
    }
}
