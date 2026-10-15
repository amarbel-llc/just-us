//! Client side of the CRAP attach protocol (crap RFC 0002): detect a
//! `CRAP=2` offer, connect to the sink server named by `CRAP_SINK`, or
//! — as a root-capable node — birth one (a re-exec of this binary into
//! `crap_serve`) and connect to it. Terminal roots rendezvous on a
//! tty-keyed socket path; the kernel's bind atomicity is the leader
//! election.

use {
  super::*,
  std::{
    io::Read,
    os::fd::AsRawFd,
    os::unix::net::{UnixListener, UnixStream},
    time::Duration,
  },
};

/// A successful attachment: a connected sink, the id base granted to
/// this connection, and the metadata children need.
pub(crate) struct Attachment {
  /// Granted node-id base; ids are `base + n`, `n` from 1.
  pub(crate) base: usize,
  /// `CRAP_PARENT` from the offer: harness node our roots nest under.
  pub(crate) parent: Option<usize>,
  /// Sink address, re-exported to children alongside their
  /// `CRAP_PARENT`.
  pub(crate) sink_path: String,
  /// The connection itself; records are written here.
  pub(crate) stream: UnixStream,
}

enum AttachError {
  /// A live server answered and said no (deny grant, EOF before
  /// grant, malformed grant, unsupported format): degrade to dumb
  /// output. MUST NOT birth a surprise server.
  Denied,
  /// Nothing answered (missing socket, refused connection): per RFC
  /// 0002 §3, proceed as if `CRAP_SINK` were absent and re-root.
  NoServer,
}

/// CRAP major version this client implements.
const CRAP_VERSION: &str = "2";

const FORMAT: &str = "ndjson-crap/1";

/// Perform the RFC 0002 §3 attachment procedure. `None` means
/// unattached: behave as if the protocol did not exist. Every failure
/// mode degrades silently.
pub(crate) fn attach() -> Option<Attachment> {
  let version = env::var("CRAP").ok()?;
  if version.trim() != CRAP_VERSION {
    return None;
  }
  let parent = env::var("CRAP_PARENT")
    .ok()
    .and_then(|value| value.trim().parse().ok());
  if let Ok(path) = env::var("CRAP_SINK") {
    match connect(&path) {
      Ok((stream, base)) => {
        return Some(Attachment {
          base,
          parent,
          sink_path: path,
          stream,
        });
      }
      Err(AttachError::Denied) => return None,
      Err(AttachError::NoServer) => {}
    }
  }
  // Root-capable: no sink above (or a dead one). RFC 0002 §3 step 3.
  // Any inherited CRAP_PARENT named a node in the dead tree's id
  // namespace; in the fresh tree it would dangle (and wrongly mark us
  // nested), so roots start unparented.
  let dir = runtime_dir()?;
  // SAFETY: `isatty` has no preconditions.
  if unsafe { libc::isatty(libc::STDOUT_FILENO) } == 1 {
    terminal_attach(&dir)
  } else {
    tree_attach(&dir)
  }
}

/// Birth a private tree server (RFC 0002 §6.2) whose merged output is
/// our inherited stdout, then connect to it.
fn tree_attach(dir: &Path) -> Option<Attachment> {
  let path = dir.join(format!("tree-{}.sock", process::id()));
  let _ = fs::remove_file(&path);
  let listener = UnixListener::bind(&path).ok()?;
  let path = path.to_str()?.to_owned();
  birth(listener, &path, ServeMode::Tree).ok()?;
  let (stream, base) = connect(&path).ok()?;
  Some(Attachment {
    base,
    parent: None,
    sink_path: path,
    stream,
  })
}

/// Join or elect the terminal's shared server (RFC 0002 §6.3): each
/// tty has zero or one server, never more. The socket path is derived
/// from the controlling terminal's device number; binding it IS the
/// election, and losers connect as clients.
fn terminal_attach(dir: &Path) -> Option<Attachment> {
  let path = dir.join(tty_socket_name(stdout_rdev()?));
  let path = path.to_str()?.to_owned();
  for _ in 0..4 {
    match connect(&path) {
      Ok((stream, base)) => {
        return Some(Attachment {
          base,
          parent: None,
          sink_path: path,
          stream,
        });
      }
      Err(AttachError::Denied) => return None,
      Err(AttachError::NoServer) => {}
    }
    match UnixListener::bind(&path) {
      Ok(listener) => {
        birth(listener, &path, ServeMode::Terminal).ok()?;
        // Loop back to connect to the server we just elected.
      }
      Err(error) if error.kind() == io::ErrorKind::AddrInUse => {
        // Either we lost the election race (a live server bound it
        // between our connect and bind — loop back and connect), or
        // the path is stale debris from a crashed server. Disambiguate
        // and reclaim under a sidecar lock so two reclaimers cannot
        // both bind (RFC 0002 §6.3 step 3).
        if !reclaim_stale(&path) {
          continue;
        }
        let Ok(listener) = UnixListener::bind(&path) else {
          continue;
        };
        birth(listener, &path, ServeMode::Terminal).ok()?;
      }
      Err(_) => return None,
    }
  }
  None
}

