use super::*;

#[derive(Debug, Default, PartialEq, Clone, Copy, Serialize, ValueEnum, EnumString)]
#[strum(serialize_all = "kebab-case")]
pub(crate) enum TapStream {
  #[default]
  Buffered,
  #[strum(serialize = "comments")]
  #[value(alias = "comments")]
  StreamedOutput,
  Stderr,
}
