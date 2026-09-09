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

// Build-time pin for `run_recipe`'s async (ringmaster) mode
// (docs/features/0006 addendum). Set by flake.nix's `just` derivation to
// the store path of clown's `ringmaster` package; unset for a plain
// `cargo build`, in which case `mcp_serve.rs` falls back to a PATH
// lookup at runtime.
fn forward_ringmaster_bin() {
  println!("cargo::rerun-if-env-changed=RINGMASTER_BIN");
  if let Ok(path) = std::env::var("RINGMASTER_BIN") {
    println!("cargo::rustc-env=RINGMASTER_BIN={path}");
  }
}

fn main() {
  enforce_version_env();
  forward_ringmaster_bin();

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
