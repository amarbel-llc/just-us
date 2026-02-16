use super::*;

pub(crate) struct TapTestResult {
  pub(crate) number: usize,
  pub(crate) name: String,
  pub(crate) ok: bool,
  pub(crate) error_message: Option<String>,
  pub(crate) exit_code: Option<i32>,
  pub(crate) output: Option<String>,
}

pub(crate) struct TapWriter {
  pub(crate) counter: usize,
  pub(crate) failures: usize,
}

impl TapWriter {
  pub(crate) fn new() -> Self {
    Self {
      counter: 0,
      failures: 0,
    }
  }
}

pub(crate) fn write_version(writer: &mut impl Write) -> io::Result<()> {
  writeln!(writer, "TAP version 14")
}

pub(crate) fn write_plan(writer: &mut impl Write, count: usize) -> io::Result<()> {
  writeln!(writer, "1..{count}")
}

fn write_yaml_field(writer: &mut impl Write, key: &str, value: &str) -> io::Result<()> {
  if value.contains('\n') {
    writeln!(writer, "  {key}: |")?;
    for line in value.lines() {
      let line = line.rsplit('\r').next().unwrap_or(line);
      writeln!(writer, "    {line}")?;
    }
  } else {
    let value = value.rsplit('\r').next().unwrap_or(value);
    writeln!(writer, "  {key}: \"{value}\"")?;
  }
  Ok(())
}

fn has_yaml_block(result: &TapTestResult) -> bool {
  !result.ok || result.output.is_some()
}

pub(crate) fn write_test_point(writer: &mut impl Write, result: &TapTestResult) -> io::Result<()> {
  let status = if result.ok { "ok" } else { "not ok" };
  writeln!(writer, "{status} {} - {}", result.number, result.name)?;

  if has_yaml_block(result) {
    writeln!(writer, "  ---")?;
    if let Some(ref message) = result.error_message {
      write_yaml_field(writer, "message", message)?;
    }
    if !result.ok {
      writeln!(writer, "  severity: fail")?;
    }
    if let Some(code) = result.exit_code {
      writeln!(writer, "  exitcode: {code}")?;
    }
    if let Some(ref output) = result.output {
      write_yaml_field(writer, "output", output)?;
    }
    writeln!(writer, "  ...")?;
  }

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn version_line() {
    let mut buf = Vec::new();
    write_version(&mut buf).unwrap();
    assert_eq!(String::from_utf8(buf).unwrap(), "TAP version 14\n");
  }

  #[test]
  fn plan_line() {
    let mut buf = Vec::new();
    write_plan(&mut buf, 3).unwrap();
    assert_eq!(String::from_utf8(buf).unwrap(), "1..3\n");
  }

  #[test]
  fn passing_test_point() {
    let mut buf = Vec::new();
    let result = TapTestResult {
      number: 1,
      name: "build".into(),
      ok: true,
      error_message: None,
      exit_code: None,
      output: None,
    };
    write_test_point(&mut buf, &result).unwrap();
    assert_eq!(String::from_utf8(buf).unwrap(), "ok 1 - build\n");
  }

  #[test]
  fn passing_test_point_with_output() {
    let mut buf = Vec::new();
    let result = TapTestResult {
      number: 1,
      name: "build".into(),
      ok: true,
      error_message: None,
      exit_code: None,
      output: Some("building\n".into()),
    };
    write_test_point(&mut buf, &result).unwrap();
    assert_eq!(
      String::from_utf8(buf).unwrap(),
      "ok 1 - build\n  ---\n  output: |\n    building\n  ...\n"
    );
  }

  #[test]
  fn failing_test_point() {
    let mut buf = Vec::new();
    let result = TapTestResult {
      number: 2,
      name: "test".into(),
      ok: false,
      error_message: Some("Recipe `test` failed on line 5 with exit code 1".into()),
      exit_code: Some(1),
      output: None,
    };
    write_test_point(&mut buf, &result).unwrap();
    let output = String::from_utf8(buf).unwrap();
    assert_eq!(
      output,
      "not ok 2 - test\n  ---\n  message: \"Recipe `test` failed on line 5 with exit code 1\"\n  severity: fail\n  exitcode: 1\n  ...\n"
    );
  }

  #[test]
  fn failing_test_point_with_output() {
    let mut buf = Vec::new();
    let result = TapTestResult {
      number: 2,
      name: "test".into(),
      ok: false,
      error_message: Some("Recipe `test` failed on line 5 with exit code 1".into()),
      exit_code: Some(1),
      output: Some("running tests\nfailed assertion".into()),
    };
    write_test_point(&mut buf, &result).unwrap();
    let output = String::from_utf8(buf).unwrap();
    assert!(output.contains("output: |"));
    assert!(output.contains("    running tests"));
    assert!(output.contains("    failed assertion"));
  }

  #[test]
  fn output_strips_carriage_returns() {
    let mut buf = Vec::new();
    let result = TapTestResult {
      number: 1,
      name: "build".into(),
      ok: true,
      error_message: None,
      exit_code: None,
      output: Some("progress\rwarning: done\nline two\r\n".into()),
    };
    write_test_point(&mut buf, &result).unwrap();
    let output = String::from_utf8(buf).unwrap();
    assert!(
      output.contains("    warning: done\n"),
      "\\r-overwritten prefix should be stripped: {output:?}"
    );
    assert!(
      output.contains("    line two\n"),
      "trailing \\r should be stripped: {output:?}"
    );
    assert!(!output.contains('\r'), "no carriage returns in output: {output:?}");
  }

  #[test]
  fn colored_output_preserves_indentation() {
    let mut buf = Vec::new();
    let result = TapTestResult {
      number: 1,
      name: "build".into(),
      ok: true,
      error_message: None,
      exit_code: None,
      output: Some(
        "\x1b[32m  indented green\x1b[0m\r\n\x1b[31mred line\x1b[0m\r\n".into(),
      ),
    };
    write_test_point(&mut buf, &result).unwrap();
    let output = String::from_utf8(buf).unwrap();
    assert!(
      output.contains("    \x1b[32m  indented green\x1b[0m\n"),
      "colored indented output should be preserved: {output:?}"
    );
    assert!(
      output.contains("    \x1b[31mred line\x1b[0m\n"),
      "colored output should be preserved: {output:?}"
    );
    assert!(
      !output.contains('\r'),
      "no carriage returns in output: {output:?}"
    );
  }

  #[test]
  fn failing_test_point_no_exit_code() {
    let mut buf = Vec::new();
    let result = TapTestResult {
      number: 1,
      name: "broken".into(),
      ok: false,
      error_message: Some("Recipe `broken` failed for an unknown reason".into()),
      exit_code: None,
      output: None,
    };
    write_test_point(&mut buf, &result).unwrap();
    let output = String::from_utf8(buf).unwrap();
    assert!(output.contains("not ok 1 - broken"));
    assert!(output.contains("severity: fail"));
    assert!(!output.contains("exitcode"));
  }
}
