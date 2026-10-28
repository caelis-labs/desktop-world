//! Bounded, sequential byte transport for one owned native worker process.
//!
//! JSON decoding and request/response identity belong to the adapter. This layer
//! accepts a nonempty UTF-8 line without its newline and returns the same form.
//! A transport failure permanently retires the process; calls are never replayed.
//! The worker must not pass these pipes to descendants and must implement its
//! own parent-death/EOF watchdog. Killing a process does not undo native effects.

use std::fmt;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;

pub const MAX_REQUEST_BYTES: usize = 64 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 24 * 1024 * 1024;
const POLL: Duration = Duration::from_millis(5);
const THREAD_CLEANUP_BUDGET: Duration = Duration::from_millis(200);
#[cfg(unix)]
const TERMINATION_GRACE: Duration = Duration::from_millis(500);
// Private worker exit status: admitted input could not drain before shutdown.
pub(crate) const INPUT_QUIESCE_FAILED_EXIT_CODE: i32 = 74;

#[cfg(unix)]
fn request_termination(child: &Child) -> std::io::Result<()> {
    // Keep this transport std-only (also compiled directly with rustc in CI).
    unsafe extern "C" {
        fn kill(pid: std::ffi::c_int, signal: std::ffi::c_int) -> std::ffi::c_int;
    }
    // SIGTERM is 15 on the Unix targets supported by DTW. The unreaped Child
    // remains exclusively owned under Process.child, so its PID cannot recycle.
    if unsafe { kill(child.id() as std::ffi::c_int, 15) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    Cancelled,
    TimedOut,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransportErrorKind {
    Spawn(String),
    ThreadSpawn(String),
    InvalidRequest,
    RequestTooLarge,
    Busy,
    Stopped,
    Control(StopReason),
    Write(String),
    Read(String),
    UnexpectedEof,
    MalformedFrame,
    ResponseTooLarge,
    Shutdown(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransportError {
    pub kind: TransportErrorKind,
    /// False proves this request was never handed to a pipe write. True is
    /// conservative: partial writes and a lost reply can already have effects.
    pub request_may_have_been_sent: bool,
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?} (request may have been sent: {})",
            self.kind, self.request_may_have_been_sent
        )
    }
}

impl std::error::Error for TransportError {}

impl TransportError {
    fn local(kind: TransportErrorKind) -> Self {
        Self {
            kind,
            request_may_have_been_sent: false,
        }
    }
}

struct Process {
    child: Mutex<Option<Child>>,
    stopped: AtomicBool,
    shutdown_failure: Mutex<Option<String>>,
}

impl Process {
    fn reap_if_exited(&self) -> bool {
        let status = {
            let mut slot = self.child.lock().expect("worker process lock poisoned");
            match slot.as_mut() {
                None => Ok(true),
                Some(child) => child
                    .try_wait()
                    .map(|status| {
                        if let Some(status) = status {
                            self.record_exit(status, false);
                            slot.take();
                            true
                        } else {
                            false
                        }
                    })
                    .map_err(|error| self.shutdown_error(error)),
            }
        };
        match status {
            Ok(false) => false,
            Ok(true) => {
                self.stopped.store(true, Ordering::SeqCst);
                true
            }
            Err(_) => {
                let _ = self.stop();
                true
            }
        }
    }

    fn stop(&self) -> Result<(), TransportErrorKind> {
        self.stopped.store(true, Ordering::SeqCst);
        let mut child_slot = self.child.lock().expect("worker process lock poisoned");
        let Some(child) = child_slot.as_mut() else {
            return self.shutdown_result();
        };
        if let Some(status) = child
            .try_wait()
            .map_err(|error| self.shutdown_error(error))?
        {
            self.record_exit(status, false);
            child_slot.take();
            return self.shutdown_result();
        }
        #[cfg(unix)]
        {
            let requested = request_termination(child);
            if requested.is_ok() {
                let until = Instant::now() + TERMINATION_GRACE;
                loop {
                    if let Some(status) = child
                        .try_wait()
                        .map_err(|error| self.shutdown_error(error))?
                    {
                        self.record_exit(status, true);
                        child_slot.take();
                        return self.shutdown_result();
                    }
                    let left = until.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        break;
                    }
                    thread::sleep(left.min(POLL));
                }
                self.record_shutdown_failure("Worker did not exit within the 500 ms termination grace; forced termination cannot confirm input cleanup".into());
            } else {
                self.record_shutdown_failure(format!(
                    "Cannot request graceful worker termination: {}; input cleanup is unconfirmed",
                    requested.unwrap_err()
                ));
            }
        }
        // The process is owned, never looked up again by PID. Reap it even when
        // kill reports that it has already exited. Do not wait on a live process
        // after a failed kill; retain ownership so stop/Drop can retry cleanup.
        if let Err(kill_error) = child.kill() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.record_exit(status, false);
                    child_slot.take();
                    return self.shutdown_result();
                }
                Ok(None) => return Err(self.shutdown_error(kill_error)),
                Err(error) => {
                    self.shutdown_error(kill_error);
                    return Err(self.shutdown_error(error));
                }
            }
        }
        let status = child.wait().map_err(|error| self.shutdown_error(error))?;
        #[cfg(unix)]
        self.record_exit(status, false);
        // Windows has no cooperative native-input backend. Its successful,
        // explicitly requested TerminateProcess is expected transport cleanup.
        #[cfg(not(unix))]
        let _ = status;
        child_slot.take();
        self.shutdown_result()
    }

    fn record_shutdown_failure(&self, message: String) {
        self.shutdown_failure
            .lock()
            .expect("worker shutdown lock poisoned")
            .get_or_insert(message);
    }

    fn shutdown_error(&self, error: std::io::Error) -> TransportErrorKind {
        // Callers still hold child while recording this failure. Another stop
        // cannot finish a successful retry before the first fault is durable.
        let mut failure = self
            .shutdown_failure
            .lock()
            .expect("worker shutdown lock poisoned");
        let message = failure.get_or_insert_with(|| {
            format!("Worker cleanup failed: {error}; input cleanup is unconfirmed")
        });
        TransportErrorKind::Shutdown(message.clone())
    }

    fn record_exit(&self, status: std::process::ExitStatus, expected_sigterm: bool) {
        if status.success() {
            return;
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            // Only the grace loop after our own successful SIGTERM request can
            // expect this status. Idle reaping must not excuse external SIGTERM.
            if expected_sigterm && status.signal() == Some(15) {
                return;
            }
        }
        #[cfg(not(unix))]
        let _ = expected_sigterm;
        let message = if status.code() == Some(INPUT_QUIESCE_FAILED_EXIT_CODE) {
            format!("Native worker could not drain admitted input before shutdown ({status}); input cleanup is unconfirmed")
        } else {
            format!("Worker exited unexpectedly ({status}); input cleanup is unconfirmed")
        };
        let mut failure = self
            .shutdown_failure
            .lock()
            .expect("worker shutdown lock poisoned");
        // Exit is recorded once while consuming the owned Child. If a grace or
        // kill failure already exists, retain it and append the actual status.
        match failure.as_mut() {
            Some(previous) => {
                previous.push_str("; ");
                previous.push_str(&message);
            }
            None => *failure = Some(message),
        }
    }

    fn shutdown_result(&self) -> Result<(), TransportErrorKind> {
        match self
            .shutdown_failure
            .lock()
            .expect("worker shutdown lock poisoned")
            .clone()
        {
            Some(error) => Err(TransportErrorKind::Shutdown(error)),
            None => Ok(()),
        }
    }
}

