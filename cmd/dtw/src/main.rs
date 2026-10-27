//! Single app-scoped DTW entry. No unrestricted or compatibility runtime.
use desktop_world::app_protocol::{self, wire::WireError, Reply};
use std::process::ExitCode;

const HELP: &str = "\
DTW — app-scoped desktop operations

Usage:
  dtw app catalog
  dtw app serve --grant /absolute/host-grant.json
  dtw --help
  dtw --version

The Harness supplies the trusted app grant. No grant is inferred from MCP input.
Native app operations currently require macOS.";

fn run(args: &[String]) -> Result<(), WireError> {
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] | ["--help" | "-h"] | ["app"] | ["app", "--help" | "-h"] => println!("{HELP}"),
        ["--version" | "-V"] => println!("dtw {}", env!("CARGO_PKG_VERSION")),
        ["app", ..] => return app_protocol::run(&args[1..]),
        _ => {
            return Err(WireError {
                code: "invalid_arguments".into(),
                message: "Unknown command; use dtw --help for the app-scoped entry".into(),
            })
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    #[cfg(target_os = "macos")]
    if desktop_world::backends::macos::worker_entry() {
        return ExitCode::SUCCESS;
    }
    match run(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}", Reply::error(error).data);
            ExitCode::from(2)
        }
    }
}
