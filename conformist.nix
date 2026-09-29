# This repo's conformist overlay, imported into the existing, heavily
# customized `conformistEval` in flake.nix (NOT the generic
# `conformist.lib.presets.eng` + self-referential `just-us` input
# `conformist conform`'s brownfield scaffold proposed — just-us IS
# just-us, so importing itself as a flake input made no sense, and its
# `conformistEval` already carries this repo's rustfmt/nixfmt/
# shellcheck/agents-md/justfile-linter wiring by hand). This file only
# holds the bits that were genuinely new: nixfmt enablement moved here
# from the inline block, plus the tree-wide excludes below.
{ ... }:
{
  programs.nixfmt.enable = true;

  # Prose and generated files are out of scope for code formatters.
  settings.excludes = [
    "*.md"
    "flake.lock"
    "LICENSE"
  ];
}
