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

// eng-versioning(7) "Commit embedding (Rust)": resolve JUST_US_GIT_SHA and
// flow it into the crate via `cargo::rustc-env` so `--version` can show
// `<version>+<sha>` (src/arguments.rs's `version = concat!(...)`).
// Mirrors posh's cited reference implementation (posh-build's
// flow_git_sha/git_describe), inlined here rather than a shared crate
// since just-us is a single binary, not a workspace of many small ones.
//
// Resolution order:
//   1. `$JUST_US_GIT_SHA` in the build env -- the nix derivation sets this
//      from the flake's own git revision (`self.shortRev or
//      self.dirtyShortRev or "unknown"`, already carrying a "-dirty"
//      suffix when unclean), since the nix build sandbox has no `.git` of
//      its own to ask.
//   2. `git` in a dev checkout -- short sha plus "-dirty" for a modified
//      tree.
//   3. "unknown" -- no env var, no git (e.g. a bare source tarball).
fn flow_git_sha() {
  println!("cargo::rerun-if-env-changed=JUST_US_GIT_SHA");

  let git_sha = std::env::var("JUST_US_GIT_SHA")
    .ok()
    .filter(|sha| !sha.is_empty())
    .or_else(git_describe)
    .unwrap_or_else(|| "unknown".to_owned());

  println!("cargo::rustc-env=JUST_US_GIT_SHA={git_sha}");
}

// Dev-checkout git revision: `<short-sha>` plus `-dirty` when the working
// tree has uncommitted changes. `None` outside a git checkout (the nix
// build sets `$JUST_US_GIT_SHA` instead, so this never runs there).
fn git_describe() -> Option<String> {
  let rev = std::process::Command::new("git")
    .args(["rev-parse", "--short=12", "HEAD"])
    .output()
    .ok()?;

  if !rev.status.success() {
    return None;
  }

  let mut sha = String::from_utf8(rev.stdout).ok()?.trim().to_owned();

  if sha.is_empty() {
    return None;
  }

  if let Ok(status) = std::process::Command::new("git")
    .args(["status", "--porcelain"])
    .output()
  {
    if status.status.success() && !status.stdout.is_empty() {
      sha.push_str("-dirty");
    }
  }

  Some(sha)
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
  flow_git_sha();
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
