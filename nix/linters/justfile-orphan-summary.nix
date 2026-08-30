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
# prelude. Whole-tree check (passes-files=false): reads the native recipe model
# via `just --dump --dump-format model`, takes no file arguments.
# Prose origin: eng-design_patterns-justfile(7) RECIPE DESCRIPTIONS.
#
# The signal is the model's `doc_prelude` field, a just-us FORK feature (see
# `src/recipe.rs` and docs/features/0002-doc-prelude.md): the run of
# content-bearing comment lines immediately above the doc-comment line,
# terminating at a bare `#` line, a blank line, a non-comment item, or the start
# of file. Non-empty ⇒ the `--list` description is a truncated prose fragment.
# In the model `doc_prelude` is ALWAYS present (empty list when none) and is
# carried for module recipes too, so — unlike this check's old
# `--dump-format json` form, which read only the root `.recipes` and silently
# skipped `mod`-imported recipes — a module recipe's orphaned prelude is now
# flagged (the same conformist#89 gap the seven model checks close).
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
  model = import ../justfile-model.nix { inherit pkgs lib; };

  check = model.mkModelCheck {
    name = "justfile-orphan-summary";
    # cfg.justPackage, NOT the shared option directly: this module keeps its own
    # back-compat option (below) that DEFAULTS to the shared one, so old wiring
    # setting `linters.justfile-orphan-summary.justPackage` explicitly is still
    # honoured.
    justPackage = cfg.justPackage;
    okMessage = "no recipe hides prose above its --list description";
    # Non-private recipes across the root AND all modules whose doc comment is
    # preceded by an orphaned prose block. `doc_prelude` is always present in the
    # model (empty list when none), so no `// []` normalization is needed.
    filter = ''
      recipes
      | public
      | .[]
      | select((.doc_prelude | length) > 0)
      | "recipe '\(.namepath)' has comment lines above its doc comment; `just --list` shows ONLY the last comment line, so the rest is invisible and the description reads as a truncated fragment - separate the prose from the one-line summary with a bare `#` line (or a blank line), and make that summary a whole sentence fragment that stands alone (conformist-justfile(7) RECIPE DESCRIPTIONS)"
    '';
  };
in
{
  imports = [ ../justfile-common.nix ];

  options.linters.justfile-orphan-summary = {
    enable = lib.mkEnableOption "the no-orphaned-prose-above-a-recipe-description whole-tree check (eng-design_patterns-justfile(7), amarbel-llc/just-us)";

    justPackage = lib.mkOption {
      type = lib.types.package;
      default = config.linters.justfile-common.justPackage;
      defaultText = lib.literalExpression "config.linters.justfile-common.justPackage";
      description = ''
        The `just` package whose binary the check invokes. MUST be the just-us
        fork (amarbel-llc/just-us): the rule reads the fork-only `doc_prelude`
        field of `just --dump --dump-format model`, and an upstream `just` never
        emits it (and rejects the format outright), so the check would otherwise
        pass vacuously.

        DEPRECATED in favour of the shared `linters.justfile-common.justPackage`
        option, which every `justfile-*` linter reads. This per-linter option is
        retained only for back-compat: it is wired explicitly by conformist's eng
        template and by the `//go:embed`-ed scaffold flake, so removing it would
        be a fleet-wide eval break. It now DEFAULTS to the shared option, so new
        wiring should set `linters.justfile-common.justPackage` once for the
        whole family and leave this unset; existing wiring that sets it
        explicitly still wins. Slated for removal in a later, deliberate fleet
        sweep once scaffolded repos have cycled onto the shared option.
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
