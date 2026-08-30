//! Integration tests for `--dump --dump-format model`, the fork's normalized
//! recipe model (`src/recipe_model.rs`, `docs/features/0003-recipe-model.md`).
//!
//! The shared fixture is a root justfile that imports a submodule, so every
//! test exercises the two structural wins the model exists for: a module
//! recipe surfaces with its BARE name plus a separate module path
//! (conformist#85), and module recipes appear in the one FLAT `recipes` list
//! so a consumer cannot miss them by failing to recurse (conformist#89).

use super::*;

#[derive(Debug, Deserialize)]
struct Model {
  schema: String,
  version: u32,
  source: String,
  root_default: Option<String>,
  recipes: Vec<ModelRecipe>,
  modules: Vec<ModelModule>,
}

#[derive(Debug, Deserialize)]
struct ModelRecipe {
  name: String,
  namepath: String,
  module: Vec<String>,
  source: String,
  line: usize,
  doc: Option<String>,
  doc_prelude: Vec<String>,
  groups: Vec<String>,
  attributes: Vec<String>,
  private: bool,
  is_default: bool,
  has_body: bool,
  parameters: Vec<String>,
  dependencies: Vec<ModelDependency>,
}

#[derive(Debug, Deserialize)]
struct ModelDependency {
  name: String,
  namepath: String,
}

#[derive(Debug, Deserialize)]
struct ModelModule {
  path: Vec<String>,
  doc: Option<String>,
  source: String,
}

impl Model {
  #[track_caller]
  fn recipe(&self, namepath: &str) -> &ModelRecipe {
    self
      .recipes
      .iter()
      .find(|recipe| recipe.namepath == namepath)
      .unwrap_or_else(|| panic!("no recipe with namepath `{namepath}`"))
  }
}

/// Root justfile: a default aggregate, a leaf with an orphaned doc prelude, a
/// grouped leaf that depends on it, a private helper, and a `mod` import.
const ROOT: &str = "
  # exploration recipes
  mod explore 'zz-explore/justfile'

  # the default aggregate
  # run build then test
  default: build test

  # Build the release binary and
  # strip it
  build:
      @echo building

  # run the tests
  [group('test')]
  test: build
      @echo testing

  _helper:
      @echo helper
";

/// The imported submodule: a default recipe that itself carries an orphaned
/// doc prelude, and a second recipe that depends on the first (proving
/// dependency namepaths resolve WITH the module qualifier).
const EXPLORE: &str = "
  # poke at internals and
  # see what happens
  debug-foo:
      @echo foo

  # a clean explorer
  explore-bar: debug-foo
      @echo bar
";

#[track_caller]
fn model() -> Model {
  let stdout = Test::new()
    .justfile(ROOT)
    .write("zz-explore/justfile", unindent(EXPLORE))
    .args(["--dump", "--dump-format", "model"])
    .stdout_regex(".*")
    .success()
    .stdout;

  serde_json::from_str(&stdout)
    .unwrap_or_else(|error| panic!("model output was not valid JSON: {error}\n{stdout}"))
}

#[test]
fn schema_and_root_default() {
  let model = model();
  assert_eq!(model.schema, "just-us.recipe-model");
  assert_eq!(model.version, 1);
  assert_eq!(model.root_default.as_deref(), Some("default"));
  // The root anchor: the root justfile's own path.
  assert!(model.source.ends_with("justfile"));
  assert!(!model.source.ends_with("zz-explore/justfile"));
}

/// conformist#89: the flat list carries every recipe from the root AND the
/// module, in a stable namepath-sorted order — a consumer cannot miss module
/// recipes by failing to recurse.
#[test]
fn recipes_are_flat_across_modules() {
  let model = model();
  let namepaths: Vec<&str> = model.recipes.iter().map(|r| r.namepath.as_str()).collect();
  assert_eq!(
    namepaths,
    [
      "_helper",
      "build",
      "default",
      "explore::debug-foo",
      "explore::explore-bar",
      "test",
    ],
  );
}

