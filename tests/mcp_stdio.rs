//! Regression tests for just-us#36: an `EAGAIN` on `just --mcp`'s stdin must
//! neither kill the server nor corrupt the request it lands in the middle of.
//!
//! In the field this was reached through `run_recipe`, which handed every
//! recipe subprocess a dup of the server's own stdin; anything in the recipe's
//! process tree that set `O_NONBLOCK` on it -- `ssh` is the classic offender
//! -- made the flag visible to the server's very next read, which then failed
//! and took the process down for the rest of the session. That path is closed
//! (`src/mcp_serve.rs` now spawns every child with `Stdio::null()`, which the
//! bats suite covers), but `O_NONBLOCK` lives on the open file *description*,
//! so it can still arrive from outside this process entirely. One `EAGAIN`
//! must never be fatal.
//!
//! Reproducing that needs a descriptor whose description these tests *share*
//! with the server. `Stdio::piped()` cannot do it: a pipe's two ends are two
//! separate descriptions, so setting the flag on the write end says nothing
//! about the child's read end. So the pipe is made here and its read end
//! duplicated -- one copy becomes the child's stdin, one stays behind -- which
//! is exactly the relationship a spawned recipe used to have with this stdin.

use super::*;

use {
  nix::fcntl::{FcntlArg, FdFlag, OFlag},
  std::{
    fs::File,
    io::{BufRead, BufReader},
    os::fd::OwnedFd,
    process::Child,
    sync::mpsc,
  },
};

/// Longest a single response may take before the server counts as wedged.
/// Generous: each one is an in-memory serialization of a fixed roster.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(10);

/// Long enough for the server to have gone round its request loop and be
/// waiting in `read` again, so the next thing a test does lands where it
/// intends to.
const SETTLE: Duration = Duration::from_millis(200);

/// `pipe(2)` hands back descriptors *without* `FD_CLOEXEC`, so without this the
/// spawned server would inherit a numbered copy of the pipe's write end -- and
/// closing this test's own copy would never produce the EOF that ends the
/// server's request loop. `pipe2(O_CLOEXEC)` would say it in one call but does
/// not exist on macOS; this is the portable spelling `src/signals.rs` uses too.
fn set_cloexec(fd: &OwnedFd) {
  let flags = nix::fcntl::fcntl(fd, FcntlArg::F_GETFD).expect("F_GETFD on a pipe end");

  nix::fcntl::fcntl(
    fd,
    FcntlArg::F_SETFD(FdFlag::from_bits_retain(flags) | FdFlag::FD_CLOEXEC),
  )
  .expect("F_SETFD FD_CLOEXEC on a pipe end");
}

fn set_nonblocking(fd: &OwnedFd) {
  let flags = nix::fcntl::fcntl(fd, FcntlArg::F_GETFL).expect("F_GETFL on the shared stdin");

  nix::fcntl::fcntl(
    fd,
    FcntlArg::F_SETFL(OFlag::from_bits_retain(flags) | OFlag::O_NONBLOCK),
  )
  .expect("F_SETFL O_NONBLOCK on the shared stdin");
}

/// A running `just --mcp` whose stdin this test can both write to and set
/// flags on, plus its responses arriving off-thread.
struct Server {
  child: Child,
  /// The pipe's write end: what requests go down.
  requests: File,
  /// This test's own copy of the READ end -- the same open file description
  /// the server is reading from, which is what makes `set_nonblocking`
  /// visible to the server at all.
  stdin: OwnedFd,
  responses: mpsc::Receiver<String>,
}

impl Server {
  fn start(dir: &Path) -> Self {
    let (read, write) = nix::unistd::pipe().unwrap();

    set_cloexec(&read);
    set_cloexec(&write);

    // `try_clone` dups with FD_CLOEXEC set, which `Command` then clears on the
    // one descriptor it installs as the child's fd 0 -- so the server gets
    // this and nothing else of the pipe.
    let child_stdin = read.try_clone().unwrap();

    let mut child = Command::new(JUST)
      .arg("--mcp")
      .current_dir(dir)
      .stdin(Stdio::from(child_stdin))
      .stdout(Stdio::piped())
      .stderr(Stdio::null())
      .spawn()
      .unwrap();

    // Responses are collected off-thread so a wedged server fails the test on
    // a deadline instead of hanging it: a `read_line` straight off the child's
    // stdout has no timeout of its own.
    let stdout = child.stdout.take().unwrap();
    let (sender, responses) = mpsc::channel();

    thread::spawn(move || {
      let mut reader = BufReader::new(stdout);

      loop {
        let mut line = String::new();

        match reader.read_line(&mut line) {
          Ok(0) | Err(_) => return,
          Ok(_) => {
            if sender.send(line).is_err() {
              return;
            }
          }
        }
      }
    });

    Self {
      child,
      requests: File::from(write),
      stdin: read,
      responses,
    }
  }