/// With `<path>.lock` held, re-verify the socket is dead and unlink
/// it. Returns true when the caller should try binding.
fn reclaim_stale(path: &str) -> bool {
  let Ok(lock) = File::create(format!("{path}.lock")) else {
    return false;
  };
  // SAFETY: flock on a valid descriptor; LOCK_EX blocks until held.
  if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
    return false;
  }
  // Lock held (released on drop/close). Someone may have reclaimed
  // and rebound while we waited; a live server now means we should
  // just connect (return false → caller loops back to connect).
  if UnixStream::connect(path).is_ok() {
    return false;
  }
  let _ = fs::remove_file(path);
  true
}

/// Connect and perform the grant exchange (RFC 0002 §4): the server
/// answers each accept with exactly one JSON line, either a base
/// grant or a denial.
fn connect(path: &str) -> Result<(UnixStream, usize), AttachError> {
  let stream = UnixStream::connect(path).map_err(|error| match error.kind() {
    io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => AttachError::NoServer,
    _ => AttachError::Denied,
  })?;
  let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
  let mut line = Vec::new();
  let mut byte = [0u8; 1];
  loop {
    match (&stream).read(&mut byte) {
      Ok(1) if byte[0] == b'\n' => break,
      Ok(1) => {
        if line.len() >= 4096 {
          return Err(AttachError::Denied);
        }
        line.push(byte[0]);
      }
      // EOF before a grant is an answered denial, not absence.
      _ => return Err(AttachError::Denied),
    }
  }
  let base = parse_grant(&line)?;
  let _ = stream.set_read_timeout(None);
  Ok((stream, base))
}

/// Parse a grant line into the granted base. Anything other than a
/// well-formed CRAP-2 base grant for a format we can emit — including
/// an explicit `deny` — is a denial.
fn parse_grant(line: &[u8]) -> Result<usize, AttachError> {
  let value: serde_json::Value = serde_json::from_slice(line).map_err(|_| AttachError::Denied)?;
  if value.get("deny").is_some() {
    return Err(AttachError::Denied);
  }
  if value.get("version").and_then(serde_json::Value::as_u64) != Some(2)
    || value.get("format").and_then(serde_json::Value::as_str) != Some(FORMAT)
  {
    return Err(AttachError::Denied);
  }
  value
    .get("base")
    .and_then(serde_json::Value::as_u64)
    .and_then(|base| usize::try_from(base).ok())
    .ok_or(AttachError::Denied)
}

/// Spawn this binary re-exec'd into the serve loop (RFC 0002 §6.2,
/// "re-exec self"), handing it the pre-bound listener — so there is no
/// readiness race — plus, for tree servers, the read end of the lease
/// pipe whose write end we hold for the rest of our life.
fn birth(listener: UnixListener, path: &str, mode: ServeMode) -> io::Result<()> {
  let exe = env::current_exe()?;
  clear_cloexec(listener.as_raw_fd())?;
  let mut command = Command::new(exe);
  command
    .env(crap_serve::MARKER_VAR, "1")
    .env(crap_serve::LISTEN_FD_VAR, listener.as_raw_fd().to_string())
    .env(crap_serve::PATH_VAR, path)
    .env(crap_serve::MODE_VAR, mode.as_str())
    .env_remove("CRAP_SINK")
    .stdin(Stdio::null());
  let lease = if let ServeMode::Tree = mode {
    let mut fds = [0i32; 2];
    // SAFETY: plain pipe(2); fds are written on success only.
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
      return Err(io::Error::last_os_error());
    }
    // The write end must be ours alone — if the server (or any other
    // exec'd child) inherited a copy, the lease could never EOF and
    // the server would outlive the tree. Mark it close-on-exec BEFORE
    // any spawn; we keep it open for our whole life (deliberately
    // leaked), and its closure at our death is the server's signal to
    // drain and exit.
    // SAFETY: fcntl on a descriptor we just created.
    if unsafe { libc::fcntl(fds[1], libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
      let error = io::Error::last_os_error();
      // SAFETY: closing both ends we own.
      unsafe {
        libc::close(fds[0]);
        libc::close(fds[1]);
      }
      return Err(error);
    }
    command.env(crap_serve::LEASE_FD_VAR, fds[0].to_string());
    Some(fds)
  } else {
    None
  };
  let spawned = command.spawn();
  if let Some([read, write]) = lease {
    // SAFETY: we own both ends; the child inherited its own copy of
    // the read end across spawn.
    unsafe {
      libc::close(read);
      if spawned.is_err() {
        libc::close(write);
      }
    }
  }
  spawned.map(|_child| ())
  // The Child handle is dropped without wait: the server outlives us
  // by design (it drains after our exit) and is reparented and reaped
  // by init, so no zombie accrues.
}

