use super::*;

#[derive(Debug, Default, PartialEq, Clone, Copy, ValueEnum, EnumString)]
#[strum(serialize_all = "kebab-case")]
pub(crate) enum OutputFormat {
  #[default]
  Default,
  Tap,
}
