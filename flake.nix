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

          # eng conformance linters. NOT enabled: eng-versioning (derives
          # the version key from go.mod; Rust repos unsupported yet) and
          # justfile-recipe-names (the upstream-heritage demo recipes are
          # deliberately non-verb-noun).
          linters.agents-md.enable = true;
          linters.justfile-default.enable = true;

          # Dogfood the fork's own linter: no recipe may hide prose above
          # the single comment line `just --list` prints as its description.
          # The module is the same file this flake exports as
          # `lib.conformistLinters.justfile-orphan-summary` (below); it now reads
          # the recipe model via the fork-only `--dump-format model`, and
          # `linters.justfile-common.justPackage` (the shared option every
          # justfile-* linter reads) is the just built right here, so the check
          # reads the fork's `doc_prelude` rather than passing vacuously against
          # an upstream `just`.
          #
          # Only orphan-summary is dogfooded here for now. The rest of the roster
          # (recipe-names, leaf-noun, aggregate-comments, ...) waits on two
          # things: the upstream-heritage demo recipes (quine, polyglot, rule110,
          # ...) are deliberately non-conformant, and conformist still ships its
          # own copies of these seven, so enabling just-us's would double-declare
          # them until conformist's half drops them and FODs this repo. Full-
          # roster dogfooding is a follow-up synchronized with that landing.
          imports = [ ./nix/linters/justfile-orphan-summary.nix ];
          linters.justfile-orphan-summary.enable = true;
          linters.justfile-common.justPackage = just;
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
      in
      {
        packages = batsLib.batsLaneOutputs // {
          default = just;
        };

        checks = {
          bats-default = batsLib.batsLaneOutputs.bats-default;
          formatting = conformistEval.config.build.check self;
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
