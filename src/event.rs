use {
  super::*,
  std::sync::atomic::{AtomicBool, AtomicUsize, Ordering},
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
  /// The crap RFC 0002 *hello*: announces attachment, the negotiated
  /// format, and this producer's position in the harness's node tree.
  /// Emitted once, before any other record, when the sink was
  /// activated by an ambient `CRAP=2` offer (never under explicit
  /// `--events-fd`, whose RFC 0002 contract pins `plan` first).
  Crap {
    version: u32,
    ndjson: u32,
    format: &'a str,
    producer: &'a str,
    parent: Option<usize>,
  },
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

/// CRAP major version this producer can attach to (crap RFC 0002).
const CRAP_VERSION: &str = "2";

/// The one format token this producer emits. Negotiation selects the
/// first token in `CRAP_ACCEPT` (harness preference order) we support.
const NDJSON_CRAP_1: &str = "ndjson-crap/1";

/// Maximum bytes of UTF-8 text per `output` record `data` field.
/// crap RFC 0002 §7 requires each serialized record to fit one
/// `write(2)` of at most `PIPE_BUF` (4096) bytes on a shared channel;
/// 1024 pre-escaping bytes leave headroom for JSON escaping and the
/// record envelope.
pub(crate) const OUTPUT_CHUNK_BYTES: usize = 1024;

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

/// Random node-id base for ambient attachment, per crap RFC 0002 §7:
/// a shared channel may carry several producers, so ids are drawn
/// from a uniformly random base in `[2^40, 2^48)` (JSON-exact, never
/// colliding with an explicit-flag producer's small monotonic ids).
/// `RandomState` is seeded from OS randomness per process, which is
/// all the uniformity the collision argument needs.
fn random_tp_base() -> usize {
  use std::hash::{BuildHasher, Hasher, RandomState};
  let mut hasher = RandomState::new().build_hasher();
  hasher.write_u32(std::process::id());
  let r = hasher.finish();
  if usize::BITS >= 64 {
    usize::try_from((1u64 << 40) + (r % ((1u64 << 48) - (1u64 << 40)))).unwrap_or(usize::MAX >> 1)
  } else {
    // 32-bit targets: keep ids large but in range.
    usize::try_from((1u64 << 28) + (r % (1u64 << 30))).unwrap_or(usize::MAX >> 1)
  }
}

/// Select the first format token in a `CRAP_ACCEPT` list we support.
/// Token grammar (crap RFC 0002 §4): `name/major[;param=value]*`,
/// comma-separated, harness preference order. Parameters are ignored.
fn negotiate(accept: &str) -> Option<&'static str> {
  for token in accept.split(',') {
    let name_version = token.split(';').next().unwrap_or("").trim();
    if name_version == NDJSON_CRAP_1 {
      return Some(NDJSON_CRAP_1);
    }
  }
  None
}

/// Ambient-attach state (crap RFC 0002). Present only when the sink
/// was activated by a `CRAP=2` environment offer.
struct AmbientAttach {
  /// `CRAP_DEPTH`: depth to assign our root recipes.
  depth_base: u32,
  /// The hello is written lazily before the first record, so silent
  /// invocations (`--list`, …) stay silent on the channel.
  hello_sent: AtomicBool,
  /// `CRAP_PARENT`: harness node id our root recipes nest under.
  parent: Option<usize>,
}

/// Sink that serializes `Event` records to a writer as one JSON record
/// per line. When constructed with `EventSink::noop()`, all `emit`
/// calls are no-ops; this lets the normal code path call `emit`
/// unconditionally without paying allocation cost when `--events-fd`
/// is not set.
pub(crate) struct EventSink {
  /// Present iff the sink was activated by an ambient `CRAP=2`
  /// offer rather than the explicit `--events-fd` flag.
  ambient: Option<AmbientAttach>,
  /// Test-point counter. `next_tp` returns `base + n` values, one-
  /// based; the base is 0 for explicit `--events-fd` sinks (so tps
  /// stay 1..n per RFC 0002) and random for ambient sinks (crap
  /// RFC 0002 §7). Shared across threads via atomic — parallel dep
  /// execution still gets distinct tp values.
  next_tp: AtomicUsize,
  /// A `dup(2)` of the channel that recipe children can inherit
  /// (`FD_CLOEXEC` clear), named in the `CRAP_FD` re-offer. `None`
  /// when the sink is inactive or the dup failed — then no re-offer
  /// is made.
  reoffer_fd: Option<i32>,
  writer: Option<Mutex<Box<dyn Write + Send>>>,
}

