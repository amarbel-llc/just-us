fn main() {
  let args = std::iter::once(std::ffi::OsString::from("just-us-agents"))
    .chain(std::iter::once(std::ffi::OsString::from("--agents-only")))
    .chain(std::env::args_os().skip(1));

  if let Err(code) = just::run(args) {
    std::process::exit(code);
  }
}
