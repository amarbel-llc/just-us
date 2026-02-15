use super::*;

pub(crate) struct TapTestResult {
  pub(crate) number: usize,
  pub(crate) name: String,
  pub(crate) ok: bool,
  pub(crate) error_message: Option<String>,
  pub(crate) exit_code: Option<i32>,
}

pub(crate) fn write_version(writer: &mut impl Write) -> io::Result<()> {
  writeln!(writer, "TAP version 14")
}

pub(crate) fn write_plan(writer: &mut impl Write, count: usize) -> io::Result<()> {
  writeln!(writer, "1..{count}")
}

pub(crate) fn write_test_point(writer: &mut impl Write, result: &TapTestResult) -> io::Result<()> {
  let status = if result.ok { "ok" } else { "not ok" };
  writeln!(writer, "{status} {} - {}", result.number, result.name)?;

  if !result.ok {
    writeln!(writer, "  ---")?;
    if let Some(ref message) = result.error_message {
      writeln!(writer, "  message: \"{message}\"")?;
    }
    writeln!(writer, "  severity: fail")?;
    if let Some(code) = result.exit_code {
      writeln!(writer, "  exitcode: {code}")?;
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
    };
    write_test_point(&mut buf, &result).unwrap();
    assert_eq!(String::from_utf8(buf).unwrap(), "ok 1 - build\n");
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
    };
    write_test_point(&mut buf, &result).unwrap();
    let output = String::from_utf8(buf).unwrap();
    assert_eq!(
      output,
      "not ok 2 - test\n  ---\n  message: \"Recipe `test` failed on line 5 with exit code 1\"\n  severity: fail\n  exitcode: 1\n  ...\n"
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
    };
    write_test_point(&mut buf, &result).unwrap();
    let output = String::from_utf8(buf).unwrap();
    assert!(output.contains("not ok 1 - broken"));
    assert!(output.contains("severity: fail"));
    assert!(!output.contains("exitcode"));
  }
}
