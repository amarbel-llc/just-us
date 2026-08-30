# lib.conformistPresets.justfile — the complete just-us justfile-linter family,
# enabled in one import. An adopter writes:
#
#   imports = [ just-us.lib.conformistPresets.justfile ];
#   linters.justfile-common.justPackage = just-us.packages.${system}.default;
#
# and gets all eight justfile-* checks wired against the fork's recipe model,
# with ONE setting for the whole family. Each `enable` is `lib.mkDefault true`,
# so a repo opts a single rule out with a plain assignment (no mkForce needed):
#
#   linters.justfile-recipe-names.enable = false;  # e.g. upstream-heritage recipes
#
# The eight linter modules each `imports = [ ../justfile-common.nix ]`; the
# module system dedupes imports by path, so importing them together here (and
# transitively the shared-option module, once) is well-formed, and each module
# also works standalone via `lib.conformistLinters.justfile-<name>`.
{ lib, ... }:
{
  imports = [
    ../linters/justfile-recipe-names.nix
    ../linters/justfile-task-hierarchy.nix
    ../linters/justfile-recipe-descriptions.nix
    ../linters/justfile-debug-recipes.nix
    ../linters/justfile-leaf-noun.nix
    ../linters/justfile-aggregate-comments.nix
    ../linters/justfile-default.nix
    ../linters/justfile-orphan-summary.nix
  ];

  config.linters = {
    justfile-recipe-names.enable = lib.mkDefault true;
    justfile-task-hierarchy.enable = lib.mkDefault true;
    justfile-recipe-descriptions.enable = lib.mkDefault true;
    justfile-debug-recipes.enable = lib.mkDefault true;
    justfile-leaf-noun.enable = lib.mkDefault true;
    justfile-aggregate-comments.enable = lib.mkDefault true;
    justfile-default.enable = lib.mkDefault true;
    justfile-orphan-summary.enable = lib.mkDefault true;
  };
}
