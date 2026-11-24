//! Single app-scoped DTW entry. No unrestricted or compatibility runtime.
use desktop_world::app_protocol::{self, wire::WireError, Reply};
use std::process::ExitCode;

const HELP: &str = "\
DTW — app-scoped desktop operations

Usage:
  dtw app catalog
  dtw mcp
  dtw package import|bind|activate|unbind|status|gc [--store <absolute-directory>]
  dtw setup resume|prepare|confirm [--store <absolute-directory>]
  dtw --help
  dtw --version

Every execution names an app identifier, such as com.google.Chrome.
Native providers: macOS, Windows, and Linux X11.
Linux requires a local X11 session (Wayland and XWayland are unsupported).";

fn run(args: &[String]) -> Result<(), WireError> {
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] | ["--help" | "-h"] | ["app"] | ["app", "--help" | "-h"] => println!("{HELP}"),
        ["--version" | "-V"] => println!("dtw {}", env!("CARGO_PKG_VERSION")),
        [command @ ("package" | "setup"), ..] => {
            let (result, code) = if *command == "setup" {
                (desktop_world::package::run_setup(&args[1..]), "setup_error")
            } else {
                (desktop_world::package::run(&args[1..]), "package_error")
            };
            let value = result.map_err(|error| WireError {
                code: code.into(),
                message: error.message,
                details: error.details.map(|details| *details),
            })?;
            println!("{value}");
        }
        ["app", ..] => return app_protocol::run(&args[1..]),
        ["mcp", ..] => return app_protocol::run(args),
        _ => {
            return Err(WireError {
                details: None,
                code: "invalid_arguments".into(),
                message: "Unknown command; use dtw --help for the app-scoped entry".into(),
            })
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    #[cfg(unix)]
    desktop_world::backends::record_launch_path();
    // Setup capability discovery must work without opening the store.
    let setup_help = std::env::args_os().nth(1).is_some_and(|arg| arg == "setup")
        && desktop_world::package::setup_wants_help(&std::env::args().skip(2).collect::<Vec<_>>());
    // Runtime and worker processes independently protect their own packages.
    let _package_lease = match if setup_help {
        Ok(None)
    } else {
        desktop_world::package::runtime_lease()
    } {
        Ok(lease) => lease,
        Err(error) => {
            eprintln!(
                "{}",
                Reply::error(WireError {
                    code: "package_unavailable".into(),
                    message: error.message,
                    details: error.details.map(|details| *details)
                })
                .data
            );
            return ExitCode::from(2);
        }
    };
    #[cfg(target_os = "macos")]
    if desktop_world::backends::macos::overlay_worker_entry()
        || desktop_world::backends::macos::worker_entry()
    {
        return ExitCode::SUCCESS;
    }
    #[cfg(target_os = "windows")]
    if desktop_world::backends::windows::worker_entry() {
        return ExitCode::SUCCESS;
    }
    #[cfg(target_os = "linux")]
    if desktop_world::backends::linux::worker_entry() {
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
