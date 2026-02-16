use super::*;

#[test]
fn single_passing_recipe() {
  Test::new()
    .justfile(
      "
      build:
        echo hello
      ",
    )
    .arg("--tap")
    .arg("build")
    .stdout_regex("TAP version 14\n1\\.\\.1\nok 1 - build\n  ---\n  output: \\|\n    hello\n  \\.\\.\\.\n")
    .stderr("")
    .success();
}

#[test]
fn single_failing_recipe() {
  Test::new()
    .justfile(
      "
      test:
        @exit 1
      ",
    )
    .arg("--tap")
    .arg("test")
    .stdout_regex("TAP version 14\n1\\.\\.1\nnot ok 1 - test\n  ---\n  message: \".*\"\n  severity: fail\n  exitcode: 1\n  \\.\\.\\.\n")
    .stderr("")
    .failure();
}

#[test]
fn multiple_recipes_all_pass() {
  Test::new()
    .justfile(
      "
      build:
        echo building

      lint:
        echo linting
      ",
    )
    .arg("--tap")
    .args(["build", "lint"])
    .stdout_regex("TAP version 14\n1\\.\\.2\nok 1 - build\n  ---\n  output: \\|\n    building\n  \\.\\.\\.\nok 2 - lint\n  ---\n  output: \\|\n    linting\n  \\.\\.\\.\n")
    .stderr("")
    .success();
}

#[test]
fn mixed_results_continues_past_failure() {
  Test::new()
    .justfile(
      "
      build:
        echo building

      test:
        @exit 1

      lint:
        echo linting
      ",
    )
    .arg("--tap")
    .args(["build", "test", "lint"])
    .stdout_regex("TAP version 14\n1\\.\\.3\nok 1 - build\n  ---\n  output: \\|\n    building\n  \\.\\.\\.\nnot ok 2 - test\n  ---\n  message: \".*\"\n  severity: fail\n  exitcode: 1\n  \\.\\.\\.\nok 3 - lint\n  ---\n  output: \\|\n    linting\n  \\.\\.\\.\n")
    .stderr("")
    .failure();
}

#[test]
fn tap_captures_recipe_output() {
  Test::new()
    .justfile(
      "
      build:
        echo captured-output
      ",
    )
    .arg("--tap")
    .arg("build")
    .stdout_regex("TAP version 14\n1\\.\\.1\nok 1 - build\n  ---\n  output: \\|\n    captured-output\n  \\.\\.\\.\n")
    .stderr("")
    .success();
}

#[test]
fn tap_with_env_var() {
  Test::new()
    .justfile(
      "
      build:
        echo hello
      ",
    )
    .env("JUST_TAP", "true")
    .arg("build")
    .stdout_regex("TAP version 14\n1\\.\\.1\nok 1 - build\n  ---\n  output: \\|\n    hello\n  \\.\\.\\.\n")
    .stderr("")
    .success();
}

#[test]
fn tap_expands_dependencies() {
  Test::new()
    .justfile(
      "
      compile:
        echo compiling

      build: compile
        echo building
      ",
    )
    .arg("--tap")
    .arg("build")
    .stdout_regex("TAP version 14\n1\\.\\.2\nok 1 - compile\n  ---\n  output: \\|\n    compiling\n  \\.\\.\\.\nok 2 - build\n  ---\n  output: \\|\n    building\n  \\.\\.\\.\n")
    .stderr("")
    .success();
}

#[test]
fn tap_expands_deep_dependencies() {
  Test::new()
    .justfile(
      "
      compile:
        echo compiling

      build: compile
        echo building

      test: build
        echo testing
      ",
    )
    .arg("--tap")
    .arg("test")
    .stdout_regex("TAP version 14\n1\\.\\.3\nok 1 - compile\n  ---\n  output: \\|\n    compiling\n  \\.\\.\\.\nok 2 - build\n  ---\n  output: \\|\n    building\n  \\.\\.\\.\nok 3 - test\n  ---\n  output: \\|\n    testing\n  \\.\\.\\.\n")
    .stderr("")
    .success();
}

#[test]
fn tap_failing_dependency() {
  Test::new()
    .justfile(
      "
      compile:
        @exit 1

      build: compile
        echo building
      ",
    )
    .arg("--tap")
    .arg("build")
    .stdout_regex("TAP version 14\n1\\.\\.2\nnot ok 1 - compile\n  ---\n  message: \".*\"\n  severity: fail\n  exitcode: 1\n  \\.\\.\\.\n")
    .stderr("")
    .failure();
}

#[test]
fn tap_quiet_recipe_output_captured() {
  Test::new()
    .justfile(
      "
      build:
        @echo quiet-output
      ",
    )
    .arg("--tap")
    .arg("build")
    .stdout_regex("TAP version 14\n1\\.\\.1\nok 1 - build\n  ---\n  output: \\|\n    quiet-output\n  \\.\\.\\.\n")
    .stderr("")
    .success();
}

#[test]
fn tap_no_output_no_yaml_block() {
  Test::new()
    .justfile(
      "
      build:
        @true
      ",
    )
    .arg("--tap")
    .arg("build")
    .stdout(
      "
      TAP version 14
      1..1
      ok 1 - build
      ",
    )
    .stderr("")
    .success();
}

#[test]
fn tap_shared_dependency_runs_once() {
  Test::new()
    .justfile(
      "
      compile:
        echo compiling

      build: compile
        echo building

      test: compile
        echo testing
      ",
    )
    .arg("--tap")
    .args(["build", "test"])
    .stdout_regex("TAP version 14\n1\\.\\.3\nok 1 - compile\n  ---\n  output: \\|\n    compiling\n  \\.\\.\\.\nok 2 - build\n  ---\n  output: \\|\n    building\n  \\.\\.\\.\nok 3 - test\n  ---\n  output: \\|\n    testing\n  \\.\\.\\.\n")
    .stderr("")
    .success();
}
