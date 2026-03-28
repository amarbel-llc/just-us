mod cache;
mod graphql;
mod helpers;
mod resources;
mod tools;

use cache::ResultCache;
use clap::{Parser, Subcommand};
use mcp_server::McpServer;
use resources::ResultResource;
use std::sync::Arc;
use tools::{
  DumpJustfileTool, ListRecipesTool, ListVariablesTool, RunRecipeRequestTool, RunRecipeTool,
  ShowRecipeTool,
};

#[derive(Parser)]
#[command(name = "just-us-agents")]
#[command(about = "MCP server and GraphQL API for justfile operations")]
struct Cli {
  /// Path to the just binary
  #[arg(long, default_value = "just")]
  just_binary: String,

  #[command(subcommand)]
  command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
  /// Run as MCP server over stdio
  Mcp,
  /// Run as GraphQL server over stdio (newline-delimited JSON)
  Graphql,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  let cli = Cli::parse();

  match cli.command.unwrap_or(Command::Mcp) {
    Command::Mcp => run_mcp_server(cli.just_binary).await,
    Command::Graphql => graphql::run_graphql_server(cli.just_binary).await,
  }
}

async fn run_mcp_server(just_binary: String) -> Result<(), Box<dyn std::error::Error>> {
  let cache = Arc::new(ResultCache::new()?);

  let server = McpServer::builder("just", env!("CARGO_PKG_VERSION"))
    .with_tool(ListRecipesTool {
      just_binary: just_binary.clone(),
    })
    .with_tool(ShowRecipeTool {
      just_binary: just_binary.clone(),
    })
    .with_tool(RunRecipeTool {
      just_binary: just_binary.clone(),
      cache: cache.clone(),
    })
    .with_tool(RunRecipeRequestTool {
      just_binary: just_binary.clone(),
      cache: cache.clone(),
    })
    .with_tool(ListVariablesTool {
      just_binary: just_binary.clone(),
    })
    .with_tool(DumpJustfileTool { just_binary })
    .with_resource(ResultResource {
      cache: cache.clone(),
    })
    .build();

  server.run_stdio().await?;
  cache.cleanup()?;
  Ok(())
}
