use {
  super::*,
  std::sync::atomic::{AtomicUsize, Ordering},
};

/// One record on the `--events-fd` stream.
///
/// Wire format is specified in
/// `docs/rfcs/0002-just-events-fd-stream.md`. Each variant serializes
/// to one NDJSON record with a `type` discriminator and the variant's
/// fields flattened alongside.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Event<'a> {
  Plan {
    version: u32,
    recipe_count: usize,
  },
  RecipeStart {
    tp: usize,
    name: &'a str,
    namepath: &'a str,
    depth: u32,
    parent: Option<usize>,
    doc: Option<&'a str>,
    quiet: bool,
  },
  RecipeCommand {
    tp: usize,
    command: &'a str,
    line: usize,
  },
  Output {
    tp: usize,
    stream: OutputStream,
    format: OutputDataFormat,
    data: &'a str,
  },
  RecipeComplete {
    tp: usize,
    exit_code: Option<i32>,
    signal: Option<&'a str>,
    duration_ms: u64,
  },
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum OutputStream {
  Stdout,
  Stderr,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum OutputDataFormat {
  Utf8,
  Base64,
}

const SCHEMA_VERSION: u32 = 1;

/// Maximum bytes of UTF-8 text per `output` record `data` field.
/// crap RFC 0002 §5 asks producers to keep serialized record lines
/// under 64 KiB as buffer hygiene for the sink server; 8 KiB of
/// pre-escaping text stays under that even at worst-case JSON
/// escaping (6 output bytes per input byte).
pub(crate) const OUTPUT_CHUNK_BYTES: usize = 8192;

/// Split `data` into chunks of at most `max_bytes` bytes, each on a
/// char boundary. `max_bytes` must be at least 4 (the maximum UTF-8
/// sequence length) or a multi-byte char could stall progress.
pub(crate) fn chunk_str(mut data: &str, max_bytes: usize) -> impl Iterator<Item = &str> {
  std::iter::from_fn(move || {
    if data.is_empty() {
      return None;
    }
    let mut end = data.len().min(max_bytes);
    while !data.is_char_boundary(end) {
      end -= 1;
    }
    let (chunk, rest) = data.split_at(end);
    data = rest;
    Some(chunk)
  })
}

/// Ambient-attach state (crap RFC 0002). Present only when the sink
/// was reached via a `CRAP=2` environment offer — either inherited
/// (`CRAP_SINK`) or root-elected (we birthed the server).
struct AmbientAttach {
  /// `CRAP_PARENT`: harness node id our root recipes nest under.
  parent: Option<usize>,
  /// The sink's socket path, re-exported to recipe children so they
  /// connect themselves (crap RFC 0002 §5).
  sink_path: String,
}

/// Sink that serializes `Event` records to a writer as one JSON record
/// per line. When constructed with `EventSink::noop()`, all `emit`
/// calls are no-ops; this lets the normal code path call `emit`
/// unconditionally without paying allocation cost when neither
/// `--events-fd` nor a CRAP offer is active.
pub(crate) struct EventSink {
  /// Present iff the sink was activated by an ambient `CRAP=2`
  /// offer rather than the explicit `--events-fd` flag.
  ambient: Option<AmbientAttach>,
  /// Test-point counter. `next_tp` returns `base + n` values, one-
  /// based; the base is 0 for explicit `--events-fd` sinks (so tps
  /// stay 1..n per RFC 0002) and the sink server's granted base for
  /// ambient sinks (crap RFC 0002 §4). Shared across threads via
  /// atomic — parallel dep execution still gets distinct tp values.
  next_tp: AtomicUsize,
  writer: Option<Mutex<Box<dyn Write + Send>>>,
}

impl EventSink {
  /// Environment variables forming a crap RFC 0002 offer. An aware
  /// program either scopes them per child (`CRAP_PARENT`) or
  /// withdraws all of them for children whose stdout it consumes as
  /// data.
  pub(crate) const OFFER_VARS: [&'static str; 3] = ["CRAP", "CRAP_SINK", "CRAP_PARENT"];

  pub(crate) fn noop() -> Self {
    Self {
      ambient: None,
      next_tp: AtomicUsize::new(0),
      writer: None,
    }
  }

  pub(crate) fn from_writer<W: Write + Send + 'static>(writer: W) -> Self {
    Self {
      ambient: None,
      next_tp: AtomicUsize::new(0),
      writer: Some(Mutex::new(Box::new(writer))),
    }
  }

  /// Allocate the next test-point number. Returns base + a one-based
  /// counter; the first call returns base + 1.
  pub(crate) fn next_tp(&self) -> usize {
    self.next_tp.fetch_add(1, Ordering::Relaxed) + 1
  }

  /// Build an `EventSink` from a `Config`. If `events_fd` is unset,
  /// returns a no-op sink. If set, validates the fd is open and
  /// writable before wrapping it; an invalid fd yields an
  /// `io::Error` so the caller can construct an
  /// `Error::EventsFdInvalid` with the descriptor.
  pub(crate) fn from_config(config: &Config) -> io::Result<Self> {
    let Some(fd) = config.events_fd else {
      return Ok(Self::noop());
    };
    Self::validate_fd(fd)?;
    #[cfg(unix)]
    {
      use std::os::fd::FromRawFd;
      // SAFETY: `validate_fd` succeeded, so `fd` is an open writable
      // file descriptor. We assume sole ownership for the lifetime
      // of this `EventSink`; the wrapped `File` will close `fd` on
      // drop.
      let file = unsafe { File::from_raw_fd(fd) };
      Ok(Self::from_writer(file))
    }
    #[cfg(not(unix))]
    {
      let _ = fd;
      Err(io::Error::other(
        "--events-fd is only supported on Unix targets",
      ))
    }
  }

  /// Build an `EventSink` from an ambient crap RFC 0002 offer:
  /// connect to the inherited `CRAP_SINK`, or — as a root-capable
  /// node — birth a sink server and connect to it
  /// (`crap_attach::attach`). Per §3 of the RFC, every failure mode
  /// degrades silently to a noop sink; only the explicit
  /// `--events-fd` flag errors.
  pub(crate) fn from_ambient() -> Self {
    #[cfg(unix)]
    {
      match crap_attach::attach() {
        Some(attachment) => Self {
          ambient: Some(AmbientAttach {
            parent: attachment.parent,
            sink_path: attachment.sink_path,
          }),
          next_tp: AtomicUsize::new(attachment.base),
          writer: Some(Mutex::new(Box::new(attachment.stream))),
        },
        None => Self::noop(),
      }
    }
    #[cfg(not(unix))]
    {
      Self::noop()
    }
  }

  /// `CRAP_PARENT` of the ambient offer: the harness node id that
  /// top-level recipe nodes carry as `parent`.
  pub(crate) fn root_parent(&self) -> Option<usize> {
    self.ambient.as_ref().and_then(|ambient| ambient.parent)
  }

  /// The environment for scoping a recipe child into our tree (crap
  /// RFC 0002 §5): the inherited offer and sink address, with
  /// `CRAP_PARENT` set to the child's execution node so a crap-aware
  /// child connects itself and nests; anything else emits garbage,
  /// which the capture path wraps as `output` records. `None` when
  /// the sink is not ambient (explicit `--events-fd` has no socket
  /// for children to connect to; the child sees the inherited
  /// environment untouched).
  pub(crate) fn reoffer_env(&self, tp: usize) -> Option<Vec<(String, String)>> {
    let ambient = self.ambient.as_ref()?;
    Some(vec![
      ("CRAP".into(), "2".into()),
      ("CRAP_SINK".into(), ambient.sink_path.clone()),
      ("CRAP_PARENT".into(), tp.to_string()),
    ])
  }

  #[cfg(unix)]
  fn validate_fd(fd: i32) -> io::Result<()> {
    // `fcntl(fd, F_GETFL)` returns the file's status flags or -1
    // with errno set (EBADF for a closed fd, EINVAL for some
    // unsupported descriptor types). We use that to gate ownership
    // before wrapping the raw fd in a File.
    // SAFETY: `F_GETFL` reads descriptor state only; an invalid fd
    // yields -1 with errno set, handled below.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 {
      return Err(io::Error::last_os_error());
    }
    let access = flags & libc::O_ACCMODE;
    if access != libc::O_WRONLY && access != libc::O_RDWR {
      return Err(io::Error::other("file descriptor is not writable"));
    }
    Ok(())
  }

  #[cfg(not(unix))]
  fn validate_fd(_fd: i32) -> io::Result<()> {
    Err(io::Error::other(
      "--events-fd is only supported on Unix targets",
    ))
  }

  pub(crate) fn is_active(&self) -> bool {
    self.writer.is_some()
  }

  pub(crate) fn schema_version() -> u32 {
    SCHEMA_VERSION
  }

  /// Serialize `event` as one JSON line on the wire. Errors are
  /// silently dropped: an unwritable sink does not abort recipe
  /// execution (crap RFC 0002 §3 step 4 / eng RFC 0002 write-failure
  /// policy).
  ///
  /// Each record is serialized to a buffer and written with a single
  /// `write_all`; per-connection framing at the sink server keeps it
  /// whole regardless.
  pub(crate) fn emit(&self, event: &Event<'_>) {
    let Some(writer) = &self.writer else { return };
    let Ok(mut line) = serde_json::to_vec(event) else {
      return;
    };
    line.push(b'\n');
    let mut guard = match writer.lock() {
      Ok(guard) => guard,
      Err(_) => return,
    };
    let _ = guard.write_all(&line);
    let _ = guard.flush();
  }

  pub(crate) fn emit_plan(&self, recipe_count: usize) {
    // A producer attached under a harness node does not emit
    // stream-global records: a nested plan would re-arm the
    // consumer's progress accounting mid-stream.
    if self.root_parent().is_some() {
      return;
    }
    self.emit(&Event::Plan {
      version: SCHEMA_VERSION,
      recipe_count,
    });
  }
}

