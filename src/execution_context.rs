use super::*;

#[derive(Copy, Clone)]
pub(crate) struct ExecutionContext<'src: 'run, 'run> {
  pub(crate) config: &'run Config,
  pub(crate) dotenv: &'run BTreeMap<String, String>,
  /// Side channel for --events-fd NDJSON emission. Always present;
  /// a noop sink when --events-fd is unset. Allocation-cost-free in
  /// the noop case so the hot path can call `events.emit(...)`
  /// unconditionally.
  pub(crate) events: &'run EventSink,
  pub(crate) module: &'run Justfile<'src>,
  pub(crate) overrides: &'run HashMap<Number, String>,
  pub(crate) search: &'run Search,
  /// Test point number for the recipe currently executing in this
  /// context. Used to tag `recipe_command` and `output` events. The
  /// outer `run_recipe` assigns the value via `EventSink::next_tp`
  /// and constructs the context with it before any child events
  /// fire.
  pub(crate) tp: usize,
  /// Depth in the dependency tree: 0 for recipes invoked directly
  /// from the command line, 1 for their direct dependencies, and so
  /// on.
  pub(crate) depth: u32,
  /// `tp` of the recipe whose dependencies the current recipe is
  /// (i.e. the parent in the dep tree), or `None` for top-level
  /// invocations.
  pub(crate) parent_tp: Option<usize>,
}

impl<'src: 'run, 'run> ExecutionContext<'src, 'run> {
  pub(crate) fn tempdir<D>(&self, recipe: &Recipe<'src, D>) -> RunResult<'src, TempDir> {
    let mut builder = tempfile::Builder::new();

    builder.prefix(TEMPDIR_PREFIX);

    if let Some(tempdir) = &self.config.tempdir {
      builder.tempdir_in(self.search.working_directory.join(tempdir))
    } else {
      match &self.module.settings.tempdir {
        Some(tempdir) => builder.tempdir_in(self.search.working_directory.join(tempdir)),
        None => {
          if let Some(runtime_dir) = dirs::runtime_dir() {
            let path = runtime_dir.join(JUST_DIRECTORY);
            fs::create_dir_all(&path).map_err(|io_error| Error::RuntimeDirIo {
              io_error,
              path: path.clone(),
            })?;
            builder.tempdir_in(path)
          } else {
            builder.tempdir()
          }
        }
      }
    }
    .map_err(|error| Error::TempdirIo {
      recipe: recipe.name(),
      io_error: error,
    })
  }

  pub(crate) fn working_directory(&self) -> PathBuf {
    let base = if self.module.is_submodule() {
      &self.module.working_directory
    } else {
      &self.search.working_directory
    };

    if let Some(setting) = &self.module.settings.working_directory {
      base.join(setting)
    } else {
      base.into()
    }
  }
}
