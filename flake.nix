{
  description = "Just a command runner";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";

    # bats helper libraries + the `batsLane` builder. Used only for the
    # `bats-*` flake outputs; the upstream-facing `packages.default`
    # derivation stays on stock nixpkgs.
    #
    # Don't override bats.inputs.nixpkgs to follow our `nixpkgs`: the
    # bats flake expects an amarbel-llc/nixpkgs-shaped tree (it reads
    # `nixpkgs.overlays.default` internally). Stock NixOS/nixpkgs
    # doesn't expose that, so we let bats bring its own pin.
    bats.url = "github:amarbel-llc/bats";

    # conformist (formatter/linter multiplexer) supplies `nix fmt`, the
    # read-only `checks.formatting` gate, and the eng conformance
    # linters. Tool binaries resolve from our `nixpkgs`; only the
    # conformist binary itself comes from the input's own pin.
    conformist.url = "github:amarbel-llc/conformist";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      bats,
      ...
    }@inputs:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
        };

        package = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package;

        just = pkgs.rustPlatform.buildRustPackage {
          pname = "just";
          version = package.version;

          src = ./.;

          auditable = false;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          nativeBuildInputs = with pkgs; [
            installShellFiles
            pkg-config
          ];

          doCheck = false;

          postInstall = ''
            for shell in bash fish zsh; do
              $out/bin/just --completions $shell > just.$shell
              installShellCompletion just.$shell
            done

            $out/bin/just --man > just.1
            installManPage just.1
          '';

          meta = {
            description = package.description;
            homepage = package.homepage;
            changelog = "${package.repository}/blob/master/CHANGELOG.md";
            license = pkgs.lib.licenses.cc0;
            mainProgram = "just";
          };
        };

        # just-us-clown-plugin stages a clown plugin (clown-plugin-protocol(7) /
        # clown-json(5)) that contributes this repo's public justfile recipes
        # (name + doc line, no filtering) into the agent's dynamic system
        # prompt via `just --mcp` (docs/features/0005). Mirrors
        # cutting-garden's `cuttingGardenClownPlugin`: the source-controlled
        # `clown.json.in` / `plugin.json.in` carry `@JUST_US@` / `@version@`
        # placeholders, substituted here so the manifest can't drift from the
        # binary it points at. eng's `mkCircus`/`lib/circus.nix` mounts a
        # plugin by consuming this derivation's
        # share/purse-first/just-us/{.claude-plugin/plugin.json,clown.json}
        # (see docs/features/0004).
        justUsClownPlugin = pkgs.runCommand "just-us-clown-plugin" { } ''
          pluginRoot=$out/share/purse-first/just-us
          mkdir -p $pluginRoot/.claude-plugin
          substitute \
            ${./plugins/just-us/.claude-plugin/plugin.json.in} \
            $pluginRoot/.claude-plugin/plugin.json \
            --replace-fail '@version@' '${package.version}'
          substitute \
            ${./plugins/just-us/clown.json.in} \
            $pluginRoot/clown.json \
            --replace-fail '@JUST_US@' '${just}/bin/just'
        '';

        conformistEval = inputs.conformist.lib.evalModule pkgs {
          package = inputs.conformist.packages.${system}.default;

          # Formatters: rust + nix only — deliberately narrow so upstream
          # prose and config (README.md, Cargo.toml, ...) stay untouched
          # across resyncs. rustfmt's edition default (2024) matches
          # rustfmt.toml.
          programs.rustfmt.enable = true;
          programs.nixfmt.enable = true;

          # Native read-only check so `checks.formatting` doesn't use the
          # sandbox-copy strategy, which loses rustfmt.toml and checks
          # against rustfmt's default config (conformist#28). Drop when
          # that's fixed upstream.
          settings.formatter.rustfmt."check-command" = pkgs.lib.getExe pkgs.rustfmt;
          settings.formatter.rustfmt."check-options" = [
            "--check"
            "--config"
            "skip_children=true"
            "--edition"
            "2024"
          ];

          # Mirror the pre-conformist lint-shellcheck gate: only the
          # install script. The rest of the tree's shell (e.g. generated
          # completions) is upstream's and not held to shellcheck.
          linters.shellcheck.enable = true;
          linters.shellcheck.includes = pkgs.lib.mkForce [ "www/install.sh" ];

          # eng conformance linters. NOT enabled: eng-versioning (derives the
          # version key from go.mod; Rust repos unsupported yet).
          linters.agents-md.enable = true;

          # Dogfood the fork's full justfile-linter roster against this repo's
          # own justfile. `lib.conformistPresets.justfile` (the module this flake
          # exports, below) enables all eight — the seven model checks plus
          # justfile-orphan-summary — each with `lib.mkDefault true`, so a plain
          # `enable = false` opts one out. `justfile-common.justPackage` is the
          # just built right here, so the checks read the fork's `--dump-format
          # model` / `doc_prelude` rather than passing vacuously against a stock
          # `just`. This became possible once the conformist input was bumped past
          # its half of the transplant, which removed conformist's own copies of
          # the seven (they used to double-declare with these).
          imports = [ ./nix/presets/justfile.nix ];
          linters.justfile-common.justPackage = just;

          # just-us is an upstream FORK: its justfile carries upstream heritage
          # (the `demo` group — quine, polyglot, rule110, pwd — kept verbatim on
          # purpose) and a large surface of test/maintenance utility recipes
          # (test-filter, test-fuzz, build-man, view-man, ...). Six of the eight
          # rules don't fit that shape without an editorial sweep of the whole
          # justfile, so they are opted out here (plain `false` beats the roster's
          # `mkDefault true`); the two the fork passes cleanly —
          # justfile-orphan-summary and justfile-default — stay enforced. This
          # dogfoods the adopter path (the exported roster, imported and run
          # against a real justfile with the fork's own binary); the six rules'
          # behavioral correctness is proven separately by
          # nix/justfile-linter-fixtures.nix (wired into checks below) and by the
          # conformist consumer. Bringing the fork's own justfile to fuller
          # conformance — so more of these can be re-enabled — is tracked
          # editorial follow-up, not a blocker here.
          linters.justfile-recipe-names.enable = false; # heritage demo names are non-verb-noun
          linters.justfile-leaf-noun.enable = false; # heritage demos are bare verbs
          linters.justfile-task-hierarchy.enable = false; # many legitimately-orphan test/util leaves
          linters.justfile-recipe-descriptions.enable = false; # heritage + utility leaves undocumented
          linters.justfile-debug-recipes.enable = false; # some debug/explore recipes undocumented
          linters.justfile-aggregate-comments.enable = false; # default/lint/ci carry informative comments
        };

        batsLib = import ./bats.nix {
          inherit pkgs;
          myBin = just;
          batsLane = inputs.bats.lib.${system}.batsLane;
          bats-libs = inputs.bats.packages.${system}.bats-libs;
          batsSrc = pkgs.lib.cleanSourceWith {
            src = ./zz-tests_bats;
            filter =
              path: type:
              type == "directory"
              || pkgs.lib.hasSuffix ".bats" path
              || baseNameOf path == "common.bash"
              || baseNameOf path == "setup_suite.bash";
          };
        };

        # Behavioral fixtures for the eight justfile-* linters: each runs a
        # linter's compiled check against a crafted fixture tree and asserts the
        # exit code (and, on failure cases, a token from the rule's own message).
        # This is the in-tree proof that every rule executes and catches, which
        # the dogfood above deliberately does not exercise for the six opted-out
        # rules. `lib` is conformist's evalModule library; `justPackage` is the
        # fork built here. (This can only evaluate now that the conformist input
        # was bumped past its half of the transplant — before that, conformist
        # readDir-enumerated its own copies of the seven and importing this fork's
        # collided on `settings.linter.<name>.command`.)
        justfileLinterFixtures = import ./nix/justfile-linter-fixtures.nix {
          inherit pkgs;
          lib = inputs.conformist.lib;
          justPackage = just;
        };
      in
      {
        packages = batsLib.batsLaneOutputs // {
          default = just;

          # Clown plugin closure for eng's mkCircus (docs/features/0004).
          just-us-clown-plugin = justUsClownPlugin;
        };

        checks = {
          bats-default = batsLib.batsLaneOutputs.bats-default;
          formatting = conformistEval.config.build.check self;
          justfile-linter-fixtures = justfileLinterFixtures.justfile-linter-fixtures;
        };

        # `nix fmt` — rewrites the worktree with every formatter and
        # linter repair; `checks.formatting` is the read-only twin.
        formatter = conformistEval.config.build.wrapper;

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            rustc
            cargo
            clippy
            rustfmt
            # The integration suite (choose::default, edit::editor_precedence)
            # symlinks fake tool names onto `which cat` and runs them; that
            # breaks when cat is a single-binary coreutils multicall that
            # dispatches on argv[0]. Pin a separate-binaries coreutils first
            # in PATH so the symlink trick works.
            (coreutils.override { singleBinary = false; })
            # cargo l* (lclippy/lrun/ltest) used by the lint/run recipes.
            cargo-limit
            # bin/forbid greps with rg.
            ripgrep
            # upstream's tests/backticks.rs configures `set shell :=
            # ['python3', '-c']`, so the cargo test suite needs python3 on PATH;
            # without it backticks::trailing_newlines_are_stripped fails with
            # "could not find the shell". Not otherwise used by the fork.
            python3
          ];
        };
      }
    )
    // {
      # System-independent reusable outputs, outside eachDefaultSystem: a
      # conformist linter module PATH carries no per-system derivation, so it
      # must not be published per system.
      #
      # The eight `justfile-*` linters — the seven that read the recipe model
      # (`--dump-format model`) plus justfile-orphan-summary (`doc_prelude`) —
      # are the modules a downstream repo imports into its own
      # `conformist.lib.evalModule`. Wire the whole family in one import:
      #
      #   imports = [ just-us.lib.conformistPresets.justfile ];
      #   linters.justfile-common.justPackage = just-us.packages.${system}.default;
      #
      # or a single rule via `lib.conformistLinters.justfile-<name>` plus the same
      # `linters.justfile-common.justPackage` setting. That shared option is
      # MANDATORY and MUST be a just-us build: the seven model checks read a
      # fork-only dump format a stock `just` rejects, and orphan-summary reads the
      # fork-only `doc_prelude` field (which a stock `just` silently omits — a
      # vacuous pass). It has no default precisely because these paths are
      # system-independent and cannot close over this flake's per-system `just`.
      #
      # The coupling lives HERE, in the repo that owns the parser feature, and
      # not upstream in conformist: conformist must stay strictly upstream of its
      # consumers and must not take just-us as an input — it FODs just-us source
      # instead. Same arrangement as purse-first's `lib.conformistLinters.dewey-*`
      # (purse-first#163), which keeps its dagnabit coupling in the repo that owns
      # dagnabit.
      #
      # IN-TREE PATHS ARE A CONSUMPTION CONTRACT. Because conformist has no just-us
      # flake input, it cannot reach these flake outputs; it imports the modules
      # by in-tree path from a fixed-output fetch of just-us source. The paths
      # below — nix/linters/justfile-*.nix, nix/justfile-common.nix,
      # nix/justfile-model.nix, nix/presets/justfile.nix — are therefore
      # load-bearing for conformist: moving them is a coordinated breaking change.
      # See docs/features/0003-recipe-model.md.
      lib.conformistLinters = {
        justfile-recipe-names = ./nix/linters/justfile-recipe-names.nix;
        justfile-task-hierarchy = ./nix/linters/justfile-task-hierarchy.nix;
        justfile-recipe-descriptions = ./nix/linters/justfile-recipe-descriptions.nix;
        justfile-debug-recipes = ./nix/linters/justfile-debug-recipes.nix;
        justfile-leaf-noun = ./nix/linters/justfile-leaf-noun.nix;
        justfile-aggregate-comments = ./nix/linters/justfile-aggregate-comments.nix;
        justfile-default = ./nix/linters/justfile-default.nix;
        justfile-orphan-summary = ./nix/linters/justfile-orphan-summary.nix;
      };

      lib.conformistPresets.justfile = ./nix/presets/justfile.nix;
    };
}
