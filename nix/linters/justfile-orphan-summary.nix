# conformist-justfile(7) RECIPE DESCRIPTIONS, second half: a recipe's doc
# comment is the SINGLE comment line immediately above it, and that is the only
# line `just --list` prints. When a recipe is introduced by a block of
# contiguous comment lines, every line but the last is silently dropped and the
# `--list` summary column shows a truncated fragment of a sentence:
#
#   # Build the release binary and
#   # strip it
#   build:
#
# lists as `strip it`. This check fails any recipe carrying such an orphaned
# prelude. Whole-tree check (passes-files=false): reads recipe metadata from
# `just --dump --dump-format json`, takes no file arguments.
# Prose origin: eng-design_patterns-justfile(7) RECIPE DESCRIPTIONS.
#
# The signal is the `doc_prelude` field, which is a just-us FORK feature (see
# `src/recipe.rs`): the run of content-bearing comment lines immediately above
# the doc-comment line, terminating at a bare `#` line, a blank line, a
# non-comment item, or the start of file. Non-empty ⇒ the `--list` description
# is a truncated prose fragment. Upstream `just` never emits the key, so the
# check MUST run the fork's binary — hence the required `justPackage` option
# (this module is a system-independent path exported from just-us's flake, so it
# cannot close over a system-specific derivation; the consumer supplies one).
# A stock `just` would read as zero findings, which is why the option is
# mandatory rather than defaulting to `pkgs.just`.
#
# Scope: ALL recipes, leaf and aggregate. Private recipes (underscore-prefixed
# or [private]) don't appear in `just --list` and are exempt, consistent with
# justfile-recipe-descriptions. Unlike that linter, debug/explore recipes are
# NOT exempt: it defers those to justfile-debug-recipes (#23), which enforces
# its own doc-comment rule on them, but NO linter covers the orphaned-prelude
# rule for debug/explore recipes — exempting them here would leave a hole where
# a throwaway recipe's `--list` line is a sentence fragment and nothing
# complains.
#
# No `repair-command` on purpose. There is no honest autofix: mechanically
# inserting a bare `#` above the last comment line would satisfy the check while
# leaving `--list` showing the exact same useless fragment as the description.
# The fix is editorial — write a real one-line summary — so this linter reports
# and a human rewrites.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.linters.justfile-orphan-summary;

  check = pkgs.writeShellApplication {
    name = "conformist-justfile-orphan-summary";
    runtimeInputs = [
      pkgs.coreutils
      pkgs.jq
      cfg.justPackage
    ];
    text = ''
      [ -f justfile ] || {
        echo "justfile-orphan-summary: justfile missing at tree root" >&2
        exit 1
      }

      # Recipes, excluding private ones, whose doc comment is preceded by an
      # orphaned prose block. `doc_prelude` is omitted from the JSON when empty
      # (serde skip_serializing_if), so the key is ABSENT on the common clean
      # recipe — `// []` normalizes that to the empty list rather than letting
      # `null` reach `length`.
      filter='.recipes | to_entries[]
        | select(.value.private | not)
        | select(((.value.doc_prelude // []) | length) > 0)
        | .key'

      # Capture (not `< <(...)`) so a just/jq failure aborts loudly instead of
      # reading as "no findings" — a check must never pass vacuously on a parse
      # error.
      if ! offenders=$(just --dump --dump-format json | jq -r "$filter"); then
        echo "justfile-orphan-summary: failed to read recipes via just/jq" >&2
        exit 2
      fi

      fail=0
      while read -r name; do
        [ -n "$name" ] || continue
        echo "justfile-orphan-summary: recipe '$name' has comment lines above its doc comment; \`just --list\` shows ONLY the last comment line, so the rest is invisible and the description reads as a truncated fragment — separate the prose from the one-line summary with a bare \`#\` line (or a blank line), and make that summary a whole sentence fragment that stands alone (conformist-justfile(7) RECIPE DESCRIPTIONS)" >&2
        fail=1
      done <<< "$offenders"

      if [ "$fail" -ne 0 ]; then
        exit 1
      fi
      echo "justfile-orphan-summary: no recipe hides prose above its --list description"
    '';
  };
in
{
  options.linters.justfile-orphan-summary = {
    enable = lib.mkEnableOption "the no-orphaned-prose-above-a-recipe-description whole-tree check (eng-design_patterns-justfile(7), amarbel-llc/just-us)";

    justPackage = lib.mkOption {
      type = lib.types.package;
      description = ''
        The `just` package whose binary the check invokes. MUST be the just-us
        fork (amarbel-llc/just-us): the rule reads the fork-only `doc_prelude`
        field of `just --dump --dump-format json`, and an upstream `just` never
        emits it, so the check would pass vacuously. Deliberately has no default
        — this module is exported from just-us's flake as a system-independent
        path (`lib.conformistLinters.justfile-orphan-summary`) and so cannot
        close over a system-specific derivation; the consumer sets it to
        `just-us.packages.''${system}.default`.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    settings.linter.justfile-orphan-summary = {
      command = lib.getExe check;
      includes = [ "justfile" ];
      passes-files = false;
    };
  };
}
