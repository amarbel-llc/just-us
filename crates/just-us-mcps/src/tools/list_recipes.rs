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
        "List recipes in the justfile categorized by agent permission. Returns two categories: \
         'carte_blanche' (always-allowed, can be run freely) and 'bureaucratic' (per-request, \
         require user confirmation). Recipes marked never-allowed are excluded entirely."
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

        let mut carte_blanche = Vec::new();
        let mut bureaucratic = Vec::new();

        if let Some(obj) = recipes.as_object() {
            for (name, recipe) in obj {
                let mut agent_permission = "per-request".to_string();
                if let Some(attrs) = recipe.get("attributes").and_then(|a| a.as_array()) {
                    for attr in attrs {
                        if let Some(value) = attr.get("agents").and_then(|v| v.as_str()) {
                            agent_permission = value.to_string();
                            break;
                        }
                    }
                }

                if agent_permission == "never-allowed" {
                    continue;
                }

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

                match agent_permission.as_str() {
                    "always-allowed" => carte_blanche.push(entry),
                    _ => bureaucratic.push(entry),
                }
            }
        }

        let output_json = serde_json::to_string_pretty(&json!({
            "carte_blanche": carte_blanche,
            "bureaucratic": bureaucratic,
        }))
        .unwrap_or_else(|_| "{}".to_string());

        Ok(ToolResult::text(output_json))
    }
}
