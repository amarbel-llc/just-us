use super::*;

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

/// Sink that serializes `Event` records to a writer as one JSON record
/// per line. When constructed with `EventSink::noop()`, all `emit`
/// calls are no-ops; this lets the normal code path call `emit`
/// unconditionally without paying allocation cost when `--events-fd`
/// is not set.
pub(crate) struct EventSink {
  writer: Option<Mutex<Box<dyn Write + Send>>>,
}

impl EventSink {
  pub(crate) fn noop() -> Self {
    Self { writer: None }
  }

  pub(crate) fn from_writer<W: Write + Send + 'static>(writer: W) -> Self {
    Self {
      writer: Some(Mutex::new(Box::new(writer))),
    }
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

  #[cfg(unix)]
  fn validate_fd(fd: i32) -> io::Result<()> {
    // `fcntl(fd, F_GETFL)` returns the file's status flags or -1
    // with errno set (EBADF for a closed fd, EINVAL for some
    // unsupported descriptor types). We use that to gate ownership
    // before wrapping the raw fd in a File.
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
  pub(crate) fn emit(&self, event: &Event<'_>) {
    let Some(writer) = &self.writer else { return };
    let mut guard = match writer.lock() {
      Ok(guard) => guard,
      Err(_) => return,
    };
    if serde_json::to_writer(&mut *guard, event).is_err() {
      return;
    }
    let _ = guard.write_all(b"\n");
    let _ = guard.flush();
  }

  pub(crate) fn emit_plan(&self, recipe_count: usize) {
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
    assert_eq!(out, "{\"type\":\"plan\",\"version\":1,\"recipe_count\":3}\n");
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
}
