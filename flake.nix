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

        # Tests that require /usr/bin/env (shebangs), USER env var,
        # or other host facilities unavailable inside the Nix sandbox.
        skippedTests = [
          "completions::bash" # needs git repo + cargo build
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
          "edit::editor_precedence"
          "edit::editor_working_directory"
          "edit::status_error"
          "choose::default"
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

        skipArgs = builtins.concatStringsSep " " (
          builtins.map (t: "--skip ${t}") skippedTests
        );

        just = craneLib.buildPackage (
          commonArgs
          // {
            inherit cargoArtifacts;

            nativeCheckInputs = with pkgs; [
              bash
              coreutils
            ];

            cargoTestExtraArgs = "-- ${skipArgs}";
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
