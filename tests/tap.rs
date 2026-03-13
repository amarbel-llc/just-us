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
    .env("LC_ALL", "C")
    .args(["--output-format", "tap"])
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
    .env("LC_ALL", "C")
    .args(["--output-format", "tap"])
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
    .env("LC_ALL", "C")
    .args(["--output-format", "tap"])
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
    .env("LC_ALL", "C")
    .args(["--output-format", "tap"])
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
    .env("LC_ALL", "C")
    .args(["--output-format", "tap"])
    .arg("build")
    .stdout_regex("TAP version 14\n1\\.\\.1\nok 1 - build\n  ---\n  output: \\|\n    captured-output\n  \\.\\.\\.\n")
    .stderr("")
    .success();
}

#[test]
fn output_format_with_env_var() {
  Test::new()
    .justfile(
      "
      build:
        echo hello
      ",
    )
    .env("LC_ALL", "C")
    .env("JUST_OUTPUT_FORMAT", "tap")
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
    .env("LC_ALL", "C")
    .args(["--output-format", "tap"])
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
    .env("LC_ALL", "C")
    .args(["--output-format", "tap"])
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
    .env("LC_ALL", "C")
    .args(["--output-format", "tap"])
    .arg("build")
    .stdout_regex("TAP version 14\n1\\.\\.2\nnot ok 1 - compile\n  ---\n  message: \".*\"\n  severity: fail\n  exitcode: 1\n  \\.\\.\\.\n")
    .stderr("")
    .failure();
}

