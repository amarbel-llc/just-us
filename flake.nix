{
  description = "just - a command runner";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/4590696c8693fea477850fe379a01544293ca4e2";
    nixpkgs-master.url = "github:NixOS/nixpkgs/e2dde111aea2c0699531dc616112a96cd55ab8b5";
    utils.url = "https://flakehub.com/f/numtide/flake-utils/0.1.102";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
    purse-first.url = "github:amarbel-llc/purse-first";
    bob.url = "github:amarbel-llc/bob";
  };

  outputs =
    {
      self,
      nixpkgs,
      nixpkgs-master,
      utils,
      rust-overlay,
      crane,
      purse-first,
      bob,
    }:
    utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        pkgs-master = import nixpkgs-master { inherit system; };

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

        # Tests that require /usr/bin/env (shebangs), USER env var,
        # or other host facilities unavailable inside the Nix sandbox.
        skippedTests = [
          # shebang tests — all use #!/usr/bin/env
          "shebang::run_shebang"
          "script::multiline_shebang_line_numbers"
          "script::shebang_line_numbers"
          "script::shebang_line_numbers_with_multiline_constructs"
          "imports::shebang_recipes_in_imports_in_root_run_in_justfile_directory"
          "interpolation::shebang_line_numbers_are_correct_with_multi_line_interpolations"
          "no_exit_message::shebang_exit_message_setting_suppressed"
          "no_exit_message::shebang_exit_message_suppressed"
          "unexport::unexport_environment_variable_shebang"
          "tempdir::argument_overrides_setting"
          "tempdir::setting"
          # editor/chooser tests — scripts use shebangs
          "edit::editor_working_directory"
          "edit::status_error"
          "choose::status_error"
          "quiet::choose_status"
          # working_directory tests — use #!/usr/bin/env sh
          "working_directory::change_working_directory_to_search_justfile_parent"
          "working_directory::justfile_and_working_directory"
          "working_directory::justfile_without_working_directory"
          "working_directory::justfile_without_working_directory_relative"
          "working_directory::search_dir_child"
          "working_directory::search_dir_parent"
          # shell/backtick tests — can't find shell in sandbox
          "backticks::trailing_newlines_are_stripped"
          "shell::flag"
          # misc sandbox incompatibilities
          "command::command_not_found"
          "functions::env_var_functions_unix" # USER env var not set
          "functions::path_functions" # uses /usr/bin/env echo
          "functions::path_functions2" # uses /usr/bin/env echo
        ];

        skipArgs = builtins.concatStringsSep " " (builtins.map (t: "--skip ${t}") skippedTests);

        just = craneLib.buildPackage (
          commonArgs
          // {
            inherit cargoArtifacts;

            nativeCheckInputs = with pkgs; [
              bashInteractive
              coreutils
            ];

            cargoTestExtraArgs = "-- ${skipArgs}";
          }
        );

        just-us-agents-unwrapped = craneLib.buildPackage (
          commonArgs
          // {
            inherit cargoArtifacts;
            cargoExtraArgs = "-p just-us-agents";
            doCheck = false;
          }
        );

        just-us-agents = pkgs.runCommand "just-us-agents" { } ''
          mkdir -p $out/bin
          cp ${just-us-agents-unwrapped}/bin/just-us-agents $out/bin/

          ${purse-first.packages.${system}.purse-first}/bin/purse-first generate-plugin \
            --root ${./.} \
            --output $out
        '';
      in
      {
        packages = {
          default = pkgs.symlinkJoin {
            name = "just-us";
            paths = [
              just
              just-us-agents
            ];
          };
          just = just;
          just-us-agents = just-us-agents;
        };

        devShells.default = pkgs-master.mkShell {
          packages = [
            rustToolchain
            pkgs-master.cargo-deny
            pkgs-master.cargo-edit
            pkgs-master.cargo-watch
            pkgs-master.rust-analyzer
            pkgs.bashInteractive
            pkgs.openssl
            pkgs.pkg-config
            pkgs.just
            bob.packages.${system}.batman
          ];
        };
      }
    );
}
