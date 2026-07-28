---
status: experimental
date: 2026-07-28
promotion-criteria: the `justfile-orphan-summary` linter is enabled
  across the eng fleet, not just-us alone, with no revision to the
  field's delimiters or shape during that rollout; no consumer needs
  the prelude in a form other than a source-ordered list of lines
---

# `doc_prelude`: the recipe comment lines `--list` discards

## Problem Statement

`just --list` derives a recipe's description from exactly one comment
line — the last one immediately above the recipe. A recipe introduced
by a block of contiguous comment lines therefore lists as that final
line alone, which is usually a mid-sentence fragment (`strip it`,
`which also sit at column 0`). Nothing warns: the justfile parses, it
reads correctly in an editor, and only the `--list` column and shell
completion show the damage. The condition was undetectable from
outside `just`, because the discarded lines never left the parser.

## Interface

`doc_prelude` is a per-recipe field of
`just --dump --dump-format json`. It carries the comment lines that
sit above a recipe's doc-comment line and are therefore invisible to
`--list`. There is no flag and no setting; the field is always
computed and is part of the JSON dump's shape.

The field contract:

- The value is the run of **content-bearing** comment lines
  immediately above the doc-comment line, in **source order** —
  topmost line first. Each entry is the comment's text with the `#`
  and surrounding whitespace trimmed.
- The run terminates at the first of:
  - a bare `#` line — one whose text after the `#` is empty or
    whitespace only — which is the sanctioned way to write a block of
    prose and still give `--list` a standalone summary;
  - a blank line;
  - any non-comment item, including a comment trailing an item on the
    same line;
  - the start of the file.
- A `[doc(...)]` attribute always yields an empty value. The
  attribute is what `--list` prints, so comment lines above it
  truncate nothing.
- A recipe with no doc comment at all yields an empty value.
- **The key is omitted from the JSON when the value is empty.**
  Consumers MUST treat an absent key as empty. In `jq`,
  `null | length` is 0, so both `.doc_prelude | length > 0` and the
  more explicit `(.doc_prelude // []) | length > 0` behave correctly
  whether or not the key is present.

A non-empty `doc_prelude` means exactly one thing: this recipe's
`--list` description is a truncated fragment of a longer block.

Recipes are the only carrier. Module items compute the doc comment
the same way but discard the prelude — `--list` renders submodules
from `doc` alone, and the rule the field exists to serve is scoped to
recipes.

The addition is **strictly additive**, and verified as such:

- `doc` is unchanged: still the single last comment line.
- `--list` output is unchanged.
- `--fmt` output is unchanged. The prelude comments remain ordinary
  comment items in the AST — the parser only *inspects* them, never
  pops them — so `--fmt` re-emits them verbatim.
- The JSON dump of a justfile with no orphaned prelude is
  byte-identical to upstream's, because the key is skipped when empty
  (`tests/json.rs::doc_prelude_key_omitted_when_empty`).

## Examples

A recipe whose comment block runs straight into it:

    # Rewrite version.env (canonical source of truth) and the matching
    # Cargo.toml version line. Pure mutation; scoped to [package] so it
    # never touches [dependencies] version lines,
    # which also sit at column 0.
    bump-version new_version:
        @echo {{new_version}}

`--list` shows the tail of the block, not a summary:

    $ just --list
    Available recipes:
        bump-version new_version # which also sit at column 0.

and the dump reports the three lines it swallowed:

    $ just --dump --dump-format json \
        | jq '.recipes["bump-version"] | {doc, doc_prelude}'
    {
      "doc": "which also sit at column 0.",
      "doc_prelude": [
        "Rewrite version.env (canonical source of truth) and the matching",
        "Cargo.toml version line. Pure mutation; scoped to [package] so it",
        "never touches [dependencies] version lines,"
      ]
    }

Separating the prose from the summary with a bare `#` line fixes the
description and empties the prelude — which is then omitted entirely:

    # Rewrite version.env and Cargo.toml together; scoped to [package]
    # so it never touches [dependencies] version lines.
    #
    # rewrite the fork version in version.env and Cargo.toml
    bump-version new_version:
        @echo {{new_version}}

    $ just --list
    Available recipes:
        bump-version new_version # rewrite the fork version in version.env and Cargo.toml

    $ just --dump --dump-format json \
        | jq -c '.recipes["bump-version"] | {doc, has_prelude: has("doc_prelude")}'
    {"doc":"rewrite the fork version in version.env and Cargo.toml","has_prelude":false}

## Why a lint and not a behavior change

