---
status: experimental
date: 2026-06-11
promotion-criteria: at least one external consumer (tap-dancer or crap)
  drives a real workload off the stream without requesting schema
  changes; no version-1 field semantics revised for 4 weeks
---

# `--events-fd`: machine-readable recipe-execution events

## Problem Statement

Programs that supervise `just` runs — agent harnesses, TUIs, CI
annotators — can only observe an interleaved human-oriented terminal
stream, with no reliable way to tell which recipe is running, which
command produced which bytes, or how each recipe ended. Parsing that
output is guesswork that breaks on quiet flags, color, and recipe
chrome. They need a structured side channel that does not disturb
`just`'s human-facing behavior when unused.

## Interface

`just --events-fd <FD> <recipe>` (or `JUST_EVENTS_FD=<FD>`) writes
newline-delimited JSON records to file descriptor `<FD>`, which the
caller opens and passes in. The wire format — record types `plan`,
`recipe_start`, `recipe_command`, `output`, `recipe_complete`, their
fields, and ordering guarantees — is specified normatively in
[RFC 0002](../rfcs/0002-just-events-fd-stream.md).

Observable behavior:

- Without the flag, behavior is byte-identical to the upstream `just`
  commit the fork tracks. The feature is strictly additive.
- With the flag, the descriptor is validated (open + writable) before
  anything runs; a bad descriptor is a clean non-zero exit naming the
  fd, and no recipe has executed.
- While active, recipe child output is *captured into the stream*
  rather than passed through to `just`'s stdout/stderr. The terminal
  shows `just`'s own chrome (command echo, errors); the bytes children
  write arrive as `output` records on the fd. This redirection is the
  point of the feature: the supervising process decides what to
  render.
- Each recipe execution gets a one-based `tp` ("test point") number;
  dependency structure is reported via `depth`/`parent` so a consumer
  can rebuild the execution tree, and `recipe_command` records carry
  1-indexed justfile line numbers for source mapping.

## Examples

A justfile with one dependency:

    hello: dep
        @echo hello-from-recipe

    dep:
        @echo dep-output

Run with the stream on fd 3, drained to a file:

    $ just --events-fd 3 hello 3>events.ndjson
    $ cat events.ndjson
    {"type":"plan","version":1,"recipe_count":2}
    {"type":"recipe_start","tp":1,"name":"hello","namepath":"hello","depth":0,"parent":null,"doc":null,"quiet":false}
    {"type":"recipe_start","tp":2,"name":"dep","namepath":"dep","depth":1,"parent":1,"doc":null,"quiet":false}
    {"type":"recipe_command","tp":2,"command":"echo dep-output","line":5}
    {"type":"output","tp":2,"stream":"stdout","format":"utf8","data":"dep-output\n"}
    {"type":"recipe_complete","tp":2,"exit_code":0,"signal":null,"duration_ms":1}
    {"type":"recipe_command","tp":1,"command":"echo hello-from-recipe","line":2}
    {"type":"output","tp":1,"stream":"stdout","format":"utf8","data":"hello-from-recipe\n"}
    {"type":"recipe_complete","tp":1,"exit_code":0,"signal":null,"duration_ms":3}

A bad descriptor fails before any recipe runs:

    $ just --events-fd 99 hello
    error: --events-fd 99 is not a writable file descriptor: Bad file descriptor (os error 9)

## Limitations

- **Buffered output.** Version 1 captures via pipes and emits a
  command's `output` records after the command exits — a long-running
  command is silent on the stream until it finishes. Live streaming
  and TTY semantics are [#14](https://github.com/amarbel-llc/just-us/issues/14).
- **No TTY for children.** Captured children see `isatty == false`,
  so color-aware tools drop ANSI output. Also [#14] — the naive PTY
  implementation hangs the nix-sandboxed test lane and was reverted.
- **No signal forwarding under capture.** The capture path bypasses
  `just`'s signal-forwarding machinery; SIGINT to `just` is not
  forwarded to captured children
  ([#15](https://github.com/amarbel-llc/just-us/issues/15)).
- **Quiet is not redaction.** `@`-quiet and `--quiet` suppress
  terminal echo, not capture: command text and output land in the
  stream regardless. Treat the stream as sensitively as the terminal.
- **Unix only.** Activation fails on non-Unix targets rather than
  silently ignoring the flag.
- **Recipe granularity stops at the shell.** Shebang recipe bodies run
  as one script: no per-command `recipe_command` records. Backticks
  and variable assignments are evaluation, not execution, and emit
  nothing.

## Tuning Levers

| Lever | Current | Rationale | Change signal |
|---|---|---|---|
| output delivery | buffered, one record per (command, stream) | `Command::output()` is simple and sandbox-safe; PTY attempt hung the bats lane | a consumer needs progress from long-running commands (→ [#14]) |
| `recipe_count` semantics | unique recipes from pre-walk, advisory | avoids duplicating `Ran`'s per-(recipe, args) dedup before execution | a consumer needs an exact denominator for progress display |
| `format: base64` | reserved, never emitted | UTF-8-lossy covers observed consumers; binary-safe path kept open in schema | a consumer needs byte-exact output (checksums, binary artifacts) |

## More Information

- [RFC 0002](../rfcs/0002-just-events-fd-stream.md) — normative wire
  format; conformance suite in `zz-tests_bats/events_fd.bats`.
- [#11](https://github.com/amarbel-llc/just-us/issues/11) (closed) —
  original externalization plan that scoped RFC 0002.
- [#13](https://github.com/amarbel-llc/just-us/issues/13) — upstream
  submission tracking. Upstream `casey/just` is not accepting pull
  requests ([casey/just#3227](https://github.com/casey/just/issues/3227)),
  so the upstream artifact is a feature-request issue pointing at this
  FDR and RFC 0002; `just-us` remains the maintained implementation.
- Known consumers: `amarbel-llc/tap` (tap-dancer TAP-14 wrapper,
  [tap#32](https://github.com/amarbel-llc/tap/issues/32)) and
  `amarbel-llc/crap`
  ([crap#3](https://github.com/amarbel-llc/crap/issues/3)).

[#14]: https://github.com/amarbel-llc/just-us/issues/14
[#15]: https://github.com/amarbel-llc/just-us/issues/15
