use super::*;

#[derive(Debug, Default, PartialEq, Clone, Copy, Serialize, ValueEnum, EnumString)]
#[strum(serialize_all = "kebab-case")]
pub(crate) enum TapStream {
  Buffered,
  #[default]
  #[strum(serialize = "comments")]
  #[value(alias = "comments")]
  StreamedOutput,
  Stderr,
}
