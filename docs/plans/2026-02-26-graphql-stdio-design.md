# GraphQL over stdio for just-us-agents

## Summary

Add a `graphql` subcommand to `just-us-agents` that exposes justfile data as a
GraphQL API over stdio using newline-delimited JSON.

## Architecture

The `just-us-agents graphql` subcommand runs a loop: read a line from stdin,
parse it as a GraphQL request, execute against an async-graphql schema, write the
JSON response to stdout.

Justfile data comes from `just --dump --dump-format json`, called once at startup
for the working directory where the binary was invoked. The parsed JSON is
deserialized into Rust structs that double as GraphQL types via async-graphql
derive macros.

```
stdin (JSONL)  ->  parse GraphQL request  ->  schema.execute()  ->  stdout (JSONL)
                                                   |
                                            just --dump --dump-format json
                                            (called once, cached in memory)
```

## GraphQL Schema

```graphql
type Query {
  recipes: [Recipe!]!
  recipe(name: String!): Recipe
}

type Recipe {
  name: String!
  doc: String
  quiet: Boolean!
  private: Boolean!
  parameters: [Parameter!]!
  dependencies: [Dependency!]!
}

type Parameter {
  name: String!
  kind: ParameterKind!
  default: String
}

enum ParameterKind {
  SINGULAR
  PLUS
  STAR
}

type Dependency {
  recipe: String!
}
```

## Module Structure

```
crates/just-us-mcps/src/
  main.rs          # add clap subcommand: graphql
  graphql/
    mod.rs         # re-exports, run_graphql_server() entry point
    schema.rs      # Query root, async-graphql schema construction
    types.rs       # Recipe, Parameter, Dependency, ParameterKind
                   # (Deserialize from JSON dump + async-graphql derives)
```

Types in `types.rs` derive both `serde::Deserialize` (from `just --dump` JSON)
and `async_graphql::SimpleObject` (for GraphQL).  The dump JSON has recipes as a
map (name -> recipe), so deserialization flattens that into a vec, injecting the
map key as the `name` field.

`main.rs` gets a clap enum with `Mcp` (default, current behavior) and `Graphql`
variants.

## Data Source

Shell out to `just --dump --dump-format json`. Consistent with existing MCP
tools, no coupling to just internals.

## Stdio Protocol

Newline-delimited JSON. Each line is a JSON object:

```json
{"query": "{ recipes { name doc } }", "variables": {}}
```

Response is a single JSON line per query.

## Error Handling

- `just --dump` failure: respond with a GraphQL error, continue listening
- Invalid JSON or invalid GraphQL request: respond with a GraphQL error, don't
  crash
- EOF on stdin: terminate cleanly
- No hot-reloading of justfile data (loaded once at startup)

## Dependencies

- `async-graphql` (schema definition and execution)
- Existing: `tokio`, `serde`, `serde_json`, `clap`
