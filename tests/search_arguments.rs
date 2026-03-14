use super::*;

#[test]
fn argument_with_different_path_prefix_is_allowed() {
  Test::new()
    .justfile("foo bar:")
    .args(["./foo", "../bar"])
    .success();
}

#[test]
fn passing_dot_as_argument_is_allowed() {
  Test::new()
    .justfile(
      "
        say ARG:
          echo {{ARG}}
      ",
    )
    .write(
      "child/justfile",
      "set output-format := \"default\"\nsay ARG:\n '{{just_executable()}}' --output-format default ../say {{ARG}}",
    )
    .current_dir("child")
    .args(["say", "."])
    .stdout(".\n")
    .stderr_regex("'.*' --output-format default ../say .\necho .\n")
    .success();
}
