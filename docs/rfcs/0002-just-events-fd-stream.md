---
status: proposed
date: 2026-05-24
---

# Just Recipe Execution Event Stream

## Abstract

This RFC specifies a newline-delimited JSON (NDJSON) wire format for
reporting recipe execution events from the `just` command runner. When
invoked with `--events-fd N`, `just` emits one JSON record per line to
file descriptor `N` for each observable event during recipe execution:
plan, recipe start, command line about to execute, captured output
chunk, recipe completion. The format provides a stable contract that
external tools consume to render TAP-14, attribute observability data,
drive dashboards, or apply other presentation strategies without
modifying `just` itself.

## Introduction

`just` executes recipes by inheriting its own stdout and stderr into
child processes. This is the right default for interactive use but
gives external observers no structured handle on what is happening:
which recipe is running, when it started, when it completed, what its
exit code was, which command line within the recipe produced which
output bytes.

Several downstream presentations of recipe execution have shown the
same shape: collect what `just` is doing, render it in a different
format. TAP-14 emission is one such presentation; CI dashboards,
structured logs, and build telemetry are others. Each currently
requires either patching `just` or screen-scraping inherited stdout
with attribution heuristics.

A small structured-event side channel solves this. The event stream is
sufficient for any downstream presenter to reconstruct execution
without modifying `just` per-presenter. The TAP-14 emission pathway
that previously lived inside `just` becomes one external consumer
among many.

This document specifies the wire format and the contract `just` makes
about it. It does not specify the consumer side --- TAP-14 emission,
NDJSON conformance, or terminal rendering --- those are the consumer's
responsibility.

Related documents:

- [RFC 0001: TAP Test-Result NDJSON Schema](./0001-test-result-ndjson-schema.md) --- the schema consumers convert this stream into
- TAP-14 specification (`tap-version-14-specification.md`) --- target format for one class of consumer

## Requirements Language

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT",
"SHOULD", "SHOULD NOT", "RECOMMENDED", "MAY", and "OPTIONAL" in this
document are to be interpreted as described in RFC 2119.

## Specification

### Activation

`just` MUST accept a `--events-fd N` command-line option where `N` is a
positive integer file descriptor inherited from the parent process.
When this option is present, `just` MUST emit the event stream
specified by this RFC to that file descriptor. When this option is
absent, `just` MUST NOT change its existing behavior in any way
observable on stdout or stderr.

The descriptor MUST be writable. If `just` cannot write to `N` (e.g.,
the descriptor is not open, is not writable, or the parent did not
inherit it), `just` MUST exit with a non-zero status before executing
any recipe and MUST write a diagnostic to its own stderr explaining
the failure. `just` MUST NOT silently disable the event stream.

`just` MUST close the descriptor before exiting normally. On abnormal
termination (signal, panic), the descriptor MAY remain open; consumers
MUST treat EOF or read error as terminal.

### Document Format

A conforming event stream is a sequence of records, each encoded as
one JSON object followed by a single line feed (U+000A). Records MUST
be encoded as UTF-8. Producers MUST NOT emit byte order marks. Records
MUST NOT contain unescaped line feeds within their JSON encoding.

Every record MUST contain a `type` field whose value identifies the
record type. Consumers MUST use this field to discriminate. Producers
MAY emit record types not specified by this RFC only in a future
revision that supersedes this one; until then, producers MUST NOT emit
records with unspecified `type` values.

The event stream MUST begin with exactly one `plan` record. It MAY end
with any record type. Consumers MUST treat EOF or read error as the
end of the stream.

### Record Types

#### Plan Record

A `plan` record reports the count of recipes that will be invoked
during this execution, including all dependencies, computed by walking
the dependency graph from the requested target(s) before any recipe
runs. Its fields are:

| Field | Type | Required | Description |
|---|---|---|---|
| `type` | string | MUST | Constant `"plan"`. |
| `recipe_count` | integer | MUST | Count of recipes that will be invoked. Each recipe in this count will produce exactly one `recipe_start` event followed by exactly one `recipe_complete` event, unless execution is aborted. |