  fn send(&mut self, bytes: &[u8]) {
    self.requests.write_all(bytes).unwrap();
    self.requests.flush().unwrap();
  }

  fn tools_list(&mut self, id: u32) {
    self.send(format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/list"}}"#).as_bytes());
    self.send(b"\n");
  }

  #[track_caller]
  fn expect_response(&self, id: u32, when: &str) {
    let response = self
      .responses
      .recv_timeout(RESPONSE_TIMEOUT)
      .unwrap_or_else(|_| {
        panic!("no response to request {id} {when}: the server died or stopped answering")
      });

    assert!(
      response.contains(&format!(r#""id":{id}"#)),
      "response to request {id} {when} was not for that request: {response}"
    );
  }

  /// End of input ends the request loop, so the server should exit 0 promptly
  /// once its stdin is closed -- and in particular must not have been left
  /// spinning on a descriptor it failed to restore to blocking mode.
  #[track_caller]
  fn expect_clean_exit(mut self) {
    drop(self.requests);

    let deadline = Instant::now() + RESPONSE_TIMEOUT;

    while Instant::now() < deadline {
      if let Some(status) = self.child.try_wait().unwrap() {
        assert!(status.success(), "server exited with {status}");
        return;
      }

      thread::sleep(Duration::from_millis(50));
    }

    self.child.kill().ok();

    panic!("server did not exit after its stdin was closed");
  }
}

fn fixture() -> TempDir {
  let tmp = tempdir();
  fs::write(tmp.path().join("justfile"), "build:\n  @echo build\n").unwrap();
  tmp
}

#[test]
fn eagain_on_stdin_does_not_kill_the_mcp_server() {
  let tmp = fixture();
  let mut server = Server::start(tmp.path());

  // 1. The server answers normally, so anything that follows is a change this
  //    test caused rather than a server that never worked.
  server.tools_list(1);
  server.expect_response(1, "before O_NONBLOCK was set");

  // 2. Flag the shared description. By now the server is most likely already
  //    blocked in its next `read`, in which case that read completes normally
  //    and the one *after* it is the first to see EAGAIN; if it has not got
  //    that far, the read for request 2 sees it first. Either way the next
  //    read that finds no data pending fails with EAGAIN -- which, before the
  //    fix, exited the process.
  thread::sleep(SETTLE);
  set_nonblocking(&server.stdin);

  // 3. Two more requests, spaced so the second cannot already be sitting in
  //    the server's read buffer when it comes round the loop: the read that
  //    reaches for it has to be a real syscall on a non-blocking descriptor.
  server.tools_list(2);
  server.expect_response(2, "after O_NONBLOCK was set");

  thread::sleep(SETTLE);

  server.tools_list(3);
  server.expect_response(3, "after an EAGAIN was necessarily observed");

  server.expect_clean_exit();
}

/// Surviving the `EAGAIN` is not enough — the request it interrupts has to
/// survive it too.
///
/// Reading into a `String` via `BufRead::read_line` does not manage that: its
/// `io::append_to_string` guard commits the bytes it appended only when that
/// call's slice is valid UTF-8 on its own, so an `EAGAIN` landing between the
/// two bytes of a multi-byte character rolls the partial read back and drops
/// it. The retry then reassembles the request with a hole in the middle, which
/// fails to parse as JSON and is skipped — the caller waits forever for a
/// reply to an id the server silently discarded. `read_until` into a `Vec<u8>`
/// has no such guard, and UTF-8 is validated once on the finished line.
#[test]
fn eagain_inside_a_multibyte_character_does_not_corrupt_the_request() {
  let tmp = fixture();
  let mut server = Server::start(tmp.path());

  server.tools_list(1);
  server.expect_response(1, "before O_NONBLOCK was set");

  thread::sleep(SETTLE);
  set_nonblocking(&server.stdin);

  // `é` is two bytes (0xC3 0xA9), so splitting one byte past where it starts
  // leaves the server holding a truncated character when the pipe runs dry.
  let request = r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"show_recipe","arguments":{"recipe":"café"}}}"#;
  let split = request.find('é').unwrap() + 1;

  server.send(&request.as_bytes()[..split]);

  // Let the server consume that prefix and hit EAGAIN reaching for the rest,
  // rather than finding the whole line already waiting in the pipe.
  thread::sleep(SETTLE);

  server.send(&request.as_bytes()[split..]);
  server.send(b"\n");

  // Any well-formed reply to id 7 proves the line was reassembled intact — a
  // corrupted one parses as neither JSON nor a request and draws no reply at
  // all. (The reply itself is an `isError` for an unknown recipe; that the
  // recipe does not exist is beside the point.)
  server.expect_response(7, "after an EAGAIN split a multi-byte character");

  server.expect_clean_exit();
}
