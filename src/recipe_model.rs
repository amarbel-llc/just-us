use super::*;

/// The `--dump-format model` payload: a normalized, flat projection of the
/// compiled justfile built for policy consumers (the conformist `justfile-*`
/// linters), so a consumer reads facts by field lookup instead of
/// reconstructing recipe structure from the raw `--dump-format json` tree.
///
/// This is a **pure projection** over the already-compiled `Justfile`. It adds
/// no fields to the churny AST structs (`Recipe`, `Justfile`) — keeping the
/// fork's rebase burden confined to this module plus the one `DumpFormat`
/// variant and dump match-arm — and it carries only DATA. Policy (verb
/// allowlists, the aggregate/leaf taxonomy, lifecycle groups) stays in
/// conformist; the model never emits a derived `is_aggregate` or `verb`.
///
/// The contract — the schema, its versioning, and every field's meaning — is
/// `docs/features/0003-recipe-model.md`. Two structural wins it buys the
/// consumer:
///
/// - Every recipe carries its BARE `name` and its `module` path separately, so
///   verb extraction never has to strip a `mod::` qualifier off a namepath
///   (conformist#85).
/// - `recipes` is FLAT across all modules, so a consumer cannot miss
///   module-imported recipes by failing to recurse (conformist#89).
///
/// Known limitation, inherited from the compiled tree: platform-disabled
/// recipes (`[macos]` on Linux, etc.) are filtered out in `Analyzer` before
/// they reach `Justfile::recipes`, so they are absent here exactly as they are
/// from `--dump-format json` (just-us#19).
#[derive(Debug, Serialize)]
pub(crate) struct RecipeModel {
  /// Stable schema identifier. Never changes for this schema family.
  pub(crate) schema: &'static str,
  /// Schema version. Additive fields do not bump it; a breaking change does.
  /// Consumers pin this and tolerate additive growth.
  pub(crate) version: u32,
  /// Namepath of the ROOT justfile's default recipe (the one bare `just`
  /// runs), or null when the root has no default.
  pub(crate) root_default: Option<String>,
  /// Every recipe, across the root and all modules, flattened. Sorted by
  /// `namepath` for a stable, diff-friendly order.
  pub(crate) recipes: Vec<ModelRecipe>,
  /// Every submodule, as data (the root is not included). Sorted by `path`.
  pub(crate) modules: Vec<ModelModule>,
}

/// One recipe in the flat model. Every field is normalized data; see the
/// contract doc for exact semantics.
#[derive(Debug, Serialize)]
pub(crate) struct ModelRecipe {
  /// BARE recipe name, module qualifier stripped (e.g. `debug-foo`).
  pub(crate) name: String,
  /// Fully-qualified stable identity, `::`-joined (e.g. `explore::debug-foo`).
  pub(crate) namepath: String,
  /// Containing module path components; empty for a root recipe.
  pub(crate) module: Vec<String>,
  /// Absolute path of the justfile that defines this recipe.
  pub(crate) source: String,
  /// 1-based line of the recipe name in `source`.
  pub(crate) line: usize,
  /// The single comment line `--list` prints as the description, or null.
  pub(crate) doc: Option<String>,
  /// The comment lines stranded above `doc` (the fork's `doc_prelude`), in
  /// source order. Always present here — `[]` when there are none — unlike the
  /// raw dump, which omits the key. Non-empty ⇒ the `--list` description is a
  /// truncated fragment (the `justfile-orphan-summary` signal).
  pub(crate) doc_prelude: Vec<String>,
  /// `[group(...)]` values in source order; `[]` when none.
  pub(crate) groups: Vec<String>,
  /// Attribute discriminants present, kebab-case (e.g. `group`, `private`,
  /// `linux`), de-duplicated. The forward-compatible escape hatch for any
  /// attribute the normalized fields do not cover.
  pub(crate) attributes: Vec<String>,
  /// Hidden from `--list`: name starts with `_` or carries `[private]`.
  pub(crate) private: bool,
  /// Whether this recipe is the default recipe of its OWN justfile scope.
  pub(crate) is_default: bool,
  /// Whether the recipe has body lines (the leaf signal; an aggregate has
  /// none). The taxonomy decision stays with conformist.
  pub(crate) has_body: bool,
  /// Parameter names in declaration order.
  pub(crate) parameters: Vec<String>,
  /// Dependencies with RESOLVED identities — the raw dump drops the `mod::`
  /// qualifier from dependency entries; the model carries the namepath.
  pub(crate) dependencies: Vec<ModelDependency>,
}

