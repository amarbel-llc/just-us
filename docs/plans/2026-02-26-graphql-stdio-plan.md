# GraphQL stdio Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a `just-us-agents graphql` subcommand that exposes justfile recipes as a GraphQL API over newline-delimited JSON on stdio.

**Architecture:** Shell out to `just --dump --dump-format json` once at startup, deserialize into Rust types that double as async-graphql objects, then loop reading JSONL GraphQL requests from stdin and writing responses to stdout.

**Tech Stack:** async-graphql, tokio, serde, serde_json, clap

---

### Task 1: Add async-graphql dependency

**Files:**
- Modify: `crates/just-us-mcps/Cargo.toml`

**Step 1: Add async-graphql to dependencies**

In `crates/just-us-mcps/Cargo.toml`, add to `[dependencies]`:

```toml
async-graphql = "7"
```

**Step 2: Verify it compiles**

Run: `cargo check -p just-us-mcps`
Expected: compiles with no errors

**Step 3: Commit**

```bash
git add crates/just-us-mcps/Cargo.toml Cargo.lock
git commit -m "chore: add async-graphql dependency to just-us-mcps"
```

---

### Task 2: Create GraphQL types

**Files:**
- Create: `crates/just-us-mcps/src/graphql/types.rs`
- Create: `crates/just-us-mcps/src/graphql/mod.rs`
- Modify: `crates/just-us-mcps/src/main.rs` (add `mod graphql;`)

The `just --dump --dump-format json` output has recipes as a JSON map where keys
are recipe names. Each recipe object looks like:

```json
{
  "name": "watch",
  "doc": null,
  "quiet": false,
  "private": false,
  "parameters": [
    {
      "name": "args",
      "kind": "singular",
      "default": "test",
      "export": false,
      "help": null,
      "long": null,
      "short": null,
      "pattern": null,
      "value": null
    }
  ],
  "dependencies": [
    {
      "recipe": "test",
      "arguments": []
    }
  ]
}
```

Note: `default` can be a string literal like `"test"`, a two-element array like
`["evaluate", "git rev-parse --abbrev-ref HEAD"]` for backtick expressions, or
`null`. For the GraphQL API, stringify non-null defaults: string literals as-is,
backtick expressions as `` `command` ``, and anything else as its JSON
representation.

**Step 1: Create types.rs with GraphQL types and serde deserialization**

Create `crates/just-us-mcps/src/graphql/types.rs`:

```rust
use async_graphql::{Enum, SimpleObject};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize)]
pub struct JustfileDump {
    #[serde(default)]
    pub recipes: HashMap<String, RecipeRaw>,
}

#[derive(Deserialize)]
pub struct RecipeRaw {
    pub doc: Option<String>,
    #[serde(default)]
    pub quiet: bool,
    #[serde(default)]
    pub private: bool,
    #[serde(default)]
    pub parameters: Vec<ParameterRaw>,
    #[serde(default)]
    pub dependencies: Vec<DependencyRaw>,
}

#[derive(Deserialize)]
pub struct ParameterRaw {
    pub name: String,
    #[serde(default)]
    pub kind: String,
    pub default: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct DependencyRaw {
    pub recipe: String,
}

#[derive(SimpleObject, Clone)]
pub struct Recipe {
    pub name: String,
    pub doc: Option<String>,
    pub quiet: bool,
    pub private: bool,
    pub parameters: Vec<Parameter>,
    pub dependencies: Vec<Dependency>,
}

#[derive(SimpleObject, Clone)]
pub struct Parameter {
    pub name: String,
    pub kind: ParameterKind,
    pub default: Option<String>,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum ParameterKind {
    Singular,
    Plus,
    Star,
}

#[derive(SimpleObject, Clone)]
pub struct Dependency {
    pub recipe: String,
}

impl From<(String, RecipeRaw)> for Recipe {
    fn from((name, raw): (String, RecipeRaw)) -> Self {
        Self {
            name,
            doc: raw.doc,
            quiet: raw.quiet,
            private: raw.private,
            parameters: raw.parameters.into_iter().map(Parameter::from).collect(),
            dependencies: raw.dependencies.into_iter().map(Dependency::from).collect(),
        }
    }
}

impl From<ParameterRaw> for Parameter {
    fn from(raw: ParameterRaw) -> Self {
        let kind = match raw.kind.as_str() {
            "plus" => ParameterKind::Plus,
            "star" => ParameterKind::Star,
            _ => ParameterKind::Singular,
        };

        let default = raw.default.map(|v| match &v {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Array(arr) if arr.len() == 2 && arr[0] == "evaluate" => {
                format!("`{}`", arr[1].as_str().unwrap_or_default())
            }
            other => other.to_string(),
        });

        Self {
            name: raw.name,
            kind,
            default,
        }
    }
}

impl From<DependencyRaw> for Dependency {
    fn from(raw: DependencyRaw) -> Self {
        Self { recipe: raw.recipe }
    }
}
```

