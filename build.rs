fn version_from_env_file(contents: &str) -> Option<String> {
  contents.lines().find_map(|line| {
    line
      .trim()
      .trim_start_matches("export ")
      .strip_prefix("JUST_US_VERSION=")
      .map(str::to_owned)
  })
}

// eng-versioning(7) drift guard: version.env is the single source of
// truth; fail the build whenever Cargo.toml's package.version disagrees.
fn enforce_version_env() {
  println!("cargo::rerun-if-changed=version.env");
  println!("cargo::rerun-if-env-changed=JUST_US_VERSION");

  let authoritative = std::env::var("JUST_US_VERSION").ok().or_else(|| {
    std::fs::read_to_string("version.env")
      .ok()
      .as_deref()
      .and_then(version_from_env_file)
  });

  // Neither source exists (e.g. a published crate tarball): the guard
  // is a no-op and CARGO_PKG_VERSION stands on its own.
  let Some(want) = authoritative else { return };
  let have = std::env::var("CARGO_PKG_VERSION").unwrap();
  assert!(
    want == have,
    "Cargo.toml version ({have}) disagrees with version.env ({want}); run `just bump-version {want}`"
  );
}

fn main() {
  enforce_version_env();

  let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
  let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap();
  if os == "windows" {
    if env == "msvc" {
      println!("cargo::rustc-link-arg=/STACK:2097152");
    } else if env == "gnu" {
      println!("cargo::rustc-link-arg=-Wl,--stack,2097152");
    }
  }
}
