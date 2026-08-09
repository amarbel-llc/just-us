---
status: prototype
date: 2026-06-12
---

# FDR 0002: Ambient CRAP Attachment (crap RFC 0002 prototype)

just-us is the prototype implementation of the **CRAP attach protocol**
(crap RFC 0002, `amarbel-llc/crap` →
`docs/rfcs/0002-attach-protocol.md`), in its client/birthed-server
form: ambient detection of a crap-aware harness via a `CRAP=2`
environment offer, every node a *client* of a unix-socket sink server,
roots *birthing* that server by re-exec'ing the just binary into an
embedded serve loop, per-connection grants handing out disjoint
deterministic id bases, and terminal roots electing one shared server
per tty by atomic bind. This document records what the prototype
implements, what it deliberately omits, and where it diverges; the RFC
is normative.

> An earlier revision of this prototype implemented the RFC's
> superseded first draft (shared-fd passthrough: `CRAP_FD`, a hello
> record, random tp bases, PIPE_BUF write discipline). That transport
> is gone; the semantics it validated carried forward.

## What works

```sh
CRAP=2 just build | crap-present   # root births a tree server
CRAP=2 just build > run.ndjson     # ...whose stdout is the recorder
CRAP=2 just build                  # tty: join-or-elect the terminal server
```

- **Attachment** (`crap_attach::attach`, RFC §3): with `--events-fd` /
  `JUST_EVENTS_FD` absent and a `CRAP=2` offer present, just connects
  to the inherited `CRAP_SINK` and reads the grant. Failures divide per
  the RFC: *no server there* (missing socket, refused) ⇒ re-root;
  *a server said no* (deny grant, EOF-before-grant, malformed grant,
  foreign format) ⇒ unattached, never a surprise server. Attachment is
  gated to recipe-running subcommands (`run`/`choose`/command/evaluate)
  so `--list` and friends never touch a sink, and happens at startup —
  before any recipe — satisfying connect-before-spawn.
- **Root election** (RFC §3 step 3): stdout-is-a-tty ⇒ join-or-elect
  the tty-keyed terminal server (`tty-<maj>.<min>.sock`, bind = win,
  `EADDRINUSE` = lose and connect; stale paths reclaimed under a
  sidecar `flock`). Otherwise ⇒ birth a private tree server whose
  merged output is the inherited stdout. A re-rooted node clears any
  inherited `CRAP_PARENT` (it named a node in the dead tree's id
  namespace).
- **The embedded server** (`crap_serve`, RFC §6): this same binary
  re-exec'd with an internal marker env var (the RFC's "re-exec self"
  embodiment — Go-style, no fork hazards, no external dependency). It
  receives the pre-bound listener (no readiness race) and splices
  complete lines from all connections to its output in arrival order —
  never parsing or reordering records. Grants are
  `{"type":"crap","version":2,"base":k·2^32,"format":"ndjson-crap/1"}`
  in accept order, so the first client (normally the root) emits plain
  `tp: 1, 2, 3…` and a single-producer stream is byte-identical to an
  `--events-fd` stream. Tree servers exit on lease EOF (a pipe whose
  write end the spawner holds for life — survives SIGKILL) with a 2s
  drain grace; terminal servers are refcounted (1s linger after the
  last disconnect, 10s first-client deadline) and ignore
  SIGINT/SIGHUP so ^C and pty teardown still reach the
  drain/flush/unlink path. Terminal servers pipe their output into
  `crap-present` when it is on PATH (presentation), falling back to a
  raw splice.
- **Scoping children** (RFC §5): each recipe child gets
  `CRAP`/`CRAP_SINK`/`CRAP_PARENT=<recipe tp>` in its environment — a
  crap-aware child connects to the sink *itself* (its records never
  pass through just) and nests; everything else emits **garbage**,
  which the capture path wraps as `output` records chunked at 8 KiB
  (the RFC's 64 KiB line-hygiene bound with worst-case JSON-escaping
  headroom). Backtick / `shell()` children get the offer withdrawn
  (`EventSink::OFFER_VARS`) — evaluation is not execution.
- **Explicit `--events-fd`** is untouched: direct descriptor writes,
  plan-first, monotonic tps, error on invalid fd, no scoping env for
  children (an fd is not a connectable sink address).

Verified end to end in this container: a nested two-justfile run under
`CRAP=2` produces one merged stream (root tps `1..n`, inner just at
base 2^32 parented under the outer recipe's node, nested plan
suppressed) that passes `:: validate` (14/14) and renders via
`crap-present`; a dead `CRAP_SINK` re-roots into a complete standalone
stream; two parallel roofless justs on one pty converge on a single
terminal server as sibling top-level trees with disjoint bases; the
terminal server feeds `crap-present` when present; servers exit and
unlink their sockets after their tree/clients end.

## Deliberate omissions / divergences

- **No scope check at accept** (RFC §6.1 SHOULD): the server grants
  any same-user peer; peer-credential ancestry / ctty verification is
  pending. Until then, deny grants are emitted only for protocol
  errors, not trespass.
- **fork-and-become** (RFC §6.2's first embodiment) is not used —
  re-exec-self is simpler and immune to fork-in-threaded-process
  hazards. The serve loop is structured to permit a fork-safe
  embedding later (poll loop, no threads).
- **Terminal raw-splice fallback** prints ndjson at a human when
  `crap-present` is absent — RFC-permitted last resort.
- The lease write end and the server child handle are deliberately
  leaked/unwaited: the lease must live exactly as long as the process,
  and the server outlives us by design (init reaps it).
- **bats coverage is pending** (bats/nix unavailable in the authoring
  container): the RFC's conformance table is the spec for
  `zz-tests_bats/crap_attach.bats` (tag `crap_attach`); unit coverage
  lives in `src/event.rs`, `src/crap_attach.rs`, and
  `src/crap_serve.rs` (including a real-socket grant/splice/lease
  test).

## Implementation map

| Concern | Where |
| --- | --- |
| attach procedure, grant exchange, root election, birth | `src/crap_attach.rs` |
| serve loop: grants, splice, lease/refcount lifetimes, presenter | `src/crap_serve.rs` |
| serve-mode entry hook (before arg parsing) | `src/run.rs` |
| `EventSink` ambient construction, granted-base tps, child scoping env, plan suppression | `src/event.rs` |
| ambient gating to recipe-running subcommands | `src/subcommand.rs` |
| root `parent` scoping | `src/justfile.rs` (`run`) |
| scoping env at spawn, garbage capture chunking | `src/recipe.rs` |
| withdraw for backticks / `shell()` | `src/evaluator.rs` (`run_command`) |