The obvious alternative was to change `just` itself: join the
contiguous comment lines into a single multi-line doc string, so that
nothing is discarded and `--list` shows the whole block. That was
considered and **rejected**.

`--list` already renders embedded newlines in a description, so
joining would not be a no-op for existing justfiles. Every recipe in
the fleet that today carries a multi-line comment block — hundreds of
them — would silently switch from a one-line `--list` entry to a
multi-line one, reflowing every listing and every shell-completion
description at once. That is a far larger blast radius than a lint,
and the result it lands on is worse: a completion menu and a `--list`
column want a short summary, and a wrapped paragraph of rationale is
not a description.

Reporting the condition instead leaves `--list` byte-for-byte as it
was and puts the fix where it belongs — editorially, in the justfile,
one recipe at a time, at whatever pace each repo chooses.

## Consumer

The field's only consumer is `justfile-orphan-summary`, a conformist
linter that lives in this repo at
`nix/linters/justfile-orphan-summary.nix` and fails any non-private
recipe with a non-empty `doc_prelude`. It has no `repair-command` on
purpose: mechanically inserting a bare `#` would satisfy the check
while leaving `--list` showing the same useless fragment, so the fix
is editorial.

The linter ships from just-us rather than conformist because it must
run the fork's binary — an upstream `just` never emits the key and
the check would pass vacuously. It is exported as the
system-independent module path
`lib.conformistLinters.justfile-orphan-summary`, and a downstream
repo wires it alongside conformist's own presets:

    imports = [ just-us.lib.conformistLinters.justfile-orphan-summary ];
    linters.justfile-orphan-summary.enable = true;
    linters.justfile-orphan-summary.justPackage =
      just-us.packages.${system}.default;

`justPackage` is mandatory and has no default precisely because the
exported path cannot close over a system-specific derivation, and
because defaulting it to `pkgs.just` would silently disable the rule.

just-us dogfoods the linter in its own `flake.nix`. The fleet rollout
it enables is staged: sweep each repo's justfile comments first, then
enable the linter, so the check never lands red.

## Limitations

- **Fork-only.** Upstream `just` does not emit the key, so any
  consumer must run a just-us build. There is no runtime way to tell
  a stock `just` from the fork other than the key's absence, which is
  indistinguishable from "no findings".
- **Detection, not repair.** The field says a description is
  truncated; it cannot say what the summary should have been.
- **Not a quality check.** A single-line comment that is itself a bad
  summary yields an empty prelude and passes. The field only sees the
  structural case where `--list` is showing part of a longer block.
- **Recipes only.** Module items discard the prelude, so a module
  whose `doc` is a truncated fragment is not detectable this way.
- **No source positions.** Entries are trimmed text, not spans, so a
  consumer cannot point at a line number without re-reading the
  justfile.

## Tuning Levers

| Lever | Current | Rationale | Change signal |
|---|---|---|---|
| separator vocabulary | bare `#` line or blank line terminate the run | both already read as a paragraph break to a human; accepting only one would fail justfiles that are already correct | a fleet sweep finds a third separator style in common use |
| whitespace-only comment | treated as bare `#` (terminates) | `#` followed by trailing spaces is the same authorial intent and is invisible in an editor | none expected |
| omitted when empty | key skipped via `skip_serializing_if` | keeps every existing dump byte-identical, so no downstream JSON consumer churns | a consumer finds the absent-vs-empty distinction more error-prone than the churn would have been |
| entry shape | trimmed text, source order | matches what a human would quote back; the linter needs only emptiness | a consumer needs line numbers or the raw `#`-prefixed text |
| scope | recipes only | `--list` renders submodules from `doc` alone | a module's truncated `doc` shows up as a real complaint |

## More Information

- `src/recipe.rs` — `Recipe::doc_prelude`, the authoritative
  description of the field and its delimiters.
- `src/parser.rs` — `Parser::take_doc_comment` /
  `take_doc_prelude`, the capture; parser-level cases in
  `doc_prelude_test!`.
- `tests/json.rs` — `doc_prelude`,
  `doc_prelude_bare_hash_separator`, and
  `doc_prelude_key_omitted_when_empty` cover the dump contract.
- `nix/linters/justfile-orphan-summary.nix` — the consumer, exported
  from `flake.nix` as
  `lib.conformistLinters.justfile-orphan-summary`.
- `conformist-justfile(7)`, RECIPE DESCRIPTIONS — the normative home
  for the convention the linter enforces, including the bare-`#`
  separator and a worked example.
- [FDR 0001](0001-events-fd.md) — the fork's other fork-only feature;
  same upstream posture (casey/just is not accepting pull requests).