The `recipe_count` MUST count each unique recipe invocation, not
unique recipe definitions: if `foo` depends on `bar` and `baz`, and
the user runs `just foo`, the count is 3. If a single recipe is
requested multiple times via dependencies, it MUST be counted once
per invocation that will actually execute (after `just`'s
deduplication rules apply).

#### Example

```json
{"type":"plan","recipe_count":3}
```

#### Recipe Start Record

A `recipe_start` record indicates that `just` is about to begin
executing a recipe. Its fields are:

| Field | Type | Required | Description |
|---|---|---|---|
| `type` | string | MUST | Constant `"recipe_start"`. |
| `tp` | integer | MUST | Test point number, in the range `[1, recipe_count]`, pre-assigned during the dependency graph pre-walk. Used to correlate all subsequent events for this recipe. Producers MUST assign `tp` values such that no two recipes share the same value within one execution. |
| `name` | string | MUST | The recipe's name as written in the justfile. |
| `namepath` | string | MUST | The recipe's fully qualified path, joining module names with `::`. Equal to `name` for recipes in the top-level module. |
| `depth` | integer | MUST | Depth in the dependency tree: 0 for recipes invoked directly from the command line, 1 for their direct dependencies, and so on. |
| `parent` | integer \| null | MUST | The `tp` value of the recipe that depends on this one, or `null` if this recipe was invoked directly from the command line. Consumers MAY use `parent` to reconstruct the dependency tree. |
| `doc` | string \| null | MUST | The recipe's doc comment if present, or `null`. Producers MUST emit the resolved doc string (after `[doc(...)]` attribute resolution). |
| `quiet` | boolean | MUST | `true` if the recipe was declared with `@` prefix or `[quiet]` attribute. |

#### Example

```json
{"type":"recipe_start","tp":1,"name":"test","namepath":"test","depth":0,"parent":null,"doc":"run all tests","quiet":false}
```

#### Recipe Command Record

A `recipe_command` record reports one command line about to be
executed within a recipe body. Its fields are:

| Field | Type | Required | Description |
|---|---|---|---|
| `type` | string | MUST | Constant `"recipe_command"`. |
| `tp` | integer | MUST | The `tp` value of the recipe this command belongs to. |
| `command` | string | MUST | The evaluated command line, after variable interpolation and continuation joining. Equivalent to what `just` would print to stderr when verbose. |
| `line` | integer | MUST | 1-indexed line number in the source justfile where this command begins. |

Producers MUST emit one `recipe_command` record per executed command,
in execution order. Producers MAY emit zero `recipe_command` records
for script-style recipes (those declared with `[script]` or a shebang
line); in that case the entire script is opaque from the event
stream's perspective.

#### Example

```json
{"type":"recipe_command","tp":1,"command":"cargo test --all","line":42}
```

#### Output Record

An `output` record carries a chunk of bytes that the recipe's child
process(es) wrote to stdout or stderr. Its fields are:

| Field | Type | Required | Description |
|---|---|---|---|
| `type` | string | MUST | Constant `"output"`. |
| `tp` | integer | MUST | The `tp` value of the recipe this output belongs to. |
| `stream` | string | MUST | Either `"stdout"` or `"stderr"`. Producers SHOULD distinguish; producers MAY emit all output as `"stdout"` when the underlying capture mechanism cannot separate streams (e.g., PTY allocation merges them). |
| `data` | string | MUST | The captured byte chunk, encoded as a JSON string per the encoding rules below. |

Producers MUST preserve the byte order of captured output. Producers
SHOULD emit chunks at natural boundaries (read returns, line breaks)
to maximize liveness for consumers, but MUST NOT split UTF-8 sequences
across chunks unless required by an unrecoverable I/O constraint.
Consumers MUST handle multiple `output` records per recipe and MUST
concatenate `data` fields in record order to reconstruct the captured
stream.

Producers MUST capture output from a pseudo-terminal (PTY) when the
operating system provides one, so that ANSI escape sequences emitted
by color-aware children are preserved. Producers SHOULD fall back to
pipes when PTY allocation is unavailable.