/// A resolved dependency edge: both the bare name and the fully-qualified
/// namepath of the depended-on recipe.
#[derive(Debug, Serialize)]
pub(crate) struct ModelDependency {
  pub(crate) name: String,
  pub(crate) namepath: String,
}

/// A submodule, carried as data. Phase 1 does NOT carry a module `doc_prelude`
/// (the parser discards it for module items; tracked as just-us#21).
#[derive(Debug, Serialize)]
pub(crate) struct ModelModule {
  /// Module path components (e.g. `["explore"]`, or `["a", "b"]` when nested).
  pub(crate) path: Vec<String>,
  /// The module's `--list` doc comment, or null.
  pub(crate) doc: Option<String>,
  /// Absolute path of the justfile that defines the module's recipes.
  pub(crate) source: String,
}

/// Stable schema identifier for the recipe model. See `docs/features/0003`.
pub(crate) const SCHEMA: &str = "just-us.recipe-model";

/// Current recipe-model schema version. Bump only on a breaking change.
pub(crate) const VERSION: u32 = 1;

impl RecipeModel {
  /// Build the flat model from a compiled root `Justfile`.
  pub(crate) fn new(root: &Justfile) -> Self {
    let mut recipes = Vec::new();
    let mut modules = Vec::new();
    Self::collect(root, &mut recipes, &mut modules);
    recipes.sort_by(|a, b| a.namepath.cmp(&b.namepath));
    modules.sort_by(|a, b| a.path.cmp(&b.path));
    Self {
      schema: SCHEMA,
      version: VERSION,
      root_default: root.default.as_ref().map(|r| r.recipe_path().to_string()),
      recipes,
      modules,
    }
  }

  /// Walk the justfile tree, flattening every recipe and recording every
  /// submodule. Each recipe self-reports its module path, so recursion only
  /// needs to reach module recipes, not thread scope through.
  fn collect(jf: &Justfile, recipes: &mut Vec<ModelRecipe>, modules: &mut Vec<ModelModule>) {
    let default_namepath = jf.default.as_ref().map(|r| r.recipe_path().to_string());

    for recipe in jf.recipes.values() {
      recipes.push(ModelRecipe::new(recipe, default_namepath.as_deref()));
    }

    for module in jf.modules.values() {
      modules.push(ModelModule {
        path: module.module_path.components.clone(),
        doc: module.doc.clone(),
        source: module.source.display().to_string(),
      });
      Self::collect(module, recipes, modules);
    }
  }
}

impl ModelRecipe {
  fn new(recipe: &Recipe, scope_default_namepath: Option<&str>) -> Self {
    let namepath = recipe.recipe_path().to_string();

    let groups = recipe
      .attributes
      .iter()
      .filter_map(|attribute| match attribute {
        Attribute::Group(name) => Some(name.cooked.clone()),
        _ => None,
      })
      .collect();

    // Attribute discriminant names (kebab-case). The set is ordered, so
    // repeated discriminants (e.g. two `[group(...)]`) are adjacent and
    // collapse under `dedup`.
    let mut attributes: Vec<String> = recipe
      .attributes
      .iter()
      .map(|attribute| <&'static str>::from(attribute).to_owned())
      .collect();
    attributes.dedup();

    let dependencies = recipe
      .dependencies
      .iter()
      .map(|dependency| ModelDependency {
        name: dependency.recipe.name().to_owned(),
        namepath: dependency.recipe.recipe_path().to_string(),
      })
      .collect();

    let parameters = recipe
      .parameters
      .iter()
      .map(|parameter| parameter.name.lexeme().to_owned())
      .collect();

    let is_default = scope_default_namepath == Some(namepath.as_str());

    Self {
      name: recipe.name().to_owned(),
      module: recipe.module_path().components.clone(),
      source: recipe.name.token.path.display().to_string(),
      line: recipe.name.token.line + 1,
      doc: recipe.doc.clone(),
      doc_prelude: recipe.doc_prelude.clone(),
      groups,
      attributes,
      private: recipe.private,
      is_default,
      has_body: !recipe.body.is_empty(),
      parameters,
      dependencies,
      namepath,
    }
  }
}
