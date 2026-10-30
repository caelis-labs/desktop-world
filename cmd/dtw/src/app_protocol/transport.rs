//! Standard stdio CLI, lazy native factory and process lifecycle.
use super::{
    mcp,
    server::{Application, Factory, Server},
    wire::{self, WireError},
};
#[cfg(target_os = "linux")]
use crate::backends::linux::NativeBackend;
#[cfg(target_os = "macos")]
use crate::backends::macos::NativeBackend;
#[cfg(target_os = "windows")]
use crate::backends::windows::NativeBackend;
use crate::computer::{Backend, Error, InputPolicy};
use serde_json::{json, Value};
use std::{
    io::{self, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
#[derive(Default)]
struct NativeFactory {
    input_policy: InputPolicy,
    catalog: Mutex<Option<Arc<NativeBackend>>>,
    #[cfg(target_os = "macos")]
    cursor: Arc<crate::backends::macos::CursorSession>,
}
impl Factory for NativeFactory {
    type Provider = NativeBackend;
    fn input_policy(&self) -> InputPolicy {
        self.input_policy
    }
    fn description(&self) -> Value {
        NativeBackend::description()
    }
    fn catalog(&self) -> Result<Vec<Application>, Error> {
        let reader = {
            let mut slot = self.catalog.lock().unwrap();
            if slot.is_none() {
                *slot = Some(Arc::new(NativeBackend::spawn_with_policy(
                    self.input_policy,
                )?));
            }
            slot.as_ref().unwrap().clone()
        };
        reader.catalog(Duration::from_secs(8)).map(|apps| {
            apps.into_iter()
                .map(|a| Application {
                    app_id: a.app_id,
                    app: a.app,
                    name: a.name,
                })
                .collect()
        })
    }
    fn connect(&self) -> Result<NativeBackend, Error> {
        #[cfg(target_os = "macos")]
        {
            NativeBackend::spawn_with_cursor(self.input_policy, self.cursor.clone())
        }
        #[cfg(not(target_os = "macos"))]
        {
            NativeBackend::spawn_with_policy(self.input_policy)
        }
    }
    fn close_session(&self) -> Result<(), Error> {
        #[cfg(target_os = "macos")]
        self.cursor.close().map_err(|error| {
            Error::BackendFailed(format!("cursor presentation cleanup: {error}"))
        })?;
        Ok(())
    }
    fn close_catalog(&self) -> Result<(), Error> {
        let reader = self.catalog.lock().unwrap().clone();
        if let Some(reader) = reader {
            reader.shutdown()?;
            let mut slot = self.catalog.lock().unwrap();
            if slot
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &reader))
            {
                *slot = None;
            }
        }
        Ok(())
    }
}
fn error(code: &str, message: &str) -> WireError {
    WireError {
        code: code.into(),
        message: message.into(),
        details: None,
    }
}
pub fn run(args: &[String]) -> Result<(), WireError> {
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["catalog"] => {
            let factory = NativeFactory::default();
            let apps = factory.catalog();
            factory.close_catalog().map_err(|e| wire::core_error(&e))?;
            println!(
                "{}",
                json!({"version":1,"apps":super::server::public_apps(apps.map_err(|e|wire::core_error(&e))?)})
            );
            Ok(())
        }
        ["mcp", options @ ..] => serve(Server::new(NativeFactory {
            input_policy: input_policy(options)?,
            ..NativeFactory::default()
        })),
        _ => Err(error(
            "invalid_arguments",
            "Usage: dtw mcp [--input-policy auto|background-only] | dtw app catalog",
        )),
    }
}
fn input_policy(options: &[&str]) -> Result<InputPolicy, WireError> {
    match options {
        [] | ["--input-policy", "auto"] => Ok(InputPolicy::Auto),
        ["--input-policy", "background-only"] => Ok(InputPolicy::BackgroundOnly),
        _ => Err(error(
            "invalid_arguments",
            "Expected --input-policy auto|background-only",
        )),
    }
}

struct Watchdog(Arc<AtomicBool>);
impl Drop for Watchdog {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
fn serve(server: Arc<Server<NativeFactory>>) -> Result<(), WireError> {
    #[cfg(unix)]
    let parent_alive: Box<dyn Fn() -> bool + Send> = {
        let parent = unsafe { libc::getppid() };
        if parent <= 1 {
            return Err(error("parent_lost", "MCP client parent is unavailable"));
        }
        Box::new(move || unsafe { libc::getppid() } == parent)
    };
    #[cfg(target_os = "windows")]
    let parent_alive = crate::backends::windows::parent_alive_check()
        .map_err(|_| error("parent_lost", "Cannot retain MCP client parent identity"))?;
    let done = Arc::new(AtomicBool::new(false));
    let _watchdog = Watchdog(done.clone());
    let watched = Arc::downgrade(&server);
    std::thread::Builder::new()
        .name("dtw-client-parent".into())
        .spawn(move || {
            while !done.load(Ordering::Acquire) {
                if !parent_alive() {
                    let status = watched.upgrade().map_or(0, |s| cleanup_for_exit(&s));
                    std::process::exit(status);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        })
        .map_err(|_| error("transport_error", "Cannot start lifecycle watcher"))?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|_| error("transport_error", "Cannot start MCP runtime"))?;
    let outcome = runtime.block_on(async {
        let (read, write) = rmcp::transport::stdio();
        mcp::serve_io(server, read, write).await
    });
    // Tokio's macOS stdio reader is a blocking task that cannot be aborted.
    // Native cleanup has completed; an open stdin must not prevent exit after
    // a write failure. Process exit will end the remaining stdio task.
    runtime.shutdown_timeout(Duration::from_millis(200));
    outcome
}

/// Background I/O failure must also stop a reader blocked on an open stdin.
/// Panics in cleanup are an unknown outcome; process exit still triggers each
/// owned native worker's parent-loss cleanup instead of leaving input alive.
fn report_error(failure: WireError) {
    // A client may close stderr along with stdout. Diagnostics cannot prevent cleanup.
    let _ = writeln!(io::stderr().lock(), "{}", super::Reply::error(failure).data);
}
fn cleanup_for_exit(server: &Server<NativeFactory>) -> i32 {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| server.close())) {
        Ok(Ok(())) => 0,
        Ok(Err(failure)) => {
            report_error(failure);
            2
        }
        Err(_) => {
            report_error(error(
                "shutdown_failed",
                "Cleanup panicked; native outcome remains unknown",
            ));
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn service_policy_launch_options_are_exact_and_default_to_auto() {
        assert_eq!(input_policy(&[]).unwrap(), InputPolicy::Auto);
        assert_eq!(
            input_policy(&["--input-policy", "auto"]).unwrap(),
            InputPolicy::Auto
        );
        assert_eq!(
            input_policy(&["--input-policy", "background-only"]).unwrap(),
            InputPolicy::BackgroundOnly
        );
        for options in [
            vec!["--input-policy"],
            vec!["--input-policy", "background_only"],
            vec!["--input-policy", "foreground"],
            vec!["--input-policy=auto"],
            vec![
                "--input-policy",
                "auto",
                "--input-policy",
                "background-only",
            ],
            vec!["--other", "auto"],
        ] {
            assert_eq!(
                input_policy(&options).unwrap_err().code,
                "invalid_arguments"
            );
        }
    }
}
