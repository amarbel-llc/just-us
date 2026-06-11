---
status: experimental
date: 2026-06-11
---

# `just --events-fd` NDJSON Event Stream

## Abstract

This document specifies the wire format of the event stream produced by
`just --events-fd <FD>`: a newline-delimited JSON (NDJSON) stream of
recipe-execution events written to a caller-supplied file descriptor.
The stream lets a supervising process — an agent harness, a TUI, a CI
annotator — observe recipe structure, per-command progress, captured
child output, and per-recipe outcomes in real time, without parsing
`just`'s human-oriented terminal output.

## Introduction

`just` interleaves its own diagnostics and child-process output on
stdout/stderr in a format designed for humans. Programs that supervise
`just` runs (agent harnesses, structured loggers, progress UIs) have no
reliable way to recover which recipe is running, which command produced
which bytes, or how each recipe ended.

The `--events-fd` flag adds a machine-readable side channel. The caller
opens a file descriptor, passes its number to `just`, and receives one
JSON record per execution event. The human-oriented stdout/stderr
contract is deliberately altered while the stream is active (see
[Suppressing Inherited stdout/stderr](#suppressing-inherited-stdoutstderr));
with the flag absent, behavior is byte-identical to upstream `just`.

This specification covers the activation contract, the record schema,
and the ordering guarantees of stream version 1. It is implemented by
[just-us], the amarbel-llc fork of [casey/just]. Design history and
known limitations are recorded in the companion feature record
[FDR 0001].

## Requirements Language

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT",
"SHOULD", "SHOULD NOT", "RECOMMENDED", "MAY", and "OPTIONAL" in this
document are to be interpreted as described in RFC 2119.

## Specification

### Activation

The stream is activated by the `--events-fd <FD>` command-line option
or the `JUST_EVENTS_FD` environment variable. `<FD>` is the integer
number of a file descriptor inherited from the parent process.

- The descriptor MUST be open and writable (access mode `O_WRONLY` or
  `O_RDWR`). The producer MUST validate this before executing any
  recipe; an invalid descriptor MUST produce a diagnostic naming the
  descriptor and a non-zero exit, and no recipe may have run.
- The producer takes ownership of the descriptor and closes it on
  exit. Callers MUST NOT assume the descriptor is usable after `just`
  terminates.
- `--events-fd` is only supported on Unix targets. On other targets
  the producer MUST fail activation rather than silently ignore the
  flag.
- When the flag is absent, the producer MUST behave identically to
  unmodified `just`: no descriptor is touched and child stdio is
  inherited as usual.

Events are produced only by recipe-running invocations (`just
<recipe>`, `--choose`, command/evaluate forms). Other subcommands
(`--list`, `--dump`, `--fmt`, ...) accept and validate the flag but
write nothing to the stream.

### Document Format

The stream is newline-delimited JSON: each record is one JSON object
serialized on a single line, encoded as UTF-8, terminated by a single
`\n` (0x0A).

- Each record MUST carry a `type` field whose value is one of
  `plan`, `recipe_start`, `recipe_command`, `output`, or
  `recipe_complete`. All field names are `snake_case`.
- The first record on the stream MUST be a [plan record](#plan-record).
- The producer MUST flush after each record and MUST NOT interleave
  bytes of different records, including when recipes execute
  concurrently.
- After successful activation, a write failure on the events
  descriptor MUST NOT abort recipe execution; the producer drops the
  record and continues. Consumers requiring lossless capture should
  provide a descriptor that cannot fail mid-run (a regular file or a
  pipe they keep draining).
- Consumers MUST ignore record types they do not recognize and MUST
  ignore unrecognized fields on known record types. New record types
  and new fields MAY be added without a version bump; see
  [Compatibility](#compatibility).

### Test Points

Every record except `plan` carries a `tp` ("test point", after TAP)
field: a one-based integer identifying one recipe execution. The
producer MUST allocate `tp` values monotonically starting at 1 and
MUST NOT reuse a value within a stream. All records describing the
same recipe execution carry the same `tp`.

### Plan Record

Emitted once, first on the stream, before any recipe runs.

```json
{"type":"plan","version":1,"recipe_count":2}
```

| Field | Type | Meaning |
|-------|------|---------|
| `version` | integer | Stream schema version. This document specifies version `1`. |
| `recipe_count` | integer | Number of distinct recipes reachable from the requested invocations, per the producer's pre-execution walk of the dependency graph. |

`recipe_count` is advisory, intended for progress UIs. Consumers MUST
NOT assume it equals the number of `recipe_start` records that follow:
a recipe invoked multiple times with different arguments runs (and
starts) more than once but is counted once, and conditional paths may
run fewer.

### Recipe Start Record

Emitted when a recipe execution begins, before its dependencies run.

```json
{"type":"recipe_start","tp":2,"name":"bar","namepath":"bar","depth":1,"parent":1,"doc":"the bar recipe","quiet":false}
```

| Field | Type | Meaning |
|-------|------|---------|
| `tp` | integer | This execution's test point. |
| `name` | string | The recipe's name. |
| `namepath` | string | The recipe's module-qualified path (equals `name` outside submodules). |
| `depth` | integer | Dependency depth; `0` for directly invoked recipes. |
| `parent` | integer or null | `tp` of the recipe that depends on this one; `null` at depth 0. |
| `doc` | string or null | The recipe's doc comment, if any. |
| `quiet` | boolean | Whether the recipe is recipe-level quiet (declared with a `@` prefix). |

A `recipe_start` MUST precede every other record bearing its `tp`, and
a parent's `recipe_start` MUST precede the `recipe_start` of each of
its dependencies.

### Recipe Command Record

Emitted for each logical command line of a linewise recipe body,
before the command executes — consumers can render "about to run X"
without waiting for output.

```json
{"type":"recipe_command","tp":1,"command":"echo hello","line":2}
```

| Field | Type | Meaning |
|-------|------|---------|
| `tp` | integer | The executing recipe's test point. |
| `command` | string | The fully evaluated command text, with leading sigils (`@`, `-`, `?`) stripped. |
| `line` | integer | One-indexed line number in the source justfile where the logical command begins. |

A logical command spanning continuation lines (`\`) is one record,
with `line` naming its first source line. Comment lines and lines that
evaluate to the empty string produce no record. Under `--dry-run`,
`recipe_command` records are still emitted for the commands that would
have run. Shebang recipe bodies execute as a single script and produce
no `recipe_command` records.

### Output Record

Carries bytes a child process wrote to stdout or stderr.

```json
{"type":"output","tp":1,"stream":"stdout","format":"utf8","data":"hello\n"}
```

| Field | Type | Meaning |
|-------|------|---------|
| `tp` | integer | The recipe execution that produced the bytes. |
| `stream` | string | `stdout` or `stderr`. |
| `format` | string | Encoding of `data`: `utf8` or `base64`. |
| `data` | string | The captured bytes. |

- A version-1 producer emits `format":"utf8"` exclusively, replacing
  invalid UTF-8 sequences with U+FFFD. `base64` is reserved for a
  future binary-safe transport; consumers MUST accept `utf8` and
  SHOULD decode `base64` if encountered.
- Zero or more `output` records may appear per `(tp, stream)` pair;
  streams that produced no bytes yield no record. Consumers MUST
  concatenate `data` across records to reconstruct a stream.
- Delivery is not real-time: the version-1 producer buffers and MAY
  emit a command's output only after the command exits. Consumers
  MUST NOT treat the absence of `output` records as evidence that a
  running command is silent.

### Recipe Complete Record

Emitted exactly once per `recipe_start`, as the last record bearing
its `tp`.

```json
{"type":"recipe_complete","tp":1,"exit_code":0,"signal":null,"duration_ms":3}
```

| Field | Type | Meaning |
|-------|------|---------|
| `tp` | integer | The completed execution's test point. |
| `exit_code` | integer or null | Exit status of the recipe; `null` when terminated by a signal. |
| `signal` | string or null | Signal name (e.g. `"SIGINT"`) when signal-terminated; otherwise `null`. |
| `duration_ms` | integer | Wall-clock duration of the execution in milliseconds. |

Success is `exit_code: 0` with `signal: null`. A recipe that fails for
a reason other than a child exit status (e.g. an evaluation error
mid-body) reports `exit_code: 1`.

### Suppressing Inherited stdout/stderr

While the stream is active:

- Recipe child processes MUST NOT inherit `just`'s stdout/stderr;
  their output is captured into [output records](#output-record)
  instead. This applies to linewise commands and shebang recipe
  scripts alike.
- Children receive pipes, not a terminal: `isatty` reports false, so
  color-aware tools will typically disable ANSI output. PTY-based
  capture that preserves terminal semantics is tracked as [#14].
- `just`'s own diagnostics — command echo banners, error messages —
  continue to go to `just`'s stderr and MUST NOT appear in the event
  stream.
- Backtick expressions and variable-assignment evaluation are not
  recipe execution; their subprocess output is consumed by evaluation
  as always and produces no `output` records.
- Caveat: the capture path bypasses `just`'s signal-forwarding
  machinery, so signals delivered to `just` are not forwarded to
  children spawned under capture. Tracked as [#15].

### Ordering

- The `plan` record MUST be first.
- For a given `tp`: `recipe_start` first, then any `recipe_command`
  and `output` records, then `recipe_complete` last. A command's
  `recipe_command` record MUST precede that command's `output`
  records.
- A dependency's `recipe_complete` precedes the parent's subsequent
  body records when dependencies run sequentially.
- Under parallel dependency execution, records of different `tp`s MAY
  interleave arbitrarily (per-record atomicity still holds). Consumers
  MUST demultiplex by `tp` rather than assume contiguity.

## Security Considerations

The event stream duplicates information that already crosses the
process boundary (command text and child output), but routes it to a
descriptor the caller controls. Two points deserve attention:

- **Quiet does not mean secret.** Recipe- and line-level quiet (`@`)
  and `--quiet` suppress *echo*, not capture: evaluated command text
  appears in `recipe_command` records and child output appears in
  `output` records regardless of quiet settings. Recipes that rely on
  quiet to keep secrets off the terminal will leak them into the
  stream. Consumers MUST treat the stream with at least the
  sensitivity of the underlying terminal session, and SHOULD NOT
  persist it to shared locations by default.
- **Descriptor trust.** The producer writes to whatever descriptor
  number it is handed and cannot verify where it leads; the caller is
  responsible for the destination. Validation establishes only that
  the descriptor is open and writable.

`data` fields are JSON-encoded by a conforming serializer; consumers
MUST parse records with a JSON parser rather than pattern-match raw
lines, as command text and output can contain arbitrary content
including `"`-escaped sequences and text resembling records.

## Conformance Testing

Conformance tests for this specification live in
`zz-tests_bats/events_fd.bats` (file tag `events_fd`). The binary
under test is injected via the `JUST_BIN` environment variable,
defaulting to `just` on `PATH`; `just test-bats` runs the suite
hermetically under the nix sandbox, `just test-bats-local` against
`target/debug/just`.

### Covered Requirements

| Requirement | Test | Description |
|-------------|------|-------------|
| §Activation, flag absent ⇒ unchanged behavior | `no --events-fd: existing behavior unchanged` | stdout passthrough without the flag |
| §Activation, invalid fd MUST fail before recipes | `--events-fd N where N is an unopened fd fails before any recipe runs` | marker file proves no recipe ran |
| §Document Format, plan MUST be first | `--events-fd 3 writes a plan record with version 1 as the first event` | first line is `plan`, `version` 1 |
| §Suppressing Inherited stdout/stderr | `--events-fd active: child stdout does not pass through` | child bytes only in `output` records |
| §Recipe Start/Complete fields | `--events-fd emits recipe_start and recipe_complete per recipe` | field-level assertions incl. `doc`, `quiet` |
| §Recipe Command Record, 1-indexed lines, pre-execution | `recipe_command per command line, before execution` | continuation collapsing, line numbers |
| §Ordering, parent before child, child complete before parent body | `dep ordering: parent recipe_start before child events` | `depth`/`parent` fields, cross-`tp` order |

## Compatibility

- With `--events-fd` absent, `just-us` MUST be behaviorally identical
  to the upstream `just` commit it tracks.
- Version-1 streams may gain new record types and new fields on
  existing record types without a version bump; consumers MUST
  tolerate both (see [Document Format](#document-format)). The
  `version` field on the `plan` record increments only for changes
  that alter the meaning or shape of existing fields.
- The buffered delivery of `output` records is a permitted weakening
  in version 1, not a contract: a future producer MAY stream output
  incrementally (multiple records per command, emitted while it runs)
  without a version bump. Consumers conforming to
  [Output Record](#output-record) are unaffected.

## References

### Normative

- [RFC 2119] — Key words for use in RFCs to Indicate Requirement
  Levels.

### Informative

- [casey/just] — upstream project: <https://github.com/casey/just>
- [just-us] — implementing fork:
  <https://github.com/amarbel-llc/just-us>
- [FDR 0001] — `docs/features/0001-events-fd.md`, design intent and
  limitations.
- [#14] — PTY-based capture followup:
  <https://github.com/amarbel-llc/just-us/issues/14>
- [#15] — signal forwarding under capture:
  <https://github.com/amarbel-llc/just-us/issues/15>
- TAP — the Test Anything Protocol, origin of the "test point"
  terminology: <https://testanything.org>

[RFC 2119]: https://www.rfc-editor.org/rfc/rfc2119
[casey/just]: https://github.com/casey/just
[just-us]: https://github.com/amarbel-llc/just-us
[FDR 0001]: ../features/0001-events-fd.md
[#14]: https://github.com/amarbel-llc/just-us/issues/14
[#15]: https://github.com/amarbel-llc/just-us/issues/15
