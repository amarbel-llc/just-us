use async_graphql::{Enum, SimpleObject};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize)]
pub struct JustfileDump {
  #[serde(default)]
  pub recipes: HashMap<String, RecipeRaw>,
}

#[derive(Deserialize)]
pub struct RecipeRaw {
  pub doc: Option<String>,
  #[serde(default)]
  pub quiet: bool,
  #[serde(default)]
  pub private: bool,
  #[serde(default)]
  pub parameters: Vec<ParameterRaw>,
  #[serde(default)]
  pub dependencies: Vec<DependencyRaw>,
}

#[derive(Deserialize)]
pub struct ParameterRaw {
  pub name: String,
  #[serde(default)]
  pub kind: String,
  pub default: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct DependencyRaw {
  pub recipe: String,
}

#[derive(SimpleObject, Clone)]
pub struct Recipe {
  pub name: String,
  pub doc: Option<String>,
  pub quiet: bool,
  pub private: bool,
  pub parameters: Vec<Parameter>,
  pub dependencies: Vec<Dependency>,
}

#[derive(SimpleObject, Clone)]
pub struct Parameter {
  pub name: String,
  pub kind: ParameterKind,
  pub default: Option<String>,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum ParameterKind {
  Singular,
  Plus,
  Star,
}

#[derive(SimpleObject, Clone)]
pub struct Dependency {
  pub recipe: String,
}

impl From<(String, RecipeRaw)> for Recipe {
  fn from((name, raw): (String, RecipeRaw)) -> Self {
    Self {
      name,
      doc: raw.doc,
      quiet: raw.quiet,
      private: raw.private,
      parameters: raw.parameters.into_iter().map(Parameter::from).collect(),
      dependencies: raw.dependencies.into_iter().map(Dependency::from).collect(),
    }
  }
}

impl From<ParameterRaw> for Parameter {
  fn from(raw: ParameterRaw) -> Self {
    let kind = match raw.kind.as_str() {
      "plus" => ParameterKind::Plus,
      "star" => ParameterKind::Star,
      _ => ParameterKind::Singular,
    };

    let default = raw.default.map(|v| match &v {
      serde_json::Value::String(s) => s.clone(),
      serde_json::Value::Array(arr) if arr.len() == 2 && arr[0] == "evaluate" => {
        format!("`{}`", arr[1].as_str().unwrap_or_default())
      }
      other => other.to_string(),
    });

    Self {
      name: raw.name,
      kind,
      default,
    }
  }
}

impl From<DependencyRaw> for Dependency {
  fn from(raw: DependencyRaw) -> Self {
    Self { recipe: raw.recipe }
  }
}
