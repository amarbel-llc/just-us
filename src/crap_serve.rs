//! The CRAP sink server (crap RFC 0002 §6): birthed by a root via
//! re-exec of this binary, it owns one unix socket, answers each
//! accept with a grant carrying a disjoint id base, and splices
//! complete record lines from all connections to its merged output in
//! arrival order. It never parses, reorders, or rewrites records.
//! Tree servers exit on lease EOF (the spawner died); terminal servers
//! exit when their last client disconnects.

use {
  super::*,
  std::{
    io::Read,
    os::fd::FromRawFd,
    os::unix::net::{UnixListener, UnixStream},
    time::Duration,
  },
};

pub(crate) const MARKER_VAR: &str = "JUST_US_INTERNAL_CRAP_SERVE";
pub(crate) const LISTEN_FD_VAR: &str = "JUST_US_INTERNAL_CRAP_SERVE_LISTEN_FD";
pub(crate) const LEASE_FD_VAR: &str = "JUST_US_INTERNAL_CRAP_SERVE_LEASE_FD";
pub(crate) const PATH_VAR: &str = "JUST_US_INTERNAL_CRAP_SERVE_PATH";
pub(crate) const MODE_VAR: &str = "JUST_US_INTERNAL_CRAP_SERVE_MODE";

/// Per-connection line buffers are bounded; a line that exceeds the
/// cap without a newline drops its connection (RFC 0002 §6.1 requires
/// support to at least 64 KiB; producers SHOULD chunk below that).
const MAX_LINE: usize = 1 << 20;

/// How long a terminal server lingers after its last client
/// disconnects, so back-to-back commands reuse it.
const TERMINAL_LINGER: Duration = Duration::from_millis(1000);

/// How long an elected terminal server waits for its first client.
const TERMINAL_FIRST_CLIENT: Duration = Duration::from_secs(10);

/// How long a tree server drains open connections after lease EOF.
const DRAIN_GRACE: Duration = Duration::from_secs(2);

/// Disjoint base per accepted connection: the k-th connection gets
/// k·2^32, so the first client (normally the root itself) emits plain
/// 1, 2, 3, … (RFC 0002 §4).
const BASE_STRIDE: u64 = 1 << 32;

pub(crate) fn enabled() -> bool {
  env::var_os(MARKER_VAR).is_some()
}

struct Connection {
  buf: Vec<u8>,
  stream: UnixStream,
}

/// Run the serve loop on the inherited descriptors. Returns the
/// process exit code; the caller (`run()`) exits with it immediately —
/// no Config parsing, no justfile loading.
pub(crate) fn serve() -> i32 {
  // A terminal server shares the foreground process group and
  // session; ignore SIGINT (a ^C aimed at the tree) and SIGHUP (pty
  // teardown) so we always reach the drain/flush/unlink path — after
  // either signal the clients die, their disconnects empty the
  // refcount, and we exit cleanly. SIGTERM stays deliverable.
  // SAFETY: SIG_IGN installation has no preconditions.
  unsafe {
    libc::signal(libc::SIGINT, libc::SIG_IGN);
    libc::signal(libc::SIGHUP, libc::SIG_IGN);
  }
  let Some(listener) = fd_from_env(LISTEN_FD_VAR) else {
    return EXIT_FAILURE;
  };
  // SAFETY: the fd numbers arrive from our spawner, which cleared
  // close-on-exec on descriptors it owns; we take sole ownership.
  let listener = unsafe { UnixListener::from_raw_fd(listener) };
  // SAFETY: same provenance as the listener — an inherited descriptor
  // our spawner created for us; we take sole ownership.
  let lease = fd_from_env(LEASE_FD_VAR).map(|fd| unsafe { File::from_raw_fd(fd) });
  let path = env::var(PATH_VAR).ok();
  let terminal = env::var(MODE_VAR).as_deref() == Ok("terminal");
  let output = open_output(terminal);
  let code = run_server(&listener, lease, output, terminal);
  if let Some(path) = path {
    let _ = fs::remove_file(path);
  }
  code
}

