# just-us.recipe-model v1 schema pin. Published standalone as a conformist
# "named prelude" release asset (alongside the static `just` binary) so a
# conformist profile can validate `just --dump --dump-format model`'s shape
# without a copy of this logic drifting between the two repos — see
# nix/justfile-model.nix, which reads this SAME file into its own prelude, so
# there is exactly one place this text is written.
#
# The FDR's versioning rule: additive fields do NOT bump `version`, so a
# consumer pins the integer and tolerates growth; a bump means a breaking
# change and MUST stop the consumer rather than silently produce an empty
# finding stream that reads as a clean tree.
def model:
  if .schema != "just-us.recipe-model" then
    error("unexpected schema '\(.schema // "<absent>")'; expected 'just-us.recipe-model'")
  elif .version != 1 then
    error("unsupported recipe-model version '\(.version // "<absent>")'; this check pins version 1")
  else . end;
