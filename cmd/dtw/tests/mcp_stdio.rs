//! Real process checks for Tokio's non-abortable macOS stdio tasks.
#![cfg(target_os = "macos")]

use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

struct Mcp(Child);
impl Mcp {
    fn start() -> Self {
        Self(
            Command::new(env!("CARGO_BIN_EXE_dtw"))
                .arg("mcp")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }
    fn initialize(&mut self) {
        let input = self.0.stdin.as_mut().unwrap();
        writeln!(input, r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-06-18","capabilities":{{}},"clientInfo":{{"name":"stdio-test","version":"1"}}}}}}"#).unwrap();
        input.flush().unwrap();
    }
    fn wait(&mut self) -> ExitStatus {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < until, "stdio process failed to exit");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn eof_after_initialize_exits_cleanly() {
    let mut child = Mcp::start();
    let stdout = child.0.stdout.take().unwrap();
    let (sent, received) = mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        let mut line = String::new();
        BufReader::new(stdout).read_line(&mut line).unwrap();
        let _ = sent.send(line);
    });
    child.initialize();
    let reply: serde_json::Value =
        serde_json::from_str(&received.recv_timeout(Duration::from_secs(5)).unwrap()).unwrap();
    assert_eq!(reply["result"]["serverInfo"]["name"], "desktop-world");
    drop(child.0.stdin.take());
    assert!(child.wait().success());
    reader.join().unwrap();
}

#[test]
fn broken_stdout_exits_even_while_stdin_stays_open() {
    let mut child = Mcp::start();
    drop(child.0.stdout.take());
    child.initialize();
    // Keep the client's writer alive. A normal Tokio Runtime drop would wait
    // forever for its blocking stdin task if cleanup used that shutdown path.
    assert!(child.0.stdin.is_some());
    assert_eq!(child.wait().code(), Some(2));
}

#[test]
fn unread_stdout_exits_without_waiting_for_the_blocking_writer() {
    let mut child = Mcp::start();
    child.initialize();
    let input = child.0.stdin.as_mut().unwrap();
    writeln!(
        input,
        r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#
    )
    .unwrap();
    // Keep stdout open without reading it. Discovery responses fill the real
    // pipe and block Tokio's macOS writer; no native application is touched.
    for id in 2..130 {
        writeln!(
            input,
            r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/list"}}"#
        )
        .unwrap();
    }
    input.flush().unwrap();
    assert_eq!(child.wait().code(), Some(2));
}
