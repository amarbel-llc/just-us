---
status: prototype (transport superseded)
date: 2026-06-11
---

# FDR 0002: Ambient CRAP Attachment (crap RFC 0002 prototype)

> **Status note (2026-06-12).** crap RFC 0002 was rewritten around a
> client/birthed-server model: nodes are only ever clients of a
> unix-socket sink server birthed by the tree's root (fork-and-become /
> re-exec-self / external binary), which grants each connection a
> disjoint deterministic id base and splices lines into one merged
> stream. That dissolves this prototype's shared-fd machinery —
> `CRAP_FD`, `CRAP_ACCEPT`, `CRAP_DEPTH`, the hello, the dup'd re-offer
> descriptor, random tp bases, and the PIPE_BUF/chunking discipline are
> all withdrawn from the spec. What this prototype validated survives:
> ambient `CRAP=2` detection with silent degradation, `CRAP_PARENT`
> lineage nesting, garbage capture, evaluation-child withdrawal,
> explicit-flag precedence, and acceptance by `crap-present` /
> `:: validate`. Reworking the implementation to `attach()`
> connect-or-birth with granted bases is the tracked next step; until
> then the code below implements the superseded first-draft transport.

just-us is the prototype implementation of the **CRAP attach protocol**
(crap RFC 0002, `amarbel-llc/crap` →
`docs/rfcs/0002-attach-protocol.md`): ambient detection of a
crap-aware harness via a `CRAP=2` environment offer, a one-line JSON
*hello* announcing the negotiated format, and recursive passthrough
re-offers so a tree of producers lands in one nested ndjson-crap
stream. This document records what the prototype implements, what it
deliberately omits, and where it diverges; the RFC is normative.

## What works

```sh
CRAP=2 just build | crap-present
```

- **Ambient activation** (`EventSink::from_ambient`): with
  `--events-fd` / `JUST_EVENTS_FD` absent, a `CRAP=2` offer activates
  the event sink. Channel: `CRAP_FD`, defaulting to stdout. Format:
  first supported token of `CRAP_ACCEPT` (default `ndjson-crap/1`).
  Every defect — unsupported version, no supported format, malformed
  or dead descriptor — degrades silently to a noop sink (RFC §3); only
  the explicit flag errors.
- **Hello** (RFC §5): emitted lazily before the first record (silent
  subcommands stay silent), carrying `version`/`ndjson`/`format`/
  `producer`/`parent`. Never emitted under explicit `--events-fd`,
  whose RFC 0002 contract pins `plan` first.
- **Shared-channel discipline** (RFC §7): ambient tps draw from a
  random base in `[2^40, 2^48)`; every record is serialized to a
  buffer and written with one `write_all`; captured child output is
  chunked at 1024 bytes per `output` record so serialized lines stay
  under `PIPE_BUF`. A producer attached under `CRAP_PARENT` suppresses
  `plan`; root recipes carry `parent` = `CRAP_PARENT` and depths are
  offset by `CRAP_DEPTH`.
- **Passthrough re-offer** (RFC §6.1): for every recipe child (linewise
  and shebang/script paths), just exports `CRAP=2`,
  `CRAP_FD=<dup of the channel>`, `CRAP_ACCEPT=ndjson-crap/1`,
  `CRAP_PARENT=<recipe tp>`, `CRAP_DEPTH=<depth+1>`. A crap-aware
  child (e.g. a nested `just`) attaches and nests; everything else
  emits **garbage** — plain stdout/stderr — which the existing capture
  path wraps as `output` records under the same node. Re-offering
  happens under both ambient and explicit activation, so an
  `--events-fd` consumer gets nested grandchildren too (their records
  are new-to-RFC-0002 types, which its consumers must ignore).
- **Withdraw for data consumption** (RFC §6): backtick / `shell()`
  children have the offer removed (`EventSink::OFFER_VARS`) — their
  stdout is evaluated as a value, and an attaching producer would leak
  records into it.

Verified end to end in this container: nested two-justfile runs under
`CRAP=2` render via `crap-present` (plain fallback) and pass
`:: validate` (16/16 records); the explicit `--events-fd` stream keeps
its plan-first, monotonic-tp shape with the grandchild's hello and
node records interleaved.

## Deliberate omissions / divergences

- **No out-of-band transport** (RFC §8): `inline` only; the hello
  never carries `transport`. Reserved for a future format that needs
  it.
- **No `sid`** in the hello: random tp bases carry the
  collision-avoidance load; add `sid` if consumer-side attribution
  ever needs it.
- **`CRAP_ACCEPT` re-offer is static** (`ndjson-crap/1`), not an echo
  of the negotiated token — equivalent while only one format exists.
- **Explicit `--events-fd` channels also re-offer** even though the
  RFC only requires it of attached producers; the dup'd-descriptor
  mechanism is identical. If a consumer of the explicit stream cannot
  tolerate grandchild records, that consumer predates RFC 0002's
  unknown-type tolerance and is already nonconforming.
- **bats coverage is pending** (bats/nix unavailable in the authoring
  container): the conformance rows in crap RFC 0002 map the planned
  `zz-tests_bats/crap_attach.bats` (tag `crap_attach`); unit coverage
  lives in `src/event.rs`.
- The re-offer dup descriptor leaks (stays open until process exit) —
  harmless single-digit fd cost, simplifies lifetime vs. spawned
  children.

## Implementation map

| Concern | Where |
| --- | --- |
| offer parse, negotiate, hello, tp base, chunking | `src/event.rs` |
| ambient fallback activation | `src/subcommand.rs` |
| root `parent`/`depth` scoping | `src/justfile.rs` (`run`, `run_recipe`) |
| re-offer at spawn, garbage capture chunking | `src/recipe.rs` |
| withdraw for backticks / `shell()` | `src/evaluator.rs` (`run_command`) |
