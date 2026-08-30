# The single `justPackage` option shared by every `justfile-*` linter — the
# seven model checks (recipe-names, task-hierarchy, recipe-descriptions,
# debug-recipes, leaf-noun, aggregate-comments, default) and
# justfile-orphan-summary. All of them read
# `config.linters.justfile-common.justPackage`, so an adopter wires ONE setting
# for the whole family instead of one per linter.
#
# This is a real MODULE (it declares an option); the sibling `justfile-model.nix`
# is a plain function providing `mkModelCheck`. Every justfile-* linter module
# does `imports = [ ../justfile-common.nix ]`, so the option is declared whether
# a repo enables the whole roster (`lib.conformistPresets.justfile`) or a single
# linter (`lib.conformistLinters.justfile-<name>`). The module system dedupes
# imports by path, so importing it from eight places is one declaration.
#
# MANDATORY (no default) on purpose:
#   - justfile-orphan-summary reads the fork-only `doc_prelude` field, which a
#     stock `just` SILENTLY omits — so a `pkgs.just` default would make it pass
#     VACUOUSLY, the exact failure its own mandatory option was added to prevent.
#     The seven model checks instead fail LOUDLY on a stock `just` (it rejects
#     `--dump-format model`), but they share this one option with orphan-summary,
#     so the option's safety is set by the stricter consumer.
#   - This module path is system-independent (exported from just-us's flake as
#     `lib.conformistLinters.*` / `lib.conformistPresets.justfile`) and cannot
#     close over a per-system just-us derivation, so it cannot default to a
#     just-us build either.
#
# Setting neither this option nor a per-linter override yields ONE clear eval
# error naming this option, not one per linter. Fleet-safe: existing wiring that
# sets `linters.justfile-orphan-summary.justPackage` explicitly never forces
# this option's (absent) default.
{ lib, ... }:
{
  options.linters.justfile-common.justPackage = lib.mkOption {
    type = lib.types.package;
    description = ''
      The `just` package every `justfile-*` linter invokes, shared across the
      family (the seven model checks and justfile-orphan-summary). MUST be a
      just-us build (code.linenisgreat.com/just-us): the model checks read
      `just --dump --dump-format model` (a fork-only dump format a stock `just`
      rejects), and justfile-orphan-summary reads the fork-only `doc_prelude`
      field (which a stock `just` silently omits — a vacuous pass). No default:
      this module path is system-independent and cannot close over a per-system
      just-us derivation, and a `pkgs.just` default would make
      justfile-orphan-summary pass vacuously. Set it once to
      `just-us.packages.''${system}.default`.
    '';
  };
}
