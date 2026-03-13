
- [x] add support for latest tap14 amendments
- [x] add support for tap14+streamed_output
- [ ] migrate just-us-agents from rust-lib-mcp to purse-first rust-mcp package
- [ ] add `completions::bash` to nix `skippedTests` — the test runs `tests/completions/just.bash` which sources the generated bash completion script and exercises `compgen`/`complete` (bash programmable-completion builtins). These builtins aren't available in the nix devshell's bash, so the test always fails with `complete: command not found` / `compgen: command not found`. Fix: add `"completions::bash"` to the `skippedTests` list in `flake.nix:55`.
