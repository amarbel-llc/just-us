use super::*;

pub(crate) struct SignalHandler {
  caught: Option<Signal>,
  children: BTreeMap<i32, (Command, bool)>,
  initialized: bool,
  verbosity: Verbosity,
}

impl SignalHandler {
  pub(crate) fn install(verbosity: Verbosity) -> RunResult<'static> {
    let mut instance = Self::instance();
    instance.verbosity = verbosity;
    if !instance.initialized {
      Platform::install_signal_handler(|signal| Self::instance().handle(signal))?;
      instance.initialized = true;
    }
    Ok(())
  }

  pub(crate) fn instance() -> MutexGuard<'static, Self> {
    static INSTANCE: Mutex<SignalHandler> = Mutex::new(SignalHandler::new());

    match INSTANCE.lock() {
      Ok(guard) => guard,
      Err(poison_error) => {
        eprintln!(
          "{}",
          Error::internal(format!("signal handler mutex poisoned: {poison_error}"),)
            .color_display(Color::auto().stderr())
        );
        process::exit(EXIT_FAILURE);
      }
    }
  }

  const fn new() -> Self {
    Self {
      caught: None,
      children: BTreeMap::new(),
      initialized: false,
      verbosity: Verbosity::default(),
    }
  }

  fn handle(&mut self, signal: Signal) {
    if signal.is_fatal() {
      if self.children.is_empty() {
        process::exit(signal.code());
      }

      if self.caught.is_none() {
        self.caught = Some(signal);
      }
    }

    match signal {
      // SIGHUP, SIGINT, and SIGQUIT are normally sent to all processes
      // in the foreground process group by the terminal. Children with
      // PTY stdio may not be in the same process group, so forward
      // these signals to children that opted in via `forward_all`.
      // For children sharing the process group, we do nothing and let
      // the kernel deliver the signal directly.
      Signal::Hangup | Signal::Interrupt | Signal::Quit =>
      {
        #[cfg(not(windows))]
        for (&child, &(_, forward_all)) in &self.children {
          if forward_all {
            if self.verbosity.loquacious() {
              eprintln!("just: sending {signal} to child process {child}");
            }
            nix::sys::signal::kill(
              nix::unistd::Pid::from_raw(child),
              Some(signal.into()),
            )
            .ok();
          }
        }
      }
      // SIGTERM is not sent by the terminal, so always forward it
      Signal::Terminate =>
      {
        #[cfg(not(windows))]
        for (&child, _) in &self.children {
          if self.verbosity.loquacious() {
            eprintln!("just: sending {signal} to child process {child}");
          }
          nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(child),
            Some(signal.into()),
          )
          .ok();
        }
      }
      #[cfg(any(
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "ios",
        target_os = "macos",
        target_os = "netbsd",
        target_os = "openbsd",
      ))]
      Signal::Info => {
        let id = process::id();
        if self.children.is_empty() {
          eprintln!("just {id}: no child processes");
        } else {
          let n = self.children.len();

          let mut message = format!(
            "just {id}: {n} child {}:\n",
            if n == 1 { "process" } else { "processes" }
          );

          for (&child, (command, _)) in &self.children {
            use std::fmt::Write;
            writeln!(message, "{child}: {command:?}").unwrap();
          }

          eprint!("{message}");
        }
      }
    }
  }

  pub(crate) fn spawn<T>(
    command: Command,
    f: impl FnOnce(process::Child) -> io::Result<T>,
  ) -> (io::Result<T>, Option<Signal>) {
    Self::spawn_inner(command, false, f)
  }

  pub(crate) fn spawn_forward_all<T>(
    command: Command,
    f: impl FnOnce(process::Child) -> io::Result<T>,
  ) -> (io::Result<T>, Option<Signal>) {
    Self::spawn_inner(command, true, f)
  }

  fn spawn_inner<T>(
    mut command: Command,
    forward_all: bool,
    f: impl FnOnce(process::Child) -> io::Result<T>,
  ) -> (io::Result<T>, Option<Signal>) {
    let mut instance = Self::instance();

    let child = match command.spawn() {
      Err(err) => return (Err(err), None),
      Ok(child) => child,
    };

    let pid = match child.id().try_into() {
      Err(err) => {
        return (
          Err(io::Error::other(format!("invalid child PID: {err}"))),
          None,
        );
      }
      Ok(pid) => pid,
    };

    // Reset stdio so parent doesn't hold fds (e.g. PTY slave) that
    // would prevent EOF detection on the master side.
    command.stdout(Stdio::null());
    command.stderr(Stdio::null());

    instance.children.insert(pid, (command, forward_all));

    drop(instance);

    let result = f(child);

    let mut instance = Self::instance();

    instance.children.remove(&pid);

    (result, instance.caught)
  }
}