#[test]
fn tap_quiet_recipe_suppresses_yaml() {
  Test::new()
    .justfile(
      "
      @build:
        echo quiet-output
      ",
    )
    .env("LC_ALL", "C")
    .args(["--output-format", "tap"])
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
fn tap_no_output_no_yaml_block() {
  Test::new()
    .justfile(
      "
      build:
        @true
      ",
    )
    .env("LC_ALL", "C")
    .args(["--output-format", "tap"])
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
    .env("LC_ALL", "C")
    .args(["--output-format", "tap"])
    .args(["build", "test"])
    .stdout_regex("TAP version 14\n1\\.\\.3\nok 1 - compile\n  ---\n  output: \\|\n    compiling\n  \\.\\.\\.\nok 2 - build\n  ---\n  output: \\|\n    building\n  \\.\\.\\.\nok 3 - test\n  ---\n  output: \\|\n    testing\n  \\.\\.\\.\n")
    .stderr("")
    .success();
}

#[test]
fn output_format_justfile_setting() {
  Test::new()
    .justfile(
      "
      set output-format := \"tap\"

      build:
        echo hello
      ",
    )
    .env("LC_ALL", "C")
    .arg("build")
    .stdout_regex("TAP version 14\n1\\.\\.1\nok 1 - build\n  ---\n  output: \\|\n    hello\n  \\.\\.\\.\n")
    .stderr("")
    .success();
}

#[test]
fn output_format_cli_overrides_justfile() {
  Test::new()
    .justfile(
      "
      set output-format := \"tap\"

      build:
        echo hello
      ",
    )
    .args(["--output-format", "default"])
    .arg("build")
    .stdout("hello\n")
    .stderr("echo hello\n")
    .success();
}

#[test]
fn tap_set_quiet_suppresses_yaml() {
  Test::new()
    .justfile(
      "
      set quiet
      set output-format := \"tap\"

      build:
        echo hello
      ",
    )
    .env("LC_ALL", "C")
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
fn tap_cli_quiet_suppresses_yaml() {
  Test::new()
    .justfile(
      "
      build:
        echo hello
      ",
    )
    .env("LC_ALL", "C")
    .args(["--output-format", "tap", "--quiet"])
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
fn tap_quiet_failing_suppresses_yaml() {
  Test::new()
    .justfile(
      "
      @test:
        exit 1
      ",
    )
    .env("LC_ALL", "C")
    .args(["--output-format", "tap"])
    .arg("test")
    .stdout(
      "
      TAP version 14
      1..1
      not ok 1 - test
      ",
    )
    .stderr("")
    .failure();
}

#[test]
fn tap_no_quiet_overrides_set_quiet() {
  Test::new()
    .justfile(
      "
      set quiet
      set output-format := \"tap\"

      [no-quiet]
      build:
        echo hello
      ",
    )
    .env("LC_ALL", "C")
    .arg("build")
    .stdout_regex("TAP version 14\n1\\.\\.1\nok 1 - build\n  ---\n  output: \\|\n    hello\n  \\.\\.\\.\n")
    .stderr("")
    .success();
}

#[test]
fn tap_stream_comments_single_recipe() {
  Test::new()
    .justfile(
      "
      build:
        echo hello
      ",
    )
    .env("LC_ALL", "C")
    .args(["--output-format", "tap", "--tap-stream", "comments"])
    .arg("build")
    .stdout_regex("TAP version 14\npragma \\+streamed-output\n1\\.\\.1\n# hello\nok 1 - build\n")
    .stderr("")
    .success();
}

#[test]
fn tap_stream_comments_failing() {
  Test::new()
    .justfile(
      "
      test:
        @exit 1
      ",
    )
    .env("LC_ALL", "C")
    .args(["--output-format", "tap", "--tap-stream", "comments"])
    .arg("test")
    .stdout_regex("TAP version 14\npragma \\+streamed-output\n1\\.\\.1\nnot ok 1 - test\n  ---\n  message: \".*\"\n  severity: fail\n  exitcode: 1\n  \\.\\.\\.\n")
    .stderr("")
    .failure();
}

#[test]
fn tap_stream_comments_no_output_field() {
  Test::new()
    .justfile(
      "
      build:
        echo hello
      ",
    )
    .env("LC_ALL", "C")
    .args(["--output-format", "tap", "--tap-stream", "comments"])
    .arg("build")
    .stdout_regex("TAP version 14\npragma \\+streamed-output\n1\\.\\.1\n# hello\nok 1 - build\n$")
    .stderr("")
    .success();
}

#[test]
fn tap_stream_stderr_single_recipe() {
  Test::new()
    .justfile(
      "
      build:
        echo hello
      ",
    )
    .env("LC_ALL", "C")
    .args(["--output-format", "tap", "--tap-stream", "stderr"])
    .arg("build")
    .stdout_regex("TAP version 14\n1\\.\\.1\nok 1 - build\n  ---\n  output: \\|\n    hello\n  \\.\\.\\.\n")
    .stderr_regex("hello\n")
    .success();
}

#[test]
fn tap_stream_stderr_failing() {
  Test::new()
    .justfile(
      "
      test:
        @exit 1
      ",
    )
    .env("LC_ALL", "C")
    .args(["--output-format", "tap", "--tap-stream", "stderr"])
    .arg("test")
    .stdout_regex("TAP version 14\n1\\.\\.1\nnot ok 1 - test\n  ---\n  message: \".*\"\n  severity: fail\n  exitcode: 1\n  \\.\\.\\.\n")
    .failure();
}

#[test]
fn tap_stream_buffered_explicit() {
  Test::new()
    .justfile(
      "
      build:
        echo hello
      ",
    )
    .env("LC_ALL", "C")
    .args(["--output-format", "tap", "--tap-stream", "buffered"])
    .arg("build")
    .stdout_regex("TAP version 14\n1\\.\\.1\nok 1 - build\n  ---\n  output: \\|\n    hello\n  \\.\\.\\.\n")
    .stderr("")
    .success();
}

#[test]
fn tap_stream_justfile_setting() {
  Test::new()
    .justfile(
      r#"
      set output-format := "tap"
      set tap-stream := "comments"

      build:
        echo hello
      "#,
    )
    .env("LC_ALL", "C")
    .arg("build")
    .stdout_regex("TAP version 14\npragma \\+streamed-output\n1\\.\\.1\n# hello\nok 1 - build\n")
    .stderr("")
    .success();
}

#[test]
fn tap_stream_cli_overrides_setting() {
  Test::new()
    .justfile(
      r#"
      set output-format := "tap"
      set tap-stream := "comments"

      build:
        echo hello
      "#,
    )
    .env("LC_ALL", "C")
    .args(["--tap-stream", "buffered"])
    .arg("build")
    .stdout_regex("TAP version 14\n1\\.\\.1\nok 1 - build\n  ---\n  output: \\|\n    hello\n  \\.\\.\\.\n")
    .stderr("")
    .success();
}

#[test]
fn tap_stream_env_var() {
  Test::new()
    .justfile(
      "
      build:
        echo hello
      ",
    )
    .env("LC_ALL", "C")
    .args(["--output-format", "tap"])
    .env("JUST_TAP_STREAM", "comments")
    .arg("build")
    .stdout_regex("TAP version 14\npragma \\+streamed-output\n1\\.\\.1\n# hello\nok 1 - build\n")
    .stderr("")
    .success();
}

#[test]
fn tap_stream_comments_multiline() {
  Test::new()
    .justfile(
      "
      build:
        echo line1
        echo line2
      ",
    )
    .env("LC_ALL", "C")
    .args(["--output-format", "tap", "--tap-stream", "comments"])
    .arg("build")
    .stdout_regex("TAP version 14\npragma \\+streamed-output\n1\\.\\.1\n# line1\n# line2\nok 1 - build\n")
    .stderr("")
    .success();
}

#[test]
fn tap_recipe_comment_as_tap_comment() {
  Test::new()
    .justfile(
      "
      # Build the project
      build:
        echo building
      ",
    )
    .env("LC_ALL", "C")
    .args(["--output-format", "tap"])
    .arg("build")
    .stdout_regex("TAP version 14\n1\\.\\.1\nok 1 - build # Build the project\n  ---\n  output: \\|\n    building\n  \\.\\.\\.\n")
    .stderr("")
    .success();
}

#[test]
fn tap_recipe_doc_attribute_as_tap_comment() {
  Test::new()
    .justfile(
      r#"
      [doc("Run the test suite")]
      test:
        echo testing
      "#,
    )
    .env("LC_ALL", "C")
    .args(["--output-format", "tap"])
    .arg("test")
    .stdout_regex("TAP version 14\n1\\.\\.1\nok 1 - test # Run the test suite\n  ---\n  output: \\|\n    testing\n  \\.\\.\\.\n")
    .stderr("")
    .success();
}

#[test]
fn tap_no_comment_without_doc() {
  Test::new()
    .justfile(
      "
      build:
        echo building
      ",
    )
    .env("LC_ALL", "C")
    .args(["--output-format", "tap"])
    .arg("build")
    .stdout_regex("TAP version 14\n1\\.\\.1\nok 1 - build\n  ---\n  output: \\|\n    building\n  \\.\\.\\.\n")
    .stderr("")
    .success();
}

#[test]
fn tap_multiple_recipes_with_comments() {
  Test::new()
    .justfile(
      "
      # Compile the source
      compile:
        echo compiling

      # Run the linter
      lint:
        echo linting
      ",
    )
    .env("LC_ALL", "C")
    .args(["--output-format", "tap"])
    .args(["compile", "lint"])
    .stdout_regex("TAP version 14\n1\\.\\.2\nok 1 - compile # Compile the source\n  ---\n  output: \\|\n    compiling\n  \\.\\.\\.\nok 2 - lint # Run the linter\n  ---\n  output: \\|\n    linting\n  \\.\\.\\.\n")
    .stderr("")
    .success();
}

#[test]
fn tap_stream_streamed_output_canonical_name() {
  Test::new()
    .justfile(
      "
      build:
        echo hello
      ",
    )
    .env("LC_ALL", "C")
    .args(["--output-format", "tap", "--tap-stream", "streamed-output"])
    .arg("build")
    .stdout_regex("TAP version 14\npragma \\+streamed-output\n1\\.\\.1\n# hello\nok 1 - build\n")
    .stderr("")
    .success();
}

#[test]
fn tap_color_always_colorizes_ok() {
  Test::new()
    .justfile(
      "
      build:
        @true
      ",
    )
    .env("LC_ALL", "C")
    .args(["--output-format", "tap", "--color", "always"])
    .arg("build")
    .stdout_regex("TAP version 14\n1\\.\\.1\n\x1b\\[32mok\x1b\\[0m 1 - build\n")
    .stderr("")
    .success();
}

#[test]
fn tap_color_always_colorizes_not_ok() {
  Test::new()
    .justfile(
      "
      @test:
        exit 1
      ",
    )
    .env("LC_ALL", "C")
    .args(["--output-format", "tap", "--color", "always"])
    .arg("test")
    .stdout_regex("TAP version 14\n1\\.\\.1\n\x1b\\[31mnot ok\x1b\\[0m 1 - test\n")
    .stderr("")
    .failure();
}

#[test]
fn tap_color_never_no_ansi() {
  Test::new()
    .justfile(
      "
      build:
        @true
      ",
    )
    .env("LC_ALL", "C")
    .args(["--output-format", "tap", "--color", "never"])
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
fn tap_locale_emits_pragma_and_formats_plan() {
  Test::new()
    .justfile(
      "
      @build:
        true
      ",
    )
    .args(["--output-format", "tap", "--color", "never"])
    .env("LC_ALL", "en_US.UTF-8")
    .arg("build")
    .stdout(
      "
      TAP version 14
      pragma +locale-formatting:en-US
      1..1
      ok 1 - build
      ",
    )
    .stderr("")
    .success();
}

#[test]
fn tap_locale_lc_all_posix_underscore() {
  Test::new()
    .justfile(
      "
      @build:
        true
      ",
    )
    .args(["--output-format", "tap", "--color", "never"])
    .env("LC_ALL", "de_DE.UTF-8")
    .arg("build")
    .stdout(
      "
      TAP version 14
      pragma +locale-formatting:de-DE
      1..1
      ok 1 - build
      ",
    )
    .stderr("")
    .success();
}

#[test]
fn tap_color_always_yaml_output_preserves_sgr() {
  Test::new()
    .justfile(
      "
      build:
        printf '\\033[1mbold output\\033[0m\\n'
      ",
    )
    .env("LC_ALL", "C")
    .args(["--output-format", "tap", "--color", "always"])
    .arg("build")
    .stdout_regex(
      "TAP version 14\n1\\.\\.1\n\x1b\\[32mok\x1b\\[0m 1 - build\n  ---\n  output: \\|\n    \x1b\\[1mbold output\x1b\\[0m\n  \\.\\.\\.\n",
    )
    .stderr("")
    .success();
}

#[test]
fn tap_color_never_yaml_output_strips_ansi() {
  Test::new()
    .justfile(
      "
      build:
        printf '\\033[1mbold output\\033[0m\\n'
      ",
    )
    .env("LC_ALL", "C")
    .args(["--output-format", "tap", "--color", "never"])
    .arg("build")
    .stdout(
      "
      TAP version 14
      1..1
      ok 1 - build
        ---
        output: |
          bold output
        ...
      ",
    )
    .stderr("")
    .success();
}