/// conformist#85: a module recipe carries its BARE name and its module path
/// separately, so verb extraction never has to strip a `mod::` qualifier.
#[test]
fn module_recipe_has_bare_name_and_module_path() {
  let model = model();
  let debug_foo = model.recipe("explore::debug-foo");
  assert_eq!(debug_foo.name, "debug-foo");
  assert_eq!(debug_foo.module, ["explore"]);

  let build = model.recipe("build");
  assert_eq!(build.name, "build");
  assert!(build.module.is_empty());
}

/// The raw dump drops the `mod::` qualifier from dependency entries; the model
/// carries the resolved namepath alongside the bare name.
#[test]
fn dependencies_carry_resolved_namepaths() {
  let model = model();

  let explore_bar = model.recipe("explore::explore-bar");
  assert_eq!(explore_bar.dependencies.len(), 1);
  assert_eq!(explore_bar.dependencies[0].name, "debug-foo");
  assert_eq!(explore_bar.dependencies[0].namepath, "explore::debug-foo");

  let default = model.recipe("default");
  let dep_namepaths: Vec<&str> = default
    .dependencies
    .iter()
    .map(|d| d.namepath.as_str())
    .collect();
  assert_eq!(dep_namepaths, ["build", "test"]);
}

/// `doc_prelude` is carried for recipes in the root AND in modules (module
/// RECIPES are still recipes; only the module DECLARATION's prelude is
/// discarded — tracked as just-us#21). It is always present, `[]` when empty.
#[test]
fn doc_prelude_is_carried_for_root_and_module_recipes() {
  let model = model();

  let build = model.recipe("build");
  assert_eq!(build.doc.as_deref(), Some("strip it"));
  assert_eq!(build.doc_prelude, ["Build the release binary and"]);

  let debug_foo = model.recipe("explore::debug-foo");
  assert_eq!(debug_foo.doc.as_deref(), Some("see what happens"));
  assert_eq!(debug_foo.doc_prelude, ["poke at internals and"]);

  // A recipe with a single clean doc line strands nothing.
  assert!(model.recipe("test").doc_prelude.is_empty());
}

#[test]
fn groups_and_attributes() {
  let model = model();

  let test = model.recipe("test");
  assert_eq!(test.groups, ["test"]);
  assert_eq!(test.attributes, ["group"]);

  let build = model.recipe("build");
  assert!(build.groups.is_empty());
  assert!(build.attributes.is_empty());
}

#[test]
fn private_and_body_signals() {
  let model = model();

  assert!(model.recipe("_helper").private);
  assert!(!model.recipe("build").private);

  // A body-less aggregate vs a leaf with a body — the raw taxonomy signal.
  assert!(!model.recipe("default").has_body);
  assert!(model.recipe("build").has_body);
}

/// `is_default` is per-scope: the root's default AND each module's own default
/// both read true. `root_default` is the unambiguous root one.
#[test]
fn is_default_is_per_scope() {
  let model = model();
  assert!(model.recipe("default").is_default);
  assert!(model.recipe("explore::debug-foo").is_default);
  assert!(!model.recipe("build").is_default);
  assert_eq!(model.root_default.as_deref(), Some("default"));
}

/// Every recipe points at the file it is defined in; module recipes point at
/// the module file, not the root justfile.
#[test]
fn source_points_at_defining_file() {
  let model = model();

  assert!(model.recipe("build").source.ends_with("justfile"));
  assert!(
    model
      .recipe("explore::debug-foo")
      .source
      .ends_with("zz-explore/justfile")
  );
  // 1-based line of the recipe name; the fixture never puts a recipe on line 0.
  assert!(model.recipe("build").line >= 1);
}

#[test]
fn modules_are_listed_as_data() {
  let model = model();
  assert_eq!(model.modules.len(), 1);
  let module = &model.modules[0];
  assert_eq!(module.path, ["explore"]);
  assert_eq!(module.doc.as_deref(), Some("exploration recipes"));
  assert!(module.source.ends_with("zz-explore/justfile"));
}
