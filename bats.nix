# bats integration test lanes for the just-us `--events-fd` feature.
#
# Wraps `batsLane` (provided by `amarbel-llc/bats`'s
# `lib.${system}.batsLane` — see amarbel-llc/nixpkgs#16 for why it
# moved out of the nixpkgs overlay into bats) with project-specific
# defaults: bats-libs on BATS_LIB_PATH, the just binary exported as
# JUST_BIN, and a 10-second per-test timeout.
#
# Auto-discovers `# bats file_tags=...` directives in zz-tests_bats/
# at flake-eval time and produces one `bats-${tag}` derivation per
# unique tag plus `bats-default` (no filter).
{
  pkgs,
  batsLane,
  bats-libs,
  myBin, # the just binary derivation
  batsSrc,
  batsTestTimeout ? "10",
}:
let
  inherit (pkgs) lib;

  mkBatsLane =
    {
      filter ? "",
      base ? myBin,
    }:
    batsLane {
      inherit base filter batsSrc;
      binaries = {
        JUST_BIN = {
          inherit base;
          name = "just";
        };
      };
      batsLibPath = [ bats-libs.batsLibPath ];
      extraEnv = {
        BATS_TEST_TIMEOUT = batsTestTimeout;
      };
      # bats-island's setup_test_home calls `git config --global` to
      # populate GIT_CONFIG_GLOBAL; the sandbox needs git on PATH.
      nativeBuildInputs = [ pkgs.git ];
    };

  batsFiles = lib.filter (f: lib.hasSuffix ".bats" f) (builtins.attrNames (builtins.readDir batsSrc));

  # An optional group `(...)?` is used instead of an empty alternation
  # branch `(...|)` because darwin's libc++ regex engine (backing
  # builtins.match) rejects empty branches as invalid POSIX ERE. The
  # group is null when the input is all whitespace.
  trimWhitespace =
    s:
    let
      m = builtins.match "[[:space:]]*(.*[^[:space:]])?[[:space:]]*" s;
    in
    if m == null || builtins.head m == null then "" else builtins.head m;

  extractFileTags =
    file:
    let
      content = builtins.readFile (batsSrc + "/${file}");
      lines = lib.splitString "\n" content;
      tagLines = lib.filter (l: lib.hasPrefix "# bats file_tags=" l) lines;
    in
    if tagLines == [ ] then
      [ ]
    else
      map trimWhitespace (
        lib.splitString "," (lib.removePrefix "# bats file_tags=" (builtins.head tagLines))
      );

  allFileTags = lib.unique (lib.concatMap extractFileTags batsFiles);

  batsLaneOutputs =
    lib.listToAttrs (
      map (
        tag:
        lib.nameValuePair "bats-${tag}" (mkBatsLane {
          filter = tag;
        })
      ) allFileTags
    )
    // {
      bats-default = mkBatsLane { };
    };
in
{
  inherit mkBatsLane batsLaneOutputs;
}