#### Encoding of Output Data

The `data` field is a JSON string. Producers MUST encode captured
bytes according to the following rules:

- If the chunk is valid UTF-8, producers MUST emit it as a JSON string
  with the standard JSON escapes for `"`, `\`, control characters, and
  the line-separator and paragraph-separator code points. ANSI escape
  sequences MUST be preserved as `\u001b` followed by the remaining
  bytes of the sequence.
- If the chunk contains invalid UTF-8, producers MUST replace each
  invalid byte sequence with the Unicode replacement character
  U+FFFD. Producers MUST NOT emit invalid UTF-8 in the resulting JSON
  string.

Producers requiring exact-byte fidelity for non-text payloads MAY
emit a future record type with base64-encoded data; that capability
is out of scope for this revision.

#### Example

```json
{"type":"output","tp":1,"stream":"stdout","data":"   Compiling foo v0.1.0\n"}
{"type":"output","tp":1,"stream":"stdout","data":"\u001b[32m    Finished\u001b[0m dev profile\n"}
{"type":"output","tp":1,"stream":"stderr","data":"warning: unused variable `x`\n"}
```

#### Recipe Complete Record

A `recipe_complete` record indicates that a recipe finished executing.
Its fields are:

| Field | Type | Required | Description |
|---|---|---|---|
| `type` | string | MUST | Constant `"recipe_complete"`. |
| `tp` | integer | MUST | The `tp` value of the recipe that completed. |
| `exit_code` | integer \| null | MUST | The exit code reported by the recipe's final command, or `null` if the recipe was terminated by a signal. For multi-line recipes, this is the exit code of the last command that ran (a non-zero exit aborts the recipe unless infallible). |
| `signal` | string \| null | MUST | The name of the signal that terminated the recipe (e.g., `"SIGINT"`, `"SIGTERM"`) if `exit_code` is `null`, otherwise `null`. |
| `duration_ms` | integer | MUST | Elapsed wall-clock time in milliseconds from `recipe_start` to `recipe_complete`. |

Exactly one `recipe_complete` record MUST be emitted for each
`recipe_start` record, unless `just` is itself terminated abnormally
before the recipe finishes. Consumers MUST treat the absence of a
`recipe_complete` record (relative to a `recipe_start`) as an
abnormal termination.

#### Example: Success

```json
{"type":"recipe_complete","tp":1,"exit_code":0,"signal":null,"duration_ms":1230}
```

#### Example: Failure

```json
{"type":"recipe_complete","tp":1,"exit_code":1,"signal":null,"duration_ms":48}
```

#### Example: Killed by Signal

```json
{"type":"recipe_complete","tp":1,"exit_code":null,"signal":"SIGINT","duration_ms":312}
```

### Event Ordering

Events MUST appear on the stream in the following order:

1. Exactly one `plan` record, before any other event.
2. For each recipe invocation, in execution order:
   a. Exactly one `recipe_start` record.
   b. Zero or more `recipe_command` and `output` records interleaved,
      in observed order, all tagged with the same `tp`.
   c. Exactly one `recipe_complete` record.

When recipes execute in parallel (via `[parallel]` dependencies),
events for different recipes MAY interleave. The `tp` tag on every
non-`plan` event correlates the event back to its recipe. Consumers
MUST handle interleaving correctly. Within a single recipe, the
`recipe_start`, body events, and `recipe_complete` MUST appear in
their respective order; producers MUST NOT reorder events for the
same `tp`.

Dependency invocation order is part of the contract: for a recipe
`foo` that depends on `bar` and `baz`, the producer MUST emit
`recipe_start` for `foo` (depth 0) before any event for `bar` or
`baz` (depth 1, parent = `foo`'s tp), and MUST emit
`recipe_complete` for `bar` and `baz` before any `recipe_command`,
`output`, or `recipe_complete` for `foo`'s own body.

### Unknown Fields

Future revisions of this schema MAY add fields to existing record
types. Consumers MUST ignore unknown fields they do not recognize.
Consumers MUST NOT reject records on the basis of unknown fields.

Producers MUST NOT emit fields not specified by this RFC or by a
future revision that supersedes it.

### Field Ordering

Producers SHOULD emit fields in the order specified by the tables
above. Consumers MUST NOT depend on field order, since JSON object
member order is not significant per RFC 8259.

### Suppressing Inherited stdout/stderr

When `--events-fd` is active, producers MUST capture child process
output and MUST NOT pass it through to `just`'s own stdout or stderr.
Consumers reading the event stream and presenting output themselves
MUST be the sole sink. This guarantees consumers can format output
without interleaving with raw child bytes on the terminal.

The presence or absence of `--events-fd` is the sole switch governing
this behavior; there is no separate flag to enable or disable child
output passthrough when `--events-fd` is active.

## Security Considerations

The `output` records contain arbitrary bytes from child processes,
including potentially adversarial content from test fixtures or user
input. Consumers that display `output` data to a terminal SHOULD
strip all `ESC [` CSI sequences except SGR (color) before display to
prevent injection of terminal control codes such as cursor movement,
screen clears, or window-title manipulation.

The `command` field on `recipe_command` records contains the
post-evaluation command text, including any interpolated variables.
If a variable holds adversarial content, that content appears
verbatim in the event stream. Consumers MUST sanitize `command`
values per the conventions of any context they pass them into
(shells, file paths, HTTP requests, etc.).

The schema makes no claims about authentication, integrity, or
confidentiality. Consumers requiring these properties MUST apply them
at the transport layer.

## Conformance Testing

Conformance tests for this specification SHOULD live in
`zz-tests_bats/` alongside other TAP-related tests. Reference test
file: `zz-tests_bats/events_fd.bats`.

### Covered Requirements

| Requirement | Description |
|---|---|
| Activation | Verifies `--events-fd N` writes to the named fd and `just` exits non-zero if the fd is invalid. |
| Document Format | Verifies UTF-8 encoding, one record per line, no BOM. |
| Plan Record | Verifies `plan` appears first and `recipe_count` matches actual recipe invocations. |
| Recipe Start / Complete pairing | Verifies one `recipe_start` and one `recipe_complete` per recipe invocation. |
| Event Ordering | Verifies dependency ordering: parent's `recipe_start` precedes children's events; children's `recipe_complete` precedes parent's own body events. |
| Output Capture | Verifies `output` records carry child bytes faithfully (ANSI preserved, no UTF-8 corruption). |
| Parallel Interleaving | Verifies parallel recipes produce correctly tagged interleaved events. |
| stdout/stderr Suppression | Verifies child output does not appear on `just`'s own stdout/stderr when `--events-fd` is active. |
| Signal Termination | Verifies `recipe_complete` records `exit_code: null` and `signal: "SIG..."` when a child is killed by signal. |

## Compatibility

This is the initial version of the event stream contract. No
backwards-compatibility constraints apply.

Future revisions MUST be backwards-compatible according to the
following rules:

- New fields MAY be added to any record type.
- Existing fields MUST NOT be removed.
- Existing fields' types MUST NOT change.
- New record types MAY be added; consumers MUST ignore record types
  they do not recognize.

Incompatible changes MUST be specified in a new RFC that supersedes
this one. A schema version field is intentionally omitted in favor of
the unknown-fields and unknown-record-types rules above, consistent
with [RFC 0001].

## References

### Normative

- [RFC 2119] Bradner, S., "Key words for use in RFCs to Indicate
  Requirement Levels", BCP 14, RFC 2119, March 1997
- [RFC 8259] Bray, T., Ed., "The JavaScript Object Notation (JSON)
  Data Interchange Format", STD 90, RFC 8259, December 2017

### Informative

- [RFC 0001: TAP Test-Result NDJSON Schema](./0001-test-result-ndjson-schema.md) --- downstream consumer schema
- TAP-14 specification (`tap-version-14-specification.md` in this repo)
- `just` source repository: <https://github.com/casey/just>
