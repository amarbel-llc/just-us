mod helpers;
mod tools;

use clap::{Parser, Subcommand};
use mcp_server::McpServer;
use std::process::Command;
use tools::{DumpJustfileTool, ListRecipesTool, ListVariablesTool, RunRecipeTool, ShowRecipeTool};

#[derive(Parser)]
#[command(name = "just-us-mcp-server")]
#[command(about = "MCP server providing justfile operations as tools")]
struct Cli {
    /// Path to the just binary
    #[arg(long, default_value = "just")]
    just_binary: String,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Install just-us-mcp-server as MCP server in Claude Code
    InstallClaude,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::InstallClaude) => install_claude()?,
        None => run_server(cli.just_binary).await?,
    }

    Ok(())
}

fn install_claude() -> Result<(), Box<dyn std::error::Error>> {
    let exe_path = std::env::current_exe()?;

    // Remove existing just MCP server (ignore errors if it doesn't exist)
    let _ = Command::new("claude")
        .args(["mcp", "remove", "just"])
        .status();

    let status = Command::new("claude")
        .args([
            "mcp",
            "add",
            "just",
            "--",
            exe_path.to_str().unwrap_or("just-us-mcp-server"),
        ])
        .status()?;

    if status.success() {
        println!("Successfully installed just-us-mcp-server as MCP server 'just'");
        println!("To verify, run: claude mcp list");
        Ok(())
    } else {
        Err("Failed to install MCP server".into())
    }
}

async fn run_server(just_binary: String) -> Result<(), Box<dyn std::error::Error>> {
    let server = McpServer::builder("just-us-mcp-server", env!("CARGO_PKG_VERSION"))
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
