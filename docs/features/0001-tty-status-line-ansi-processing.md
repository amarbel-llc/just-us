---
status: accepted
date: 2026-03-14
promotion-criteria: all edge cases from real-world ~/eng justfile runs are handled without visual artifacts
---

# TTY Status Line ANSI Processing

## Problem Statement

When streaming child process output as transient TAP comment lines (`# ...`)
in `tty-build-last-line` mode, programs like `nix` emit raw terminal control
sequences that break the status line rendering. Output contains embedded
carriage returns (`\r`) for in-place progress updates, ANSI escape sequences
for colors and line erasure, and lines longer than the terminal width that wrap
and persist. Each of these must be handled to produce clean single-line status
output.

## Interface

In `TapStreamedOutput` mode (the default for `just-me`), child process
stdout/stderr is captured via a PTY and rendered as transient status lines
prefixed with `# `. The following ANSI processing is applied to each chunk of
output before display:

### 1. Line Delimiter: Split on `\r` and `\n`

Programs use two different line-ending conventions:

- **`\n` (LF)** --- standard line terminator. The PTY translates this to
  `\r\n` in the output stream.
- **`\r` (CR)** --- used by progress indicators (nix, curl, cargo) to
  overwrite the current line without advancing.

The stream callback splits on **both** `\r` and `\n` as delimiters. This
ensures each progress update becomes its own status line with a proper `# `
prefix.

**Why not just `\n`?** When only splitting on `\n`, a nix progress sequence
like `evaluating...\r\x1b[Kwarning: ...\n` becomes a single "line" containing
an embedded `\r`. When rendered as `\r\x1b[2K# evaluating...\r\x1b[K
warning:...`, the embedded `\r` moves the cursor back to column 0 and
`\x1b[K` erases the `# ` prefix --- the warning appears without its comment
marker.

**Handling `\r\n` pairs:** Splitting on `\r` first produces the content, then
splitting on the following `\n` produces an empty segment that is skipped.
This correctly handles PTY-translated line endings.

### 2. Empty Line Filtering: Visible Content Detection

After splitting, each segment is checked for **visible content** --- not just
non-empty bytes. A line consisting entirely of ANSI escape sequences (e.g.
`\x1b[0m\x1b[K`) is treated as empty because it renders as nothing visible on
screen.

The `has_visible_content()` function walks the string character by character:

- **`\x1b` (ESC)** followed by `[` starts a CSI sequence. All parameter bytes
  (`0x30`--`0x3F`), intermediate bytes (`0x20`--`0x2F`), and the final byte
  (`0x40`--`0x7E`) are consumed.
- **Whitespace and ASCII control characters** are skipped.
- **Any other character** means the line has visible content.

Lines without visible content are silently dropped, preserving the previous
meaningful status line on screen until real content arrives.

### 3. Autowrap Suppression: `DECAWM` Disable/Enable

Long status lines (e.g. flakehub URLs) can exceed the terminal width. When
the terminal wraps text to the next line, the subsequent `\r\x1b[2K` (move to
column 0, erase line) only clears the **last wrapped line** --- the upper
portion persists as a visual artifact.

Each status line is bracketed with DEC Autowrap Mode control:

```
\r\x1b[2K\x1b[?7l# {content}\x1b[?7h
```

- `\x1b[?7l` --- disable autowrap (DECAWM reset). Content exceeding terminal
  width is clipped at the right edge.
- `\x1b[?7h` --- re-enable autowrap after the status line is written.

This is only emitted when stdout is a TTY (`is_terminal()` check). Piped
output omits the DECAWM sequences since line wrapping is not a concern.

### 4. Status Line Erasure Before Test Points

Before each TAP test point (`ok`/`not ok`) is written, the current status
line is cleared with `\r\x1b[2K` to ensure the transient status does not
appear alongside the permanent test result.

### 5. Verbose YAML Output Block Filtering

When `-v`/`--verbose` is passed, YAML output blocks are included alongside
streamed status lines. The raw PTY output in these blocks is cleaned:

- Trailing `\r` stripped from each line (PTY `\n` to `\r\n` translation
  artifact)
- Empty/blank lines filtered out
- Only applied in `TapStreamedOutput` mode; buffered TAP output is
  unmodified

## Examples

A nix build emitting progress updates and warnings:

```
# Child process (nix) writes to PTY:
evaluating derivation '...'\r\x1b[K
downloading 'https://...'\r\x1b[K
warning: updating lock file\n

# Rendered status lines (each overwrites previous):
\r\x1b[2K\x1b[?7l# evaluating derivation '...'\x1b[?7h
\r\x1b[2K\x1b[?7l# downloading 'https://...'\x1b[?7h
\r\x1b[2K\x1b[?7l# warning: updating lock file\x1b[?7h
\r\x1b[2Kok 1 - build-nix
```

A long URL that would wrap without DECAWM suppression:

```
# Without \x1b[?7l: URL wraps, upper portion persists after clear
# 'https://api.flakehub.com/f/pinned/nix-community/fenix/0.1.2375...
ok 7 - build-rcm

# With \x1b[?7l: URL clipped at terminal edge, fully cleared
ok 7 - build-rcm
```

Verbose mode showing YAML blocks:

```
$ just-me -v build
# hello
ok 1 - build
  ---
  output: "hello"
  ...
```

## Limitations

- **ANSI-only lines produce no status update.** A child process that emits
  only reset/erase sequences between meaningful lines will show a stale status
  until the next visible line arrives. This is intentional --- displaying `# `
  with no visible text is worse.
- **Extra whitespace from ANSI sequences.** Some commands emit ANSI sequences
  between visible words that `trim()` does not collapse, causing extra spaces
  in the `# ` output. This is a known issue tracked in TODO.md.
- **DECAWM disable/enable is not crash-safe.** If the process is killed
  between `\x1b[?7l` and `\x1b[?7h`, the terminal may be left with autowrap
  disabled. In practice, terminals reset this on program exit, and the
  disable/enable pair is within a single `write!` call.
- **CSI sequence parsing is best-effort.** The `has_visible_content()` parser
  handles standard CSI sequences (`ESC [ ... <final>`) but does not handle
  OSC, DCS, or other less common escape sequence types. Unrecognized sequences
  may be treated as visible content.