fn fd_from_env(var: &str) -> Option<i32> {
  env::var(var).ok()?.trim().parse().ok()
}

/// The merged stream's destination. Tree servers splice to inherited
/// stdout (a pipe or file the root's caller arranged). Terminal
/// servers land on a human's tty, so they SHOULD present: pipe into
/// `crap-present` when available, falling back to a raw splice (RFC
/// 0002 §6.3 permits it as a last resort).
fn open_output(terminal: bool) -> Box<dyn Write> {
  if terminal {
    if let Ok(child) = Command::new("crap-present").stdin(Stdio::piped()).spawn() {
      if let Some(stdin) = child.stdin {
        return Box::new(stdin);
      }
    }
  }
  Box::new(io::stdout())
}

/// The loop proper, factored over arbitrary output for testability.
fn run_server(
  listener: &UnixListener,
  lease: Option<File>,
  mut output: Box<dyn Write>,
  terminal: bool,
) -> i32 {
  use std::os::fd::AsRawFd;
  if listener.set_nonblocking(true).is_err() {
    return EXIT_FAILURE;
  }
  let mut connections: Vec<Connection> = Vec::new();
  let mut granted: u64 = 0;
  let mut lease_eof = lease.is_none();
  let mut accepting = true;
  let mut deadline: Option<Instant> = terminal.then(|| Instant::now() + TERMINAL_FIRST_CLIENT);
  loop {
    let mut fds: Vec<libc::pollfd> = Vec::with_capacity(connections.len() + 2);
    let listener_index = accepting.then(|| {
      fds.push(libc::pollfd {
        fd: listener.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
      });
      fds.len() - 1
    });
    let lease_index = lease.as_ref().filter(|_| !lease_eof).map(|lease| {
      fds.push(libc::pollfd {
        fd: lease.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
      });
      fds.len() - 1
    });
    let connection_offset = fds.len();
    for connection in &connections {
      fds.push(libc::pollfd {
        fd: connection.stream.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
      });
    }
    // SAFETY: fds points at a live, correctly-sized pollfd slice.
    let rc = unsafe {
      libc::poll(
        fds.as_mut_ptr(),
        fds.len() as libc::nfds_t,
        250, // wake regularly to evaluate deadlines
      )
    };
    if rc < 0 && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
      return EXIT_FAILURE;
    }
    if let Some(index) = listener_index {
      if fds[index].revents != 0 {
        while let Ok((stream, _)) = listener.accept() {
          let grant = format!(
            "{{\"type\":\"crap\",\"version\":2,\"base\":{},\"format\":\"ndjson-crap/1\"}}\n",
            granted * BASE_STRIDE,
          );
          granted += 1;
          deadline = None;
          let mut stream = stream;
          if stream.write_all(grant.as_bytes()).is_ok() && stream.set_nonblocking(true).is_ok() {
            connections.push(Connection {
              buf: Vec::new(),
              stream,
            });
          }
        }
      }
    }
    if let Some(index) = lease_index {
      if fds[index].revents != 0 {
        let mut scratch = [0u8; 64];
        if let Some(lease) = &lease {
          if matches!((&*lease).read(&mut scratch), Ok(0) | Err(_)) {
            lease_eof = true;
            accepting = false;
            deadline = Some(Instant::now() + DRAIN_GRACE);
          }
        }
      }
    }
    let mut index = 0;
    while index < connections.len() {
      let ready = fds
        .get(connection_offset + index)
        .is_some_and(|fd| fd.revents != 0);
      if ready && !pump(&mut connections[index], &mut output) {
        connections.swap_remove(index);
        if terminal && connections.is_empty() {
          deadline = Some(Instant::now() + TERMINAL_LINGER);
        }
      } else {
        index += 1;
      }
    }
    let _ = output.flush();
    if lease_eof && !terminal {
      let drained = connections.is_empty() || deadline.is_some_and(|d| Instant::now() >= d);
      if drained {
        return EXIT_SUCCESS;
      }
    }
    if terminal && connections.is_empty() {
      if let Some(deadline) = deadline {
        if Instant::now() >= deadline {
          return EXIT_SUCCESS;
        }
      }
    }
  }
}

