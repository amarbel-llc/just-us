# Behavioral fixture tests for the eight justfile-* linters in nix/linters/.
#
# Adapted from conformist's nix/linter-fixtures.nix (sunny-buckeye@6f47df5),
# reduced to the justfile-* fixtures, which moved into this fork with the
# linters they exercise. Each case runs a linter's compiled check against a
# crafted fixture tree and asserts the exit code (and, for failure cases, a
# token from the rule's own message — so a check that exits 2 on an operational
# error, e.g. a stock `just` rejecting `--dump-format model`, cannot be mistaken
# for a real finding).
#
# TWO ADAPTATIONS from conformist's version:
#   1. The `lib.evalModule` call IMPORTS the just-us linter module by path
#      (`imports = [ ./linters/${name}.nix ]`). conformist's evalModule
#      auto-enumerates ITS OWN nix/linters via readDir; it does not know about
#      this fork's modules, so each fixture brings the module under test itself.
#   2. `needsJustUs` sets the SHARED `linters.justfile-common.justPackage`
#      option (which all eight read), not a per-linter `justPackage` — the
#      per-linter option is gone from the seven model checks.
#
# ┌─ WIRING IS BLOCKED until conformist's half lands ─────────────────────────┐
# │ This file is NOT yet imported by flake.nix. conformist's flake INPUT still │
# │ ships its own copies of the seven justfile-* linters (its nix/linters.nix  │
# │ readDir-enumerates them), so `lib.evalModule` from the input already has,  │
# │ say, justfile-default. Importing THIS fork's justfile-default.nix into the │
# │ same eval and enabling it produces a duplicate                            │
# │ `settings.linter.justfile-default.command` — an eval conflict. It resolves │
# │ only once conformist's half (which deletes those seven and FODs this repo) │
# │ lands and this repo bumps its conformist input. At that point wire         │
# │ `justfile-linter-fixtures` into flake.nix's checks + a `just` recipe.      │
# │ orphan-summary is unaffected (it was always this fork's own, never in      │
# │ conformist), so its fixtures here would work today — but they ride the     │
# │ same aggregate, so the whole file waits for the seven.                     │
# └───────────────────────────────────────────────────────────────────────────┘
#
# Usage (once unblocked): import ./nix/justfile-linter-fixtures.nix
#   { inherit pkgs; lib = conformistLib; justPackage = just; }
# Returns the individual `linter-fixture-<name>-<label>` checks plus an
# aggregate `justfile-linter-fixtures` (a link farm forcing them all to build).
{
  pkgs,
  lib, # conformist library (evalModule); nixpkgs lib is pkgs.lib below
  # The just-us build the justfile-* fixtures run against. These linters read
  # `just --dump --dump-format model` / the `doc_prelude` field, fork-only
  # surfaces a stock `just` rejects or omits, so the fixtures are only
  # meaningful against a just-us build. The `pkgs.just` default keeps this file
  # evaluating standalone; with it every fixture fails loudly rather than
  # passing vacuously. flake.nix supplies the real build.
  justPackage ? pkgs.just,
}:
let
  nixlib = pkgs.lib;

  # Which linters need the just-us binary rather than the stock default. Keyed
  # on the module's name prefix rather than an explicit per-fixture assignment,
  # so a justfile-* fixture added later cannot forget to wire it and silently
  # exercise the wrong `just`. A fixture may still override via `enableModule`.
  needsJustUs = name: nixlib.hasPrefix "justfile-" name;

  # Materialize an attrset of project-relative-path -> content into the cwd,
  # via the store so no shell heredoc escaping is needed. Read-only (444) is
  # fine — the linters only read fixtures.
  writeFixtureFiles =
    files:
    nixlib.concatStringsSep "\n" (
      nixlib.mapAttrsToList (
        path: content:
        let
          f = pkgs.writeText "fixture-file" content;
        in
        ''
          mkdir -p "$(dirname ${nixlib.escapeShellArg path})"
          cp ${f} ${nixlib.escapeShellArg path}
        ''
      ) files
    );

  # name        : linter name (key under linters.<name> and settings.linter.<name>)
  # label       : fixture label (becomes the derivation suffix)
  # enableModule: extra options merged into `linters.<name>` (e.g. { key = ...; })
  # files       : attrset of relpath -> content written into the fixture tree
  # expectFail  : true => the linter MUST exit non-zero; false => MUST exit zero
  # expectToken : optional substring the linter output MUST contain
  # evalPkgs    : pkgs the linter MODULE is evaluated against (default: pkgs).
  mkLinterFixtureCheck =
    {
      name,
      label,
      enableModule ? { },
      files,
      expectFail ? false,
      expectToken ? null,
      evalPkgs ? pkgs,
    }:
    let
      mod = lib.evalModule evalPkgs {
        enableDefaultExcludes = false;
        # Adaptation 1: bring the fork's own module under test; conformist's
        # evalModule does not know about it.
        imports = [ ./linters/${name}.nix ];
        # Adaptation 2: the seven model checks read the shared
        # `linters.justfile-common.justPackage`, so inject THAT (not a per-linter
        # option). `//` is shallow, but ${name} is never "justfile-common", so
        # the two keys coexist.
        linters = {
          ${name} = {
            enable = true;
          }
          // enableModule;
        }
        // nixlib.optionalAttrs (needsJustUs name) {
          justfile-common.justPackage = justPackage;
        };
      };
      cmd = mod.config.settings.linter.${name}.command;

      assertExit =
        if expectFail then
          ''
            if [ "$rc" -eq 0 ]; then
              echo "FIXTURE FAIL: expected linter '${name}' to reject ${label}, but it exited 0" >&2
              exit 1
            fi
          ''
        else
          ''
            if [ "$rc" -ne 0 ]; then
              echo "FIXTURE FAIL: expected linter '${name}' to pass ${label}, but it exited $rc" >&2
              exit 1
            fi
          '';

      assertToken = nixlib.optionalString (expectToken != null) ''
        if ! grep -qF ${nixlib.escapeShellArg expectToken} out.log; then
          echo "FIXTURE FAIL: linter '${name}' output did not contain ${nixlib.escapeShellArg expectToken}" >&2
          exit 1
        fi
      '';
    in
    pkgs.runCommandLocal "linter-fixture-${name}-${label}" { } ''
      mkdir fixture && cd fixture
      ${writeFixtureFiles files}

      # The whole-tree check runs at the tree root (cwd) with no file arguments.
      if ${cmd} >out.log 2>&1; then rc=0; else rc=$?; fi
      cat out.log

      ${assertExit}
      ${assertToken}

      touch $out
    '';

  fixtures = [
    # justfile-debug-recipes: debug/explore recipes must carry a doc comment
    # (eng-design_patterns-justfile(7) RECIPE DESCRIPTIONS, conformist#23).
    (mkLinterFixtureCheck {
      name = "justfile-debug-recipes";
      label = "documented-pass";
      files = {
        "justfile" = ''
          # probe the widget cache for the cache-eviction dev-loop (see #1)
          [group('debug')]
          debug-widget-cache:
              echo hi
        '';
      };
    })
    (mkLinterFixtureCheck {
      name = "justfile-debug-recipes";
      label = "undocumented-fail";
      files = {
        "justfile" = ''
          [group('debug')]
          debug-widget-cache:
              echo hi
        '';
      };
      expectFail = true;
      expectToken = "no doc comment";
    })
    # conformist#96: a bodyless (aggregate) debug recipe is exempt — it must
    # PASS even with no doc comment, since justfile-aggregate-comments forbids
    # an aggregate from carrying one at all (the two linters would otherwise be
    # unsatisfiable together).
    (mkLinterFixtureCheck {
      name = "justfile-debug-recipes";
      label = "aggregate-uncommented-pass";
      files = {
        "justfile" = ''
          # probe the widget cache for the cache-eviction dev-loop (see #1)
          [group('debug')]
          debug-widget-cache:
              echo hi

          [group('debug')]
          debug-widget: debug-widget-cache
        '';
      };
    })
    # A debug LEAF without a comment must still FAIL even when a sibling debug
    # aggregate is present (guards against the aggregate exemption over-matching).
    (mkLinterFixtureCheck {
      name = "justfile-debug-recipes";
      label = "leaf-still-fails-beside-aggregate";
      files = {
        "justfile" = ''
          [group('debug')]
          debug-widget-cache:
              echo hi

          [group('debug')]
          debug-widget: debug-widget-cache
        '';
      };
      expectFail = true;
      expectToken = "no doc comment";
    })

    # justfile-recipe-descriptions: every leaf recipe carries a doc comment
    # (eng-design_patterns-justfile(7) RECIPE DESCRIPTIONS, conformist#17).
    (mkLinterFixtureCheck {
      name = "justfile-recipe-descriptions";
      label = "documented-pass";
      files = {
        "justfile" = ''
          # builds the thing
          build-thing:
              echo hi
        '';
      };
    })
    (mkLinterFixtureCheck {
      name = "justfile-recipe-descriptions";
      label = "undocumented-leaf-fail";
      files = {
        "justfile" = ''
          build-thing:
              echo hi
        '';
      };
      expectFail = true;
      expectToken = "no doc comment";
    })
    (mkLinterFixtureCheck {
      name = "justfile-recipe-descriptions";
      label = "exempts-aggregate-and-debug-pass";
      files = {
        # Aggregate (no body) is self-documenting; debug recipe is #23's job:
        # both are exempt even when undocumented, so this passes.
        "justfile" = ''
          # documented leaf
          build-thing:
              echo hi

          agg: build-thing

          [group('debug')]
          debug-thing:
              echo hi
        '';
      };
    })

    # justfile-task-hierarchy: no leaf belongs to more than one aggregate
    # (eng-design_patterns-justfile(7) TASK HIERARCHY, conformist#17).
    (mkLinterFixtureCheck {
      name = "justfile-task-hierarchy";
      label = "single-aggregate-pass";
      files = {
        "justfile" = ''
          build: build-go build-nix

          build-go:
              echo go

          build-nix:
              echo nix
        '';
      };
    })
    (mkLinterFixtureCheck {
      name = "justfile-task-hierarchy";
      label = "orphan-leaf-pass";
      files = {
        # A leaf in no aggregate is a legitimate standalone recipe (release, tag,
        # run-nix): the upper-bound rule must NOT flag it.
        "justfile" = ''
          release-thing:
              echo release
        '';
      };
    })
    (mkLinterFixtureCheck {
      name = "justfile-task-hierarchy";
      label = "multi-aggregate-fail";
      files = {
        "justfile" = ''
          build: shared
          verify: shared

          shared:
              echo hi
        '';
      };
      expectFail = true;
      expectToken = "at most one aggregate";
    })
    (mkLinterFixtureCheck {
      name = "justfile-task-hierarchy";
      label = "pipeline-orphan-fail";
      files = {
        # A pipeline-verb leaf (build) in no aggregate is unreachable from default
        # — the tightened lower bound must flag it.
        "justfile" = ''
          build-go:
              echo go
        '';
      };
      expectFail = true;
      expectToken = "exactly one aggregate";
    })

    # justfile-recipe-names: recipes in a `mod`-imported child justfile are
    # listed module-qualified (explore::debug-widget); the qualifier is
    # stripped before verb extraction, and the rule still applies to the
    # recipe's own name (conformist#85).
    (mkLinterFixtureCheck {
      name = "justfile-recipe-names";
      label = "module-qualified-pass";
      files = {
        "justfile" = ''
          run-thing:
              echo hi

          mod explore 'zz-explore/justfile'
        '';
        "zz-explore/justfile" = ''
          # pokes the widget for the dev loop
          debug-widget:
              echo widget
        '';
      };
    })
    # justfile-recipe-names: the operational verbs migrate/provision/restart
    # (conformist-justfile(7) VERB LIST, eng#270) are canonical and pass...
    (mkLinterFixtureCheck {
      name = "justfile-recipe-names";
      label = "operational-verbs-pass";
      files = {
        "justfile" = ''
          migrate-foo:
              echo migrate

          provision-bar:
              echo provision

          restart-baz:
              echo restart
        '';
      };
    })
    # ...while a verb that still isn't in the canonical list is rejected.
    (mkLinterFixtureCheck {
      name = "justfile-recipe-names";
      label = "still-unknown-verb-fail";
      files = {
        "justfile" = ''
          frobnicate-widget:
              echo nope
        '';
      };
      expectFail = true;
      expectToken = "does not start with a known verb";
    })
    (mkLinterFixtureCheck {
      name = "justfile-recipe-names";
      label = "module-bad-verb-fail";
      files = {
        "justfile" = ''
          run-thing:
              echo hi

          mod explore 'zz-explore/justfile'
        '';
        "zz-explore/justfile" = ''
          # not a known verb even after the qualifier is stripped
          frobnicate-widget:
              echo nope
        '';
      };
      expectFail = true;
      expectToken = "does not start with a known verb";
    })

    # justfile-task-hierarchy over `mod`-imported child justfiles
    # (conformist#85): module recipes were previously invisible (the dump
    # nests them under .modules); they are now checked, a root aggregate
    # owning a module leaf is credited (the dump drops the mod:: qualifier
    # from dependencies, so ownership matches on bare names), and a module
    # pipeline-verb orphan is flagged.
    (mkLinterFixtureCheck {
      name = "justfile-task-hierarchy";
      label = "module-debug-orphan-pass";
      files = {
        "justfile" = ''
          build: build-go

          build-go:
              echo go

          mod explore 'zz-explore/justfile'
        '';
        "zz-explore/justfile" = ''
          # a debug leaf may be an orphan
          debug-widget:
              echo widget
        '';
      };
    })
    (mkLinterFixtureCheck {
      name = "justfile-task-hierarchy";
      label = "module-cross-owned-pass";
      files = {
        "justfile" = ''
          build: build-go explore::build-thing

          build-go:
              echo go

          mod explore 'zz-explore/justfile'
        '';
        "zz-explore/justfile" = ''
          # owned by the root build aggregate via explore::build-thing
          build-thing:
              echo thing
        '';
      };
    })
    (mkLinterFixtureCheck {
      name = "justfile-task-hierarchy";
      label = "module-pipeline-orphan-fail";
      files = {
        "justfile" = ''
          build: build-go

          build-go:
              echo go

          mod explore 'zz-explore/justfile'
        '';
        "zz-explore/justfile" = ''
          # a pipeline-verb leaf hiding in a module must still be flagged
          test-orphan:
              echo orphan
        '';
      };
      expectFail = true;
      expectToken = "exactly one aggregate";
    })

    # justfile-leaf-noun: a leaf must be verb-noun, not a bare verb
    # (conformist-justfile(7) AGGREGATES AND LEAVES, conformist#17).
    (mkLinterFixtureCheck {
      name = "justfile-leaf-noun";
      label = "verb-noun-pass";
      files = {
        "justfile" = ''
          build-go:
              echo hi
        '';
      };
    })
    (mkLinterFixtureCheck {
      name = "justfile-leaf-noun";
      label = "bare-verb-fail";
      files = {
        "justfile" = ''
          build:
              echo hi
        '';
      };
      expectFail = true;
      expectToken = "bare verb";
    })
    (mkLinterFixtureCheck {
      name = "justfile-leaf-noun";
      label = "tag-exempt-pass";
      files = {
        # `tag` is a verb-noun-exempt release recipe even as a single-segment leaf.
        "justfile" = ''
          tag:
              echo tag
        '';
      };
    })

    # justfile-aggregate-comments: an aggregate must not carry a doc comment
    # (conformist-justfile(7) AGGREGATES AND LEAVES, conformist#17).
    (mkLinterFixtureCheck {
      name = "justfile-aggregate-comments";
      label = "uncommented-pass";
      files = {
        "justfile" = ''
          build: build-go

          # compiles go
          build-go:
              echo hi
        '';
      };
    })
    (mkLinterFixtureCheck {
      name = "justfile-aggregate-comments";
      label = "commented-fail";
      files = {
        "justfile" = ''
          # builds everything
          build: build-go

          # compiles go
          build-go:
              echo hi
        '';
      };
      expectFail = true;
      expectToken = "doc comment";
    })

    # justfile-default: `default` is first and lists only aggregates
    # (conformist-justfile(7) DEFAULT RECIPE / AGGREGATES AND LEAVES). The
    # aggregate-pass case is the conformist#51 regression: a BACKSLASH-CONTINUED
    # aggregate must not be misread as a leaf-with-a-body (the old awk indent
    # heuristic flagged it; the `just --dump` rewrite parses it correctly).
    (mkLinterFixtureCheck {
      name = "justfile-default";
      label = "backslash-aggregate-pass";
      files = {
        "justfile" = ''
          default: test

          test: \
              test-go \
              test-bats

          test-go:
              echo go

          test-bats:
              echo bats
        '';
      };
    })
    (mkLinterFixtureCheck {
      name = "justfile-default";
      label = "lists-leaf-fail";
      files = {
        # `default` depends on a leaf that has a body — not an aggregate.
        "justfile" = ''
          default: run-thing

          run-thing:
              echo x
        '';
      };
      expectFail = true;
      expectToken = "lists leaf recipe";
    })
    (mkLinterFixtureCheck {
      name = "justfile-default";
      label = "first-not-default-fail";
      files = {
        "justfile" = ''
          build-go:
              echo go

          default: build-go
        '';
      };
      expectFail = true;
      expectToken = "first recipe must be 'default'";
    })

    # ---- conformist#89: the other five linters must SEE module recipes ------
    #
    # Before the `--dump-format model` rewrite, justfile-recipe-descriptions,
    # justfile-debug-recipes, justfile-leaf-noun, justfile-aggregate-comments and
    # justfile-default read only the ROOT `.recipes` of `--dump-format json` and
    # never recursed into `.modules`, so every `mod`-imported recipe was silently
    # unlinted. The model's `recipes` is one FLAT list spanning the root and all
    # modules, so each of these now fires.
    #
    # Each case below is a FAILURE case asserting the RULE's own message. That
    # matters more than usual here: `expectFail` only demands a non-zero exit, and
    # these checks exit 2 on an operational error (a stock `just` rejecting the
    # model format, a malformed fixture justfile) — so a fixture asserting only
    # non-zero would report success for a check that never evaluated the rule at
    # all. The token pins that a real finding was produced.
    #
    # The submodule is named `sub`, deliberately not `explore`: what exempts a
    # recipe from justfile-recipe-descriptions is its `[group('explore')]`
    # membership, not the name of the module it lives in, and a module named
    # `explore` would blur the two.
    (mkLinterFixtureCheck {
      name = "justfile-recipe-descriptions";
      label = "module-undocumented-fail";
      files = {
        "justfile" = ''
          # builds the thing
          build-thing:
              echo hi

          mod sub 'zz-sub/justfile'
        '';
        "zz-sub/justfile" = ''
          run-widget:
              echo widget
        '';
      };
      expectFail = true;
      expectToken = "no doc comment";
    })

    (mkLinterFixtureCheck {
      name = "justfile-debug-recipes";
      label = "module-undocumented-fail";
      files = {
        # The canonical eng shape for throwaway recipes is a `mod explore
        # 'zz-explore/justfile'` block — precisely the place this check could not
        # see before, so an undocumented debug recipe was most invisible exactly
        # where debug recipes actually live.
        "justfile" = ''
          # builds the thing
          build-thing:
              echo hi

          mod sub 'zz-sub/justfile'
        '';
        "zz-sub/justfile" = ''
          [group('debug')]
          debug-widget:
              echo widget
        '';
      };
      expectFail = true;
      expectToken = "no doc comment";
    })

    (mkLinterFixtureCheck {
      name = "justfile-leaf-noun";
      label = "module-bare-verb-fail";
      files = {
        "justfile" = ''
          # builds the thing
          build-thing:
              echo hi

          mod sub 'zz-sub/justfile'
        '';
        # A bare-verb leaf inside a module. The noun test runs on the model's
        # BARE `name` (`build`), never the qualified `namepath`
        # (`sub::build`) — testing the namepath would find the `::` and let this
        # through as though it had a noun (conformist#85).
        "zz-sub/justfile" = ''
          # builds it
          build:
              echo hi
        '';
      };
      expectFail = true;
      expectToken = "bare verb";
    })

    (mkLinterFixtureCheck {
      name = "justfile-aggregate-comments";
      label = "module-commented-fail";
      files = {
        "justfile" = ''
          # builds the thing
          build-thing:
              echo hi

          mod sub 'zz-sub/justfile'
        '';
        "zz-sub/justfile" = ''
          # a commented aggregate inside a module
          build-all: build-inner

          # builds the inner thing
          build-inner:
              echo inner
        '';
      };
      expectFail = true;
      expectToken = "doc comment";
    })

    # justfile-default over a module dependency. `default` naming a module
    # AGGREGATE is legitimate and must pass...
    (mkLinterFixtureCheck {
      name = "justfile-default";
      label = "module-aggregate-dep-pass";
      files = {
        "justfile" = ''
          default: sub::build-thing

          mod sub 'zz-sub/justfile'
        '';
        "zz-sub/justfile" = ''
          build-thing: build-inner

          # builds the inner thing
          build-inner:
              echo inner
        '';
      };
    })

    # ...while `default` naming a module LEAF must fail. This is the sharpest
    # conformist#89 discriminator for this linter: the old filter resolved a
    # dependency by looking the raw dump's BARE dependency name up in the ROOT
    # recipe table, so a `mod::`-qualified dependency resolved to null, `null.body`
    # measured 0, and the leaf was reported as an aggregate — a silent PASS on a
    # tree that violates the rule. Resolution is now by the model's
    # `dependencies[].namepath` against the flat list.
    (mkLinterFixtureCheck {
      name = "justfile-default";
      label = "module-leaf-dep-fail";
      files = {
        "justfile" = ''
          default: sub::run-thing

          mod sub 'zz-sub/justfile'
        '';
        "zz-sub/justfile" = ''
          # runs the thing
          run-thing:
              echo x
        '';
      };
      expectFail = true;
      expectToken = "lists leaf recipe";
    })

    # justfile-task-hierarchy: the ambiguity-skip is gone. The raw dump dropped
    # the `mod::` qualifier from dependencies, so ownership had to be matched on
    # bare names, and any leaf whose bare name appeared in more than one scope was
    # SKIPPED as unattributable. Here `test-thing` exists in both modules and only
    # alpha's is owned by the root `test` aggregate — so beta's is an un-aggregated
    # pipeline-verb leaf and must be flagged. Under the old bare-name matching both
    # were skipped as duplicates and the tree passed vacuously.
    (mkLinterFixtureCheck {
      name = "justfile-task-hierarchy";
      label = "duplicate-bare-name-across-modules-fail";
      files = {
        "justfile" = ''
          test: alpha::test-thing

          mod alpha 'zz-alpha/justfile'
          mod beta 'zz-beta/justfile'
        '';
        "zz-alpha/justfile" = ''
          # tests the thing, owned by the root test aggregate
          test-thing:
              echo a
        '';
        "zz-beta/justfile" = ''
          # tests the thing, owned by nothing
          test-thing:
              echo b
        '';
      };
      expectFail = true;
      expectToken = "exactly one aggregate";
    })
  ];
in
builtins.listToAttrs (
  map (d: {
    inherit (d) name;
    value = d;
  }) fixtures
)
// {
  # Aggregate: a link farm that forces every justfile-* fixture to build. Once
  # unblocked (see the header), `just verify-justfile-linter-fixtures` builds
  # this one path instead of the full `nix flake check`.
  justfile-linter-fixtures = pkgs.linkFarmFromDrvs "justfile-linter-fixtures" fixtures;
}