struct Request {
    bytes: Vec<u8>,
    may_have_been_sent: Arc<AtomicBool>,
    reply: SyncSender<Result<Vec<u8>, TransportErrorKind>>,
}

/// A persistent worker with one admission slot. `call` is synchronous and must
/// run off the MCP event loop. `stop` may be called from a separate controller.
pub struct Worker {
    requests: Mutex<Option<SyncSender<Request>>>,
    io_thread: Mutex<Option<JoinHandle<()>>>,
    process: Arc<Process>,
    busy: AtomicBool,
    pid: u32,
}

impl Worker {
    pub fn spawn(program: &Path, args: &[&str]) -> Result<Self, TransportError> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| TransportError::local(TransportErrorKind::Spawn(error.to_string())))?;
        let pid = child.id();
        let mut stdin = child.stdin.take().expect("requested child stdin");
        let mut stdout =
            BufReader::with_capacity(8192, child.stdout.take().expect("requested child stdout"));
        let process = Arc::new(Process {
            child: Mutex::new(Some(child)),
            stopped: AtomicBool::new(false),
            shutdown_failure: Mutex::new(None),
        });
        let (sender, receiver) = mpsc::sync_channel::<Request>(1);
        let io_process = process.clone();
        let io_thread = thread::Builder::new()
            .name("dtw-native-io".into())
            .spawn(move || {
                serve_requests(&receiver, &io_process, &mut stdin, &mut stdout);
            })
            .map_err(|error| {
                let _ = process.stop();
                TransportError::local(TransportErrorKind::ThreadSpawn(error.to_string()))
            })?;
        Ok(Self {
            requests: Mutex::new(Some(sender)),
            io_thread: Mutex::new(Some(io_thread)),
            process,
            busy: AtomicBool::new(false),
            pid,
        })
    }

    /// The original PID is diagnostic data, not a reusable target identity.
    pub fn id(&self) -> u32 {
        self.pid
    }

    /// `control` is borrowed/caller-local: it need not be Send or 'static. It is
    /// polled before dispatch, during blocked writes/reads and before returning.
    /// A zero remaining budget is equivalent to StopReason::TimedOut.
    pub fn call(
        &self,
        request: &[u8],
        control: impl Fn() -> Result<Duration, StopReason>,
    ) -> Result<Vec<u8>, TransportError> {
        if request.len() > MAX_REQUEST_BYTES {
            return Err(TransportError::local(TransportErrorKind::RequestTooLarge));
        }
        if !valid_line(request) {
            return Err(TransportError::local(TransportErrorKind::InvalidRequest));
        }
        if self.process.stopped.load(Ordering::SeqCst) {
            return Err(TransportError::local(TransportErrorKind::Stopped));
        }
        if self
            .busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(TransportError::local(TransportErrorKind::Busy));
        }
        let _admission = Admission(self);
        let sent = Arc::new(AtomicBool::new(false));
        let fail = |kind| {
            // Preserve the primary transport/control failure and dispatch
            // uncertainty. Cleanup diagnostics remain persistent on Process and
            // are returned by stop/close, including after the child is reaped.
            let _ = self.stop();
            TransportError {
                kind,
                request_may_have_been_sent: sent.load(Ordering::SeqCst),
            }
        };
        remaining(&control).map_err(|reason| fail(TransportErrorKind::Control(reason)))?;
        let (reply, response) = mpsc::sync_channel(1);
        let mut bytes = request.to_vec();
        bytes.push(b'\n');
        let queued = {
            let sender = self.requests.lock().expect("worker request lock poisoned");
            sender.as_ref().map(|sender| {
                sender.try_send(Request {
                    bytes,
                    may_have_been_sent: sent.clone(),
                    reply,
                })
            })
        };
        match queued {
            Some(Ok(())) => {}
            Some(Err(TrySendError::Full(_))) | Some(Err(TrySendError::Disconnected(_))) | None => {
                return Err(fail(TransportErrorKind::Stopped));
            }
        }
        loop {
            let budget =
                remaining(&control).map_err(|reason| fail(TransportErrorKind::Control(reason)))?;
            match response.recv_timeout(budget.min(POLL)) {
                Ok(Ok(bytes)) => {
                    remaining(&control)
                        .map_err(|reason| fail(TransportErrorKind::Control(reason)))?;
                    if self.process.stopped.load(Ordering::SeqCst) {
                        return Err(fail(TransportErrorKind::Stopped));
                    }
                    return Ok(bytes);
                }
                Ok(Err(kind)) => return Err(fail(kind)),
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(fail(TransportErrorKind::UnexpectedEof))
                }
                Err(RecvTimeoutError::Timeout) => {
                    if self.process.stopped.load(Ordering::SeqCst) {
                        // The I/O thread publishes its error before retiring the
                        // process. It may have done so just after recv timed out.
                        if let Ok(Err(kind)) = response.try_recv() {
                            return Err(fail(kind));
                        }
                        return Err(fail(TransportErrorKind::Stopped));
                    }
                }
            }
        }
    }

    /// Read an already-recorded cleanup failure without reaping or touching IO.
    pub fn cleanup_error(&self) -> Option<TransportError> {
        self.process
            .shutdown_result()
            .err()
            .map(TransportError::local)
    }

    /// Permanently retire this instance and reap its owned child. Unix workers
    /// receive a bounded SIGTERM grace before the forced-kill fallback. Reader/writer
    /// thread joins are bounded; an inherited pipe held by an unrelated process
    /// cannot make this control path join forever.
    pub fn stop(&self) -> Result<(), TransportError> {
        self.requests
            .lock()
            .expect("worker request lock poisoned")
            .take();
        let result = self.process.stop().map_err(TransportError::local);
        let handle = self
            .io_thread
            .lock()
            .expect("worker thread lock poisoned")
            .take();
        if let Some(handle) = handle {
            let deadline = Instant::now() + THREAD_CLEANUP_BUDGET;
            while !handle.is_finished() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(1));
            }
            if handle.is_finished() {
                let _ = handle.join();
            }
            // Otherwise detach. The instance is retired and its child has been
            // killed; no further request can reach this thread.
        }
        result
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

