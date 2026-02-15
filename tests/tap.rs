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
    .stdout_regex("TAP version 14\n1..1\nnot ok 1 - test\n  ---\n  message: \".*\"\n  severity: fail\n  exitcode: 1\n  ...\n")
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
    .stdout(
      "
      TAP version 14
      1..2
      ok 1 - build
      ok 2 - lint
      ",
    )
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
    .stdout_regex("TAP version 14\n1..3\nok 1 - build\nnot ok 2 - test\n  ---\n  message: \".*\"\n  severity: fail\n  exitcode: 1\n  ...\nok 3 - lint\n")
    .stderr("")
    .failure();
}

#[test]
fn tap_suppresses_recipe_output() {
  Test::new()
    .justfile(
      "
      build:
        echo should-not-appear
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
