mod cache;
mod graphql;
mod helpers;
mod progress;
mod resources;
mod tools;

use cache::ResultCache;
use clap::{Parser, Subcommand};
use mcp_server::protocol::ClientCapabilities;
use mcp_server::{Context, McpServer};
use progress::{ProgressSender, store_progress_sender};
use resources::ResultResource;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, mpsc};
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

  run_stdio_with_progress(server).await?;
  cache.cleanup()?;
  Ok(())
}

async fn run_stdio_with_progress(server: McpServer) -> Result<(), Box<dyn std::error::Error>> {
  let stdin = BufReader::new(tokio::io::stdin());
  let stdout = Arc::new(Mutex::new(tokio::io::stdout()));

  let (tx, mut rx) = mpsc::channel::<String>(64);

  let mut ctx = Context::new(
    "just",
    env!("CARGO_PKG_VERSION"),
    ClientCapabilities::default(),
  );
  store_progress_sender(&mut ctx, ProgressSender::new(tx));

  // Spawn notification writer task
  let writer_stdout = stdout.clone();
  let writer_handle = tokio::spawn(async move {
    while let Some(msg) = rx.recv().await {
      let mut out = writer_stdout.lock().await;
      let _ = out.write_all(msg.as_bytes()).await;
      let _ = out.write_all(b"\n").await;
      let _ = out.flush().await;
    }
  });

  // Request handler loop
  let mut lines = stdin.lines();
  while let Some(line) = lines.next_line().await? {
    if line.is_empty() {
      continue;
    }

    let response = server.handle_request(&line, &mut ctx).await;
    let response_json = serde_json::to_string(&response)?;

    let mut out = stdout.lock().await;
    out.write_all(response_json.as_bytes()).await?;
    out.write_all(b"\n").await?;
    out.flush().await?;
  }

  // stdin closed — drop the sender (it's inside ctx which we're about to drop)
  drop(ctx);

  // Wait for notification writer to drain and exit
  let _ = writer_handle.await;

  Ok(())
}