impl Default for EventSink {
  fn default() -> Self {
    Self::noop()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn capture<F: FnOnce(&EventSink)>(f: F) -> String {
    let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
    {
      let sink = EventSink::from_writer(BufHandle(Arc::clone(&buf)));
      f(&sink);
    }
    let bytes = Arc::try_unwrap(buf).unwrap().into_inner().unwrap();
    String::from_utf8(bytes).unwrap()
  }

  struct BufHandle(Arc<Mutex<Vec<u8>>>);

  impl Write for BufHandle {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
      self.0.lock().unwrap().extend_from_slice(data);
      Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
      Ok(())
    }
  }

  fn ambient_sink<W: Write + Send + 'static>(
    writer: W,
    base: usize,
    parent: Option<usize>,
  ) -> EventSink {
    EventSink {
      ambient: Some(AmbientAttach {
        parent,
        sink_path: "/run/crap/test.sock".into(),
      }),
      next_tp: AtomicUsize::new(base),
      writer: Some(Mutex::new(Box::new(writer))),
    }
  }

  #[test]
  fn noop_sink_emits_nothing() {
    let sink = EventSink::noop();
    sink.emit_plan(3);
    sink.emit(&Event::RecipeComplete {
      tp: 1,
      exit_code: Some(0),
      signal: None,
      duration_ms: 0,
    });
    assert!(!sink.is_active());
  }

  #[test]
  fn plan_record_shape() {
    let out = capture(|s| s.emit_plan(3));
    assert_eq!(
      out,
      "{\"type\":\"plan\",\"version\":1,\"recipe_count\":3}\n"
    );
  }

  #[test]
  fn recipe_start_with_doc_and_parent() {
    let out = capture(|s| {
      s.emit(&Event::RecipeStart {
        tp: 2,
        name: "bar",
        namepath: "bar",
        depth: 1,
        parent: Some(1),
        doc: Some("the bar recipe"),
        quiet: false,
      });
    });
    assert_eq!(
      out,
      "{\"type\":\"recipe_start\",\"tp\":2,\"name\":\"bar\",\
       \"namepath\":\"bar\",\"depth\":1,\"parent\":1,\
       \"doc\":\"the bar recipe\",\"quiet\":false}\n"
    );
  }

  #[test]
  fn recipe_start_top_level_no_doc() {
    let out = capture(|s| {
      s.emit(&Event::RecipeStart {
        tp: 1,
        name: "foo",
        namepath: "foo",
        depth: 0,
        parent: None,
        doc: None,
        quiet: true,
      });
    });
    assert_eq!(
      out,
      "{\"type\":\"recipe_start\",\"tp\":1,\"name\":\"foo\",\
       \"namepath\":\"foo\",\"depth\":0,\"parent\":null,\
       \"doc\":null,\"quiet\":true}\n"
    );
  }

  #[test]
  fn output_record_utf8() {
    let out = capture(|s| {
      s.emit(&Event::Output {
        tp: 1,
        stream: OutputStream::Stdout,
        format: OutputDataFormat::Utf8,
        data: "hello\n",
      });
    });
    assert_eq!(
      out,
      "{\"type\":\"output\",\"tp\":1,\"stream\":\"stdout\",\
       \"format\":\"utf8\",\"data\":\"hello\\n\"}\n"
    );
  }

  #[test]
  fn recipe_complete_signal() {
    let out = capture(|s| {
      s.emit(&Event::RecipeComplete {
        tp: 1,
        exit_code: None,
        signal: Some("SIGINT"),
        duration_ms: 312,
      });
    });
    assert_eq!(
      out,
      "{\"type\":\"recipe_complete\",\"tp\":1,\
       \"exit_code\":null,\"signal\":\"SIGINT\",\
       \"duration_ms\":312}\n"
    );
  }

  #[test]
  fn chunk_str_splits_on_char_boundaries() {
    let chunks: Vec<&str> = chunk_str("hello", 2).collect();
    assert_eq!(chunks, vec!["he", "ll", "o"]);
    // U+FFFD is three bytes; a 4-byte budget must not split it.
    let data = "a\u{fffd}b";
    let chunks: Vec<&str> = chunk_str(data, 4).collect();
    assert_eq!(chunks.concat(), data);
    for chunk in chunks {
      assert!(chunk.len() <= 4);
    }
    assert_eq!(chunk_str("", 4).count(), 0);
  }

  #[test]
  fn granted_base_offsets_tps() {
    let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
    let sink = ambient_sink(BufHandle(Arc::clone(&buf)), 1 << 32, Some(7));
    assert_eq!(sink.next_tp(), (1 << 32) + 1);
    assert_eq!(sink.next_tp(), (1 << 32) + 2);
  }

  #[test]
  fn nested_attachment_suppresses_plan() {
    let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
    {
      let sink = ambient_sink(BufHandle(Arc::clone(&buf)), 0, Some(7));
      sink.emit_plan(3);
    }
    let bytes = Arc::try_unwrap(buf).unwrap().into_inner().unwrap();
    assert!(bytes.is_empty(), "nested plan must be suppressed");
  }

  #[test]
  fn reoffer_scopes_child_under_node() {
    let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
    let sink = ambient_sink(BufHandle(buf), 0, None);
    let offer = sink.reoffer_env(42).unwrap();
    assert_eq!(
      offer,
      vec![
        ("CRAP".to_owned(), "2".to_owned()),
        ("CRAP_SINK".to_owned(), "/run/crap/test.sock".to_owned()),
        ("CRAP_PARENT".to_owned(), "42".to_owned()),
      ],
    );
  }

  #[test]
  fn explicit_sink_makes_no_reoffer() {
    let sink = EventSink::from_writer(Vec::new());
    assert!(sink.reoffer_env(1).is_none());
    assert!(EventSink::noop().reoffer_env(1).is_none());
  }
}
