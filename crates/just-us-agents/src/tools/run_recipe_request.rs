use crate::helpers::get_agent_permission;
use crate::tools::run_recipe::{execute_recipe, recipe_input_schema};
use async_trait::async_trait;
use mcp_server::{Context, Tool, ToolError, ToolResult};
use serde_json::Value;

pub struct RunRecipeRequestTool {
    pub just_binary: String,
}

#[async_trait]
impl Tool for RunRecipeRequestTool {
    fn name(&self) -> &str {
        "run_recipe_request"
    }

    fn description(&self) -> &str {
        "Execute a per-request recipe that requires user confirmation. For always-allowed recipes, use `run_recipe` instead."
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

        let permission = get_agent_permission(
            &self.just_binary,
            recipe,
            working_dir,
            justfile,
        )
        .await;

        match permission.as_str() {
            "never-allowed" => {
                return Ok(ToolResult::error(format!(
                    "Recipe `{recipe}` has attribute `[agents(\"never-allowed\")]` and cannot be run by agents"
                )));
            }
            "always-allowed" => {
                return Ok(ToolResult::error(format!(
                    "Recipe `{recipe}` is always-allowed. Use `run_recipe` instead."
                )));
            }
            _ => {}
        }

        execute_recipe(&self.just_binary, &arguments).await
    }
}