impl EventSink {
  /// Environment variables forming a crap RFC 0002 offer. An aware
  /// program either re-offers (rewriting all of them) or withdraws
  /// (removing all of them) for each child it executes.
  pub(crate) const OFFER_VARS: [&'static str; 5] = [
    "CRAP",
    "CRAP_FD",
    "CRAP_ACCEPT",
    "CRAP_PARENT",
    "CRAP_DEPTH",
  ];

  pub(crate) fn noop() -> Self {
    Self {
      writer: None,
      next_tp: AtomicUsize::new(0),
      reoffer_fd: None,
      ambient: None,
    }
  }

  pub(crate) fn from_writer<W: Write + Send + 'static>(writer: W) -> Self {
    Self {
      writer: Some(Mutex::new(Box::new(writer))),
      next_tp: AtomicUsize::new(0),
      reoffer_fd: None,
      ambient: None,
    }
  }

  /// Allocate the next test-point number. Returns a one-based
  /// integer; the first call returns 1.
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
      // Dup the channel before taking ownership, so recipe children
      // can be handed a descriptor that reaches it even when their
      // own stdio is repointed at capture pipes (crap RFC 0002
      // §6.1). dup'd descriptors have FD_CLOEXEC clear, so children
      // inherit it. A failed dup just disables the re-offer.
      // SAFETY: `dup` has no memory-safety preconditions; `fd` was
      // validated above and a failure returns -1, handled below.
      let reoffer = unsafe { libc::dup(fd) };
      // SAFETY: `validate_fd` succeeded, so `fd` is an open writable
      // file descriptor. We assume sole ownership for the lifetime
      // of this `EventSink`; the wrapped `File` will close `fd` on
      // drop.
      let file = unsafe { File::from_raw_fd(fd) };
      Ok(Self {
        writer: Some(Mutex::new(Box::new(file))),
        next_tp: AtomicUsize::new(0),
        reoffer_fd: (reoffer >= 0).then_some(reoffer),
        ambient: None,
      })
    }
    #[cfg(not(unix))]
    {
      let _ = fd;
      Err(io::Error::other(
        "--events-fd is only supported on Unix targets",
      ))
    }
  }

  /// Build an `EventSink` from an ambient crap RFC 0002 offer in the
  /// environment (`CRAP=2` plus optional `CRAP_FD`/`CRAP_ACCEPT`/
  /// `CRAP_PARENT`/`CRAP_DEPTH`). Per §3 of the RFC, every failure
  /// mode — absent or unsupported version, unsupported format list,
  /// malformed or dead descriptor — degrades silently to a noop
  /// sink; only the explicit `--events-fd` flag errors.
  pub(crate) fn from_ambient() -> Self {
    #[cfg(unix)]
    {
      use std::os::fd::FromRawFd;

      fn parse_var<T: FromStr>(name: &str) -> Option<T> {
        env::var(name).ok().and_then(|v| v.trim().parse().ok())
      }

      let Ok(version) = env::var("CRAP") else {
        return Self::noop();
      };
      if version.trim() != CRAP_VERSION {
        return Self::noop();
      }
      let accept = env::var("CRAP_ACCEPT").unwrap_or_else(|_| NDJSON_CRAP_1.into());
      if negotiate(&accept).is_none() {
        return Self::noop();
      }
      let fd = match env::var("CRAP_FD") {
        Ok(value) => match value.trim().parse::<i32>() {
          Ok(fd) => fd,
          Err(_) => return Self::noop(),
        },
        // Default channel: stdout (crap RFC 0002 §2).
        Err(_) => 1,
      };
      if Self::validate_fd(fd).is_err() {
        return Self::noop();
      }
      // Own a dup rather than the descriptor itself: the default
      // channel is stdout, which must survive this sink's drop.
      // SAFETY: `dup` has no memory-safety preconditions; `fd` was
      // validated above and a failure returns -1, handled below.
      let owned = unsafe { libc::dup(fd) };
      if owned < 0 {
        return Self::noop();
      }
      // SAFETY: as above; a failed dup only disables the re-offer.
      let reoffer = unsafe { libc::dup(fd) };
      // SAFETY: `owned` is a fresh dup of a validated writable fd,
      // owned exclusively by the wrapped `File`.
      let file = unsafe { File::from_raw_fd(owned) };
      Self {
        writer: Some(Mutex::new(Box::new(file))),
        next_tp: AtomicUsize::new(random_tp_base()),
        reoffer_fd: (reoffer >= 0).then_some(reoffer),
        ambient: Some(AmbientAttach {
          parent: parse_var("CRAP_PARENT"),
          depth_base: parse_var("CRAP_DEPTH").unwrap_or(0),
          hello_sent: AtomicBool::new(false),
        }),
      }
    }
    #[cfg(not(unix))]
    {
      Self::noop()
    }
  }

  /// `CRAP_PARENT` of the ambient offer: the harness node id that
  /// top-level recipe nodes carry as `parent` (crap RFC 0002 §7).
  pub(crate) fn root_parent(&self) -> Option<usize> {
    self.ambient.as_ref().and_then(|ambient| ambient.parent)
  }

  /// `CRAP_DEPTH` of the ambient offer: added to recipe depths so
  /// the emitted tree continues the harness's depth numbering.
  pub(crate) fn depth_base(&self) -> u32 {
    self
      .ambient
      .as_ref()
      .map_or(0, |ambient| ambient.depth_base)
  }

  /// The environment for re-offering the protocol to a recipe child
  /// (crap RFC 0002 §6.1 passthrough): same channel via the dup'd
  /// descriptor, accept list narrowed to the negotiated format, and
  /// the child scoped under this recipe's node. `None` when the sink
  /// is inactive (the child sees the inherited environment
  /// untouched) or when no inheritable descriptor is available.
  pub(crate) fn reoffer_env(&self, tp: usize, depth: u32) -> Option<Vec<(String, String)>> {
    if !self.is_active() {
      return None;
    }
    let fd = self.reoffer_fd?;
    Some(vec![
      ("CRAP".into(), CRAP_VERSION.into()),
      ("CRAP_FD".into(), fd.to_string()),
      ("CRAP_ACCEPT".into(), NDJSON_CRAP_1.into()),
      ("CRAP_PARENT".into(), tp.to_string()),
      (
        "CRAP_DEPTH".into(),
        (self.depth_base() + depth + 1).to_string(),
      ),
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
  /// silently dropped: an unwritable events fd does not abort recipe
  /// execution, but the activation check at startup is expected to
  /// catch dead descriptors before any recipe runs.
  ///
  /// Each record is serialized to a buffer and written with a single
  /// `write_all`, so records from other producers sharing the channel
  /// (crap RFC 0002 §7) never interleave mid-line as long as every
  /// record fits `PIPE_BUF`.
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
    // Ambient attachment announces with a hello before its first
    // record (crap RFC 0002 §5).
    if let Some(ambient) = &self.ambient {
      if !ambient.hello_sent.swap(true, Ordering::Relaxed) {
        let hello = Event::Crap {
          version: 2,
          ndjson: SCHEMA_VERSION,
          format: NDJSON_CRAP_1,
          producer: concat!("just-us/", env!("CARGO_PKG_VERSION")),
          parent: ambient.parent,
        };
        if let Ok(mut hello_line) = serde_json::to_vec(&hello) {
          hello_line.push(b'\n');
          let _ = guard.write_all(&hello_line);
        }
      }
    }
    let _ = guard.write_all(&line);
    let _ = guard.flush();
  }

  pub(crate) fn emit_plan(&self, recipe_count: usize) {
    // A producer attached under a harness node must not emit
    // stream-global records (crap RFC 0002 §7): the plan would
    // re-arm the consumer's progress accounting mid-stream.
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

  fn ambient_sink<W: Write + Send + 'static>(writer: W, parent: Option<usize>) -> EventSink {
    EventSink {
      writer: Some(Mutex::new(Box::new(writer))),
      next_tp: AtomicUsize::new(random_tp_base()),
      reoffer_fd: None,
      ambient: Some(AmbientAttach {
        parent,
        depth_base: 0,
        hello_sent: AtomicBool::new(false),
      }),
    }
  }

  #[test]
  fn negotiate_picks_first_supported_token() {
    assert_eq!(negotiate("ndjson-crap/1"), Some("ndjson-crap/1"));
    assert_eq!(
      negotiate("crap-pack/1, ndjson-crap/1;families=execution+result"),
      Some("ndjson-crap/1"),
    );
    assert_eq!(negotiate("crap-pack/1"), None);
    assert_eq!(negotiate("ndjson-crap/2"), None);
    assert_eq!(negotiate(""), None);
  }

  #[test]
  fn random_tp_base_is_in_shared_channel_range() {
    for _ in 0..16 {
      let base = random_tp_base();
      assert!(base >= 1 << 40, "base {base} below 2^40");
      assert!(base < 1 << 48, "base {base} at or above 2^48");
    }
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
  fn ambient_hello_precedes_first_record() {
    let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
    {
      let sink = ambient_sink(BufHandle(Arc::clone(&buf)), Some(7));
      sink.emit(&Event::RecipeCommand {
        tp: 1,
        command: "echo hi",
        line: 1,
      });
    }
    let bytes = Arc::try_unwrap(buf).unwrap().into_inner().unwrap();
    let out = String::from_utf8(bytes).unwrap();
    let mut lines = out.lines();
    let hello = lines.next().unwrap();
    assert_eq!(
      hello,
      format!(
        "{{\"type\":\"crap\",\"version\":2,\"ndjson\":1,\
         \"format\":\"ndjson-crap/1\",\"producer\":\"just-us/{}\",\
         \"parent\":7}}",
        env!("CARGO_PKG_VERSION"),
      ),
    );
    assert!(
      lines
        .next()
        .unwrap()
        .starts_with("{\"type\":\"recipe_command\"")
    );
    assert!(lines.next().is_none());
  }

  #[test]
  fn nested_attachment_suppresses_plan() {
    let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
    {
      let sink = ambient_sink(BufHandle(Arc::clone(&buf)), Some(7));
      sink.emit_plan(3);
    }
    let bytes = Arc::try_unwrap(buf).unwrap().into_inner().unwrap();
    assert!(bytes.is_empty(), "nested plan must be suppressed");
  }

  #[test]
  fn root_attachment_emits_hello_then_plan() {
    let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
    {
      let sink = ambient_sink(BufHandle(Arc::clone(&buf)), None);
      sink.emit_plan(3);
    }
    let bytes = Arc::try_unwrap(buf).unwrap().into_inner().unwrap();
    let out = String::from_utf8(bytes).unwrap();
    let mut lines = out.lines();
    assert!(lines.next().unwrap().starts_with("{\"type\":\"crap\""));
    assert_eq!(
      lines.next().unwrap(),
      "{\"type\":\"plan\",\"version\":1,\"recipe_count\":3}",
    );
  }

  #[test]
  fn explicit_sink_emits_no_hello() {
    let out = capture(|s| s.emit_plan(2));
    assert_eq!(
      out,
      "{\"type\":\"plan\",\"version\":1,\"recipe_count\":2}\n"
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
}
