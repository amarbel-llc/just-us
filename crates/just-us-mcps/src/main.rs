mod helpers;
mod tools;

use clap::Parser;
use mcp_server::McpServer;
use tools::{DumpJustfileTool, ListRecipesTool, ListVariablesTool, RunRecipeTool, ShowRecipeTool};

#[derive(Parser)]
#[command(name = "just-us-agents")]
#[command(about = "MCP server providing justfile operations as tools")]
struct Cli {
    /// Path to the just binary
    #[arg(long, default_value = "just")]
    just_binary: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    run_server(cli.just_binary).await
}

async fn run_server(just_binary: String) -> Result<(), Box<dyn std::error::Error>> {
    let server = McpServer::builder("just-us-agents", env!("CARGO_PKG_VERSION"))
        .with_tool(ListRecipesTool {
            just_binary: just_binary.clone(),
        })
        .with_tool(ShowRecipeTool {
            just_binary: just_binary.clone(),
        })
        .with_tool(RunRecipeTool {
            just_binary: just_binary.clone(),
        })
        .with_tool(ListVariablesTool {
            just_binary: just_binary.clone(),
        })
        .with_tool(DumpJustfileTool { just_binary })
        .build();

    server.run_stdio().await?;
    Ok(())
}