struct Admission<'a>(&'a Worker);
impl Drop for Admission<'_> {
    fn drop(&mut self) {
        if thread::panicking() {
            let _ = self.0.stop();
        }
        self.0.busy.store(false, Ordering::SeqCst);
    }
}

fn remaining(control: &impl Fn() -> Result<Duration, StopReason>) -> Result<Duration, StopReason> {
    match control()? {
        duration if duration.is_zero() => Err(StopReason::TimedOut),
        duration => Ok(duration),
    }
}

fn valid_line(bytes: &[u8]) -> bool {
    !bytes.is_empty()
        && !bytes.iter().any(|byte| matches!(byte, b'\n' | b'\r' | 0))
        && std::str::from_utf8(bytes).is_ok()
}

fn serve_requests(
    requests: &Receiver<Request>,
    process: &Process,
    stdin: &mut impl Write,
    stdout: &mut impl BufRead,
) {
    while !process.stopped.load(Ordering::SeqCst) {
        let request = match requests.recv_timeout(POLL) {
            Ok(request) => request,
            Err(RecvTimeoutError::Timeout) => {
                if process.reap_if_exited() {
                    break;
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };
        if process.stopped.load(Ordering::SeqCst) {
            break;
        }
        // Set before the first possible write, including a partial write or a
        // broken pipe. A controller can conservatively classify interruption.
        request.may_have_been_sent.store(true, Ordering::SeqCst);
        let result = stdin
            .write_all(&request.bytes)
            .and_then(|_| stdin.flush())
            .map_err(|error| TransportErrorKind::Write(error.to_string()))
            .and_then(|_| read_frame(stdout));
        let failed = result.is_err();
        // A single result fits the single-slot channel, or the caller already
        // dropped it. This send never waits behind another response.
        let _ = request.reply.send(result);
        // Publish the actual failure before marking the worker stopped, so a
        // controller polling concurrently cannot replace it with bare Stopped.
        if failed {
            let _ = process.stop();
            break;
        }
    }
}

fn read_frame(reader: &mut impl BufRead) -> Result<Vec<u8>, TransportErrorKind> {
    let mut frame = Vec::new();
    loop {
        let available = reader
            .fill_buf()
            .map_err(|error| TransportErrorKind::Read(error.to_string()))?;
        if available.is_empty() {
            return Err(if frame.is_empty() {
                TransportErrorKind::UnexpectedEof
            } else {
                TransportErrorKind::MalformedFrame
            });
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let length = newline.unwrap_or(available.len());
        // No pipelining is allowed. Reject extra bytes already delivered with
        // this response rather than saving another frame for the next request.
        // A delayed extra line still requires adapter-level request identity.
        if newline.is_some() && length + 1 != available.len() {
            return Err(TransportErrorKind::MalformedFrame);
        }
        if length > MAX_RESPONSE_BYTES - frame.len() {
            return Err(TransportErrorKind::ResponseTooLarge);
        }
        frame.extend_from_slice(&available[..length]);
        reader.consume(length + usize::from(newline.is_some()));
        if newline.is_some() {
            if !valid_line(&frame) {
                return Err(TransportErrorKind::MalformedFrame);
            }
            return Ok(frame);
        }
    }
}
