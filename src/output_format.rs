use super::*;

#[derive(Debug, Default, PartialEq, Clone, Copy, Serialize)]
pub(crate) enum OutputFormat {
  #[default]
  TapStreamedOutput,
  Default,
  Tap,
  TapStderr,
}

impl std::str::FromStr for OutputFormat {
  type Err = String;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    match s {
      "tap+streamed_output" => Ok(Self::TapStreamedOutput),
      "default" => Ok(Self::Default),
      "tap" => Ok(Self::Tap),
      "tap+stderr" => Ok(Self::TapStderr),
      other => Err(format!("unknown output format: {other}")),
    }
  }
}

impl std::fmt::Display for OutputFormat {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::TapStreamedOutput => write!(f, "tap+streamed_output"),
      Self::Default => write!(f, "default"),
      Self::Tap => write!(f, "tap"),
      Self::TapStderr => write!(f, "tap+stderr"),
    }
  }
}

impl OutputFormat {
  pub(crate) fn is_tap(self) -> bool {
    matches!(self, Self::Tap | Self::TapStreamedOutput | Self::TapStderr)
  }
}