/// Drain readable bytes from one connection, splicing every complete
/// line to the output atomically (we are the only writer, so a single
/// `write_all` per line suffices). Returns false when the connection
/// is finished and should be dropped.
fn pump(connection: &mut Connection, output: &mut Box<dyn Write>) -> bool {
  let mut chunk = [0u8; 4096];
  loop {
    match (&connection.stream).read(&mut chunk) {
      Ok(0) => {
        // Tolerate a final unterminated fragment: better a newline
        // appended than records silently dropped.
        if !connection.buf.is_empty() {
          let _ = output.write_all(&connection.buf);
          let _ = output.write_all(b"\n");
        }
        return false;
      }
      Ok(n) => {
        connection.buf.extend_from_slice(&chunk[..n]);
        while let Some(end) = connection.buf.iter().position(|&byte| byte == b'\n') {
          let _ = output.write_all(&connection.buf[..=end]);
          connection.buf.drain(..=end);
        }
        if connection.buf.len() > MAX_LINE {
          return false;
        }
      }
      Err(error) if error.kind() == io::ErrorKind::WouldBlock => return true,
      Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
      Err(_) => return false,
    }
  }
}

#[cfg(test)]
mod tests {
  use {super::*, std::os::unix::net::UnixStream};

  struct SharedBuf(Arc<Mutex<Vec<u8>>>);

  impl Write for SharedBuf {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
      self.0.lock().unwrap().extend_from_slice(data);
      Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
      Ok(())
    }
  }

  /// End-to-end over a real socket: two clients get disjoint bases in
  /// accept order, their lines splice whole, and lease EOF ends the
  /// server.
  #[test]
  fn grants_bases_and_splices_lines() {
    let dir = testing::tempdir();
    let path = dir.path().join("sink.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let mut lease_fds = [0i32; 2];
    // SAFETY: plain pipe(2).
    assert_eq!(unsafe { libc::pipe(lease_fds.as_mut_ptr()) }, 0);
    // SAFETY: we own both fresh descriptors.
    let lease_read = unsafe { File::from_raw_fd(lease_fds[0]) };
    let buf = Arc::new(Mutex::new(Vec::new()));
    let output = Box::new(SharedBuf(Arc::clone(&buf)));
    let server = thread::spawn(move || run_server(&listener, Some(lease_read), output, false));

    let read_grant = |stream: &UnixStream| -> serde_json::Value {
      let mut line = Vec::new();
      let mut byte = [0u8; 1];
      while (&*stream).read(&mut byte).unwrap() == 1 && byte[0] != b'\n' {
        line.push(byte[0]);
      }
      serde_json::from_slice(&line).unwrap()
    };

    let first = UnixStream::connect(&path).unwrap();
    let first_grant = read_grant(&first);
    assert_eq!(first_grant["base"], 0);
    assert_eq!(first_grant["format"], "ndjson-crap/1");

    let second = UnixStream::connect(&path).unwrap();
    assert_eq!(read_grant(&second)["base"], u64::from(u32::MAX) + 1);

    (&first)
      .write_all(b"{\"type\":\"plan\",\"count\":1}\n")
      .unwrap();
    (&second)
      .write_all(b"{\"type\":\"node_start\",\"tp\":4294967297}\n")
      .unwrap();
    drop(first);
    drop(second);
    // Dropping the write end EOFs the lease; the server drains the
    // already-buffered lines, flushes, and exits 0.
    // SAFETY: we own the write end and have not closed it elsewhere.
    unsafe { libc::close(lease_fds[1]) };
    assert_eq!(server.join().unwrap(), EXIT_SUCCESS);

    let merged = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
    let mut lines: Vec<&str> = merged.lines().collect();
    lines.sort_unstable();
    assert_eq!(
      lines,
      vec![
        "{\"type\":\"node_start\",\"tp\":4294967297}",
        "{\"type\":\"plan\",\"count\":1}",
      ],
    );
  }
}
