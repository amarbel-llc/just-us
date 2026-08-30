use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, ValueEnum)]
pub(crate) enum DumpFormat {
  Json,
  /// A normalized, flat recipe model for policy consumers (the conformist
  /// `justfile-*` linters). Fork-only; see `src/recipe_model.rs` and
  /// `docs/features/0003-recipe-model.md`.
  Model,
  #[default]
  Just,
}