**Step 2: Create mod.rs**

Create `crates/just-us-mcps/src/graphql/mod.rs`:

```rust
pub mod schema;
pub mod types;
```

**Step 3: Add module declaration to main.rs**

Add `mod graphql;` to the top of `crates/just-us-mcps/src/main.rs`, after the
existing `mod` declarations.

**Step 4: Verify it compiles**

Run: `cargo check -p just-us-mcps`
Expected: compiles (schema module is empty but declared, that's fine — it will
warn about unused; that's OK for now)

**Step 5: Commit**

```bash
git add crates/just-us-mcps/src/graphql/
git commit -m "feat: add graphql types for justfile recipes"
```

---

### Task 3: Create GraphQL schema and query root

**Files:**
- Create: `crates/just-us-mcps/src/graphql/schema.rs`

**Step 1: Create schema.rs with query root**

Create `crates/just-us-mcps/src/graphql/schema.rs`:

```rust
use async_graphql::{Context, EmptyMutation, EmptySubscription, Object, Schema};

use super::types::Recipe;

pub type JustfileSchema = Schema<QueryRoot, EmptyMutation, EmptySubscription>;

pub struct QueryRoot;

#[Object]
impl QueryRoot {
    async fn recipes(&self, ctx: &Context<'_>) -> Vec<Recipe> {
        ctx.data_unchecked::<Vec<Recipe>>().clone()
    }

    async fn recipe(&self, ctx: &Context<'_>, name: String) -> Option<Recipe> {
        ctx.data_unchecked::<Vec<Recipe>>()
            .iter()
            .find(|r| r.name == name)
            .cloned()
    }
}

pub fn build_schema(recipes: Vec<Recipe>) -> JustfileSchema {
    Schema::build(QueryRoot, EmptyMutation, EmptySubscription)
        .data(recipes)
        .finish()
}
```

**Step 2: Verify it compiles**

Run: `cargo check -p just-us-mcps`
Expected: compiles (may warn about unused imports — that's fine for now)

**Step 3: Commit**

```bash
git add crates/just-us-mcps/src/graphql/schema.rs
git commit -m "feat: add graphql schema with recipe queries"
```

---

### Task 4: Add graphql subcommand and stdio server loop

**Files:**
- Modify: `crates/just-us-mcps/src/graphql/mod.rs`
- Modify: `crates/just-us-mcps/src/main.rs`

**Step 1: Add run_graphql_server to mod.rs**

Replace `crates/just-us-mcps/src/graphql/mod.rs` with:

```rust
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
```

**Step 2: Refactor main.rs to add graphql subcommand**

Replace `crates/just-us-mcps/src/main.rs` with:

```rust
mod graphql;
mod helpers;
mod tools;

use clap::{Parser, Subcommand};
use mcp_server::McpServer;
use tools::{DumpJustfileTool, ListRecipesTool, ListVariablesTool, RunRecipeTool, ShowRecipeTool};

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
```

**Step 3: Verify it compiles**

Run: `cargo check -p just-us-mcps`
Expected: compiles with no errors

**Step 4: Commit**

```bash
git add crates/just-us-mcps/src/main.rs crates/just-us-mcps/src/graphql/mod.rs
git commit -m "feat: add graphql subcommand with stdio server loop"
```

---

### Task 5: Build and manual smoke test

**Files:** none (test only)

**Step 1: Build the binary**

Run: `cargo build -p just-us-mcps`
Expected: builds successfully

**Step 2: Smoke test the graphql subcommand**

Run from the repo root (which has a justfile):

```bash
echo '{"query":"{ recipes { name doc } }"}' | cargo run -p just-us-mcps -- graphql
```

Expected: a single JSON line containing `"data"` with a `"recipes"` array, each
entry having `"name"` and `"doc"` fields.

**Step 3: Test recipe lookup by name**

```bash
echo '{"query":"{ recipe(name: \"test\") { name doc quiet private parameters { name kind default } dependencies { recipe } } }"}' | cargo run -p just-us-mcps -- graphql
```

Expected: a JSON response with the `test` recipe's details.

**Step 4: Test invalid input**

```bash
echo 'not json' | cargo run -p just-us-mcps -- graphql
```

Expected: a JSON error response with `"invalid request"` message.

**Step 5: Verify MCP mode still works**

```bash
echo '{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0.1.0"}},"id":1}' | cargo run -p just-us-mcps
```

Expected: JSON-RPC response (MCP still works as default).

**Step 6: Commit (if any fixes were needed)**

Only if changes were made during smoke testing.

---

### Task 6: Nix build verification

**Files:** none

**Step 1: Build with nix**

Run: `nix build .#just-us-agents`
Expected: builds successfully

**Step 2: Smoke test nix output**

```bash
echo '{"query":"{ recipes { name } }"}' | ./result/bin/just-us-agents graphql
```

Expected: JSON response with recipe names.
