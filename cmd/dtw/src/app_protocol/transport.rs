//! App-scoped macOS CLI and bounded stdio lifecycle.
use super::{grant, mcp, wire::WireError, Coordinator};
use crate::{backends::macos::NativeBackend, computer::Backend};
use serde_json::{json, Value};
use std::{
    io::{self, BufReader, BufWriter, Write},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(8);
const STOP_TIMEOUT: Duration = Duration::from_secs(2);

pub fn run(args: &[String]) -> Result<(), WireError> {
    match args {
        [command] if command == "catalog" => {
            let backend = NativeBackend::spawn().map_err(|e| super::wire::core_error(&e))?;
            let result = backend.catalog(DISCOVERY_TIMEOUT);
            let shutdown = backend.shutdown();
            let apps = result.map_err(|e| super::wire::core_error(&e))?;
            shutdown.map_err(|e| super::wire::core_error(&e))?;
            let apps: Vec<_> = apps
                .into_iter()
                .map(|entry| {
                    let mut value = grant::app_json(&entry.app);
                    value["name"] = json!(entry.name);
                    value
                })
                .collect();
            println!("{}", json!({"version":1,"apps":apps}));
            Ok(())
        }
        [command, flag, path] if command == "serve" && flag == "--grant" => {
            if !Path::new(path).is_absolute() {
                return Err(failure("invalid_grant", "Host grant path must be absolute"));
            }
            let authority = grant::load(Path::new(path))?;
            let parent = unsafe { libc::getppid() };
            if parent <= 1 {
                return Err(failure("parent_lost", "Host parent is unavailable"));
            }
            let backend = NativeBackend::spawn().map_err(|e| super::wire::core_error(&e))?;
            let apps = backend
                .catalog(DISCOVERY_TIMEOUT)
                .map_err(|e| super::wire::core_error(&e))?;
            let Some(app) = apps.into_iter().find(|entry| entry.app == authority.app) else {
                backend
                    .shutdown()
                    .map_err(|e| super::wire::core_error(&e))?;
                return Err(failure(
                    "wrong_app",
                    "The granted app instance is no longer available",
                ));
            };
            let owner = Coordinator::new(backend, authority, app.name);
            let outcome = serve(owner.clone(), parent);
            let stopped = owner.close();
            if !owner.wait_idle(STOP_TIMEOUT) {
                return Err(failure(
                    "shutdown_failed",
                    "Execution did not settle during shutdown",
                ));
            }
            if stopped.is_error {
                return Err(failure("shutdown_failed", &stopped.data.to_string()));
            }
            match outcome {
                Ok(drained) => {
                    // Best-effort drain only, after native resources are closed.
                    let _ = drained.recv_timeout(Duration::from_millis(200));
                    Ok(())
                }
                Err(error) => Err(error),
            }
        }
        _ => Err(failure(
            "invalid_arguments",
            "Usage: dtw app catalog | dtw app serve --grant /absolute/host-grant.json",
        )),
    }
}
fn failure(code: &str, message: &str) -> WireError {
    WireError {
        details: None,
        code: code.into(),
        message: message.into(),
    }
}
struct Watchdog(Arc<AtomicBool>);
impl Drop for Watchdog {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
fn serve(
    owner: Arc<Coordinator<NativeBackend>>,
    parent: libc::pid_t,
) -> Result<mpsc::Receiver<()>, WireError> {
    let done = Arc::new(AtomicBool::new(false));
    let _watchdog = Watchdog(done.clone());
    let watched = Arc::downgrade(&owner);
    std::thread::Builder::new()
        .name("dtw-app-parent".into())
        .spawn(move || {
            while !done.load(Ordering::Acquire) {
                let Some(watched) = watched.upgrade() else {
                    break;
                };
                if unsafe { libc::getppid() } != parent {
                    watched.close();
                    watched.wait_idle(STOP_TIMEOUT);
                    std::process::exit(0);
                }
                watched.expire_if_needed();
                drop(watched);
                std::thread::sleep(Duration::from_millis(50));
            }
        })
        .map_err(|_| failure("transport_error", "Cannot start Host lifecycle watcher"))?;
    // A stalled receiver cannot block stdin cancellation or EOF. Saturation
    // closes the context; an unanswered action must be treated as uncertain.
    let (output, responses) = mpsc::sync_channel::<Value>(8);
    let (written, drained) = mpsc::sync_channel(1);
    let writer_owner = owner.clone();
    std::thread::Builder::new()
        .name("dtw-app-output".into())
        .spawn(move || {
            let mut stdout = BufWriter::new(io::stdout().lock());
            for response in responses {
                let result = serde_json::to_writer(&mut stdout, &response)
                    .map_err(io::Error::other)
                    .and_then(|()| stdout.write_all(b"\n"))
                    .and_then(|()| stdout.flush());
                if result.is_err() {
                    writer_owner.close();
                    writer_owner.wait_idle(STOP_TIMEOUT);
                    std::process::exit(2);
                }
            }
            let _ = written.send(());
        })
        .map_err(|_| failure("transport_error", "Cannot start MCP output writer"))?;
    let mut session = mcp::Session::default();
    let mut input = BufReader::new(io::stdin().lock());
    let outcome = (|| {
        while let Some(line) = mcp::read_frame(&mut input)? {
            let response = match serde_json::from_slice(&line) {
                Ok(request) => session.handle(&owner, request),
                Err(_) => Some(mcp::rpc_error(Value::Null, -32700, "Invalid JSON")),
            };
            if let Some(response) = response {
                output.try_send(response).map_err(|_| {
                    failure(
                        "output_backpressure",
                        "MCP output is stalled; context closed",
                    )
                })?;
            }
        }
        Ok(())
    })();
    drop(output);
    outcome.map(|()| drained)
}