#[derive(Clone, Copy)]
pub(crate) enum ServeMode {
  /// Shared per terminal; exits when its last client disconnects.
  Terminal,
  /// Private to one tree; exits on lease EOF.
  Tree,
}

impl ServeMode {
  pub(crate) fn as_str(self) -> &'static str {
    match self {
      Self::Terminal => "terminal",
      Self::Tree => "tree",
    }
  }
}

fn clear_cloexec(fd: i32) -> io::Result<()> {
  // SAFETY: fcntl on a valid owned descriptor.
  if unsafe { libc::fcntl(fd, libc::F_SETFD, 0) } == -1 {
    return Err(io::Error::last_os_error());
  }
  Ok(())
}

/// `$XDG_RUNTIME_DIR/crap/` (0700), falling back to `$TMPDIR`/`/tmp`.
fn runtime_dir() -> Option<PathBuf> {
  let base = env::var_os("XDG_RUNTIME_DIR")
    .map(PathBuf::from)
    .or_else(|| env::var_os("TMPDIR").map(PathBuf::from))
    .unwrap_or_else(|| PathBuf::from("/tmp"));
  let dir = base.join("crap");
  let mut builder = fs::DirBuilder::new();
  std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
  match builder.create(&dir) {
    Ok(()) => Some(dir),
    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Some(dir),
    Err(_) => None,
  }
}

/// Device number of the controlling terminal on stdout.
fn stdout_rdev() -> Option<u64> {
  // SAFETY: fstat on stdout with a zeroed out-param.
  let mut stat: libc::stat = unsafe { mem::zeroed() };
  // SAFETY: stdout is always a valid descriptor number and the
  // out-param is live for the call.
  if unsafe { libc::fstat(libc::STDOUT_FILENO, &raw mut stat) } != 0 {
    return None;
  }
  Some(stat.st_rdev)
}

/// RFC 0002 §6.3: the tty-keyed rendezvous name, derived from the
/// terminal's device number so every roofless root on one terminal
/// computes the same path.
fn tty_socket_name(rdev: u64) -> String {
  let (major, minor) = (libc::major(rdev), libc::minor(rdev));
  format!("tty-{major}.{minor}.sock")
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parse_grant_accepts_base_grant() {
    let line = br#"{"type":"crap","version":2,"base":4294967296,"format":"ndjson-crap/1"}"#;
    assert!(matches!(parse_grant(line), Ok(0x1_0000_0000)));
  }

  #[test]
  fn parse_grant_denies_deny_and_malformed() {
    for line in [
      br#"{"type":"crap","version":2,"deny":"scope"}"#.as_slice(),
      br#"{"type":"crap","version":3,"base":0,"format":"ndjson-crap/1"}"#.as_slice(),
      br#"{"type":"crap","version":2,"base":0,"format":"crap-pack/1"}"#.as_slice(),
      br#"{"type":"crap","version":2,"format":"ndjson-crap/1"}"#.as_slice(),
      b"not json".as_slice(),
    ] {
      assert!(matches!(parse_grant(line), Err(AttachError::Denied)));
    }
  }

  #[test]
  fn tty_socket_name_is_deterministic() {
    assert_eq!(tty_socket_name(0x8803), tty_socket_name(0x8803));
    assert_ne!(tty_socket_name(0x8803), tty_socket_name(0x8804));
  }
}
