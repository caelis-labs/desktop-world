//! Standard stdio CLI, lazy native factory and process lifecycle.
use super::{
    mcp::{self, Event},
    server::{Application, Factory, Server, Started},
    wire::{self, WireError},
};
use crate::{
    backends::macos::NativeBackend,
    computer::{Backend, Error},
};
use serde_json::{json, Value};
use std::{
    io::{self, BufReader, BufWriter, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};
#[derive(Default)]
struct NativeFactory {
    catalog: Mutex<Option<Arc<NativeBackend>>>,
}
impl Factory for NativeFactory {
    type Provider = NativeBackend;
    fn description(&self) -> Value {
        NativeBackend::description()
    }
    fn catalog(&self) -> Result<Vec<Application>, Error> {
        let reader = {
            let mut slot = self.catalog.lock().unwrap();
            if slot.is_none() {
                *slot = Some(Arc::new(NativeBackend::spawn()?));
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
        NativeBackend::spawn()
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
        ["mcp"] => serve(Server::new(NativeFactory::default())),
        _ => Err(error(
            "invalid_arguments",
            "Usage: dtw mcp | dtw app catalog",
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
    let parent = unsafe { libc::getppid() };
    if parent <= 1 {
        return Err(error("parent_lost", "MCP client parent is unavailable"));
    }
    let done = Arc::new(AtomicBool::new(false));
    let _watchdog = Watchdog(done.clone());
    let watched = Arc::downgrade(&server);
    std::thread::Builder::new()
        .name("dtw-client-parent".into())
        .spawn(move || {
            while !done.load(Ordering::Acquire) {
                if unsafe { libc::getppid() } != parent {
                    let status = watched.upgrade().map_or(0, |s| cleanup_for_exit(&s));
                    std::process::exit(status);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        })
        .map_err(|_| error("transport_error", "Cannot start lifecycle watcher"))?;
    let (output, responses) = mpsc::sync_channel::<Value>(8);
    let (written, drained) = mpsc::sync_channel(1);
    let writer_server = server.clone();
    std::thread::Builder::new()
        .name("dtw-mcp-output".into())
        .spawn(move || {
            let mut stdout = BufWriter::new(io::stdout().lock());
            for response in responses {
                let result = serde_json::to_writer(&mut stdout, &response)
                    .map_err(io::Error::other)
                    .and_then(|()| stdout.write_all(b"\n"))
                    .and_then(|()| stdout.flush());
                if result.is_err() {
                    terminate_transport(
                        &writer_server,
                        "transport_error",
                        "Cannot write MCP response",
                    );
                }
            }
            let _ = written.send(());
        })
        .map_err(|_| error("transport_error", "Cannot start output writer"))?;
    let mut session = mcp::Session::default();
    let mut input = BufReader::new(io::stdin().lock());
    let outcome = (|| {
        while let Some(line) = mcp::read_frame(&mut input)? {
            let event = match serde_json::from_slice(&line) {
                Ok(value) => session.handle(value),
                Err(_) => Event::Reply(mcp::rpc_error(Value::Null, -32700, "Invalid JSON")),
            };
            let response = match event {
                Event::None => None,
                Event::Reply(value) => Some(value),
                Event::Cancel(id) => {
                    server.cancel(&id);
                    None
                }
                Event::Call {
                    id,
                    name,
                    args,
                    structured,
                } => match server.begin(id.clone(), &name, args) {
                    Started::InFlight => None,
                    Started::Reply(reply) => Some(mcp::response(id, reply, structured)),
                    Started::Call(call) => {
                        let server = server.clone();
                        let output = output.clone();
                        std::thread::Builder::new()
                            .name("dtw-mcp-call".into())
                            .spawn(move || {
                                let reply = match std::panic::catch_unwind(
                                    std::panic::AssertUnwindSafe(|| server.run(&call)),
                                ) {
                                    Ok(reply) => reply,
                                    Err(_) => terminate_transport(
                                        &server,
                                        "execution_panicked",
                                        "Tool worker panicked; original effects may be unknown",
                                    ),
                                };
                                if !call.cancelled.load(Ordering::Acquire)
                                    && output
                                        .try_send(mcp::response(id, reply, structured))
                                        .is_err()
                                {
                                    terminate_transport(
                                        &server,
                                        "output_backpressure",
                                        "MCP output is stalled; stopping native work",
                                    );
                                }
                            })
                            .map_err(|_| error("transport_error", "Cannot start tool call"))?;
                        None
                    }
                },
            };
            if let Some(response) = response {
                output.try_send(response).map_err(|_| {
                    error(
                        "output_backpressure",
                        "MCP output is stalled; stopping native work",
                    )
                })?;
            }
        }
        Ok(())
    })();
    let cleanup = server.close();
    drop(output);
    let _ = drained.recv_timeout(Duration::from_millis(200));
    cleanup.and(outcome)
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
fn terminate_transport(server: &Server<NativeFactory>, code: &str, message: &str) -> ! {
    report_error(error(code, message));
    cleanup_for_exit(server);
    std::process::exit(2)
}
