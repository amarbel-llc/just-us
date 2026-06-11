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
  };

  outputs = { self, nixpkgs, flake-utils, bats, ... }@inputs:
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

        batsLib = import ./bats.nix {
          inherit pkgs;
          myBin = just;
          batsLane = inputs.bats.lib.${system}.batsLane;
          bats-libs = inputs.bats.packages.${system}.bats-libs;
          batsSrc = pkgs.lib.cleanSourceWith {
            src = ./zz-tests_bats;
            filter = path: type:
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
        };

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
          ];
        };
      }
    );
}
