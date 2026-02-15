{
  description = "just - a command runner";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/23d72dabcb3b12469f57b37170fcbc1789bd7457";
    nixpkgs-master.url = "github:NixOS/nixpkgs/b28c4999ed71543e71552ccfd0d7e68c581ba7e9";
    utils.url = "https://flakehub.com/f/numtide/flake-utils/0.1.102";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
    rust.url = "github:friedenberg/eng?dir=devenvs/rust";
  };

  outputs =
    {
      self,
      nixpkgs,
      nixpkgs-master,
      utils,
      rust-overlay,
      crane,
      rust,
    }:
    utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };

        rustToolchain = pkgs.rust-bin.stable.latest.default;
        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        src = pkgs.lib.cleanSourceWith {
          src = ./.;
          filter =
            path: type:
            (craneLib.filterCargoSources path type)
            || (builtins.match ".*\\.md$" path != null)
            || (builtins.match ".*completions/.*" path != null);
        };

        commonArgs = {
          inherit src;
          strictDeps = true;
          buildInputs = [ ];
          nativeBuildInputs = [ ];
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        just = craneLib.buildPackage (
          commonArgs
          // {
            inherit cargoArtifacts;
            # Integration tests require shell access and temp dirs
            # unavailable in the Nix sandbox
            cargoTestExtraArgs = "--lib";
          }
        );
      in
      {
        packages = {
          default = just;
          just = just;
        };

        devShells.default = rust.devShells.${system}.default;
      }
    );
}
