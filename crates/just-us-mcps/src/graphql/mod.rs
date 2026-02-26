pub mod schema;
pub mod types;

use crate::helpers::run_just;
use schema::build_schema;
use types::{JustfileDump, Recipe};

use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};

pub async fn run_graphql_server(just_binary: String) -> Result<(), Box<dyn std::error::Error>> {
    let dump_output = run_just(&just_binary, &["--dump", "--dump-format", "json"], None, None).await;

    let recipes: Vec<Recipe> = match dump_output {
        Ok(output) if output.success => {
            let dump: JustfileDump = serde_json::from_str(&output.stdout)?;
            dump.recipes
                .into_iter()
                .map(Recipe::from)
                .collect()
        }
        Ok(output) => {
            return Err(format!("just --dump failed: {}", output.stderr).into());
        }
        Err(e) => {
            return Err(format!("failed to run just: {e}").into());
        }
    };

    let schema = build_schema(recipes);

    let stdin = BufReader::new(io::stdin());
    let mut stdout = io::stdout();
    let mut lines = stdin.lines();

    while let Ok(Some(line)) = lines.next_line().await {
        let request: async_graphql::Request = match serde_json::from_str(&line) {
            Ok(req) => req,
            Err(e) => {
                let error_response = serde_json::json!({
                    "errors": [{"message": format!("invalid request: {e}")}]
                });
                let mut out = serde_json::to_string(&error_response).unwrap();
                out.push('\n');
                stdout.write_all(out.as_bytes()).await?;
                stdout.flush().await?;
                continue;
            }
        };

        let response = schema.execute(request).await;
        let mut out = serde_json::to_string(&response)?;
        out.push('\n');
        stdout.write_all(out.as_bytes()).await?;
        stdout.flush().await?;
    }

    Ok(())
}
