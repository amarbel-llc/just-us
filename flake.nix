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
    );
}
