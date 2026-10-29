use super::*;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

struct Fixture {
    path: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dtw-ipc-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir(&path).unwrap();
        Self { path }
    }

    fn worker(&self, mode: &str) -> Worker {
        let python = std::env::var_os("DTW_TEST_PYTHON").unwrap_or_else(|| {
            if cfg!(windows) {
                "python".into()
            } else {
                "python3".into()
            }
        });
        let ready = self.path.join("ready");
        Worker::spawn(
            Path::new(&python),
            &[
                "-u",
                "-c",
                include_str!("process_fixture.py"),
                mode,
                ready.to_str().unwrap(),
            ],
        )
        .unwrap()
    }

    fn wait_ready(&self) {
        self.wait_file("ready");
    }

    fn wait_file(&self, name: &str) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !self.path.join(name).exists() {
            assert!(
                Instant::now() < deadline,
                "worker did not reach fixture handshake"
            );
            thread::sleep(Duration::from_millis(2));
        }
    }
}

#[cfg(unix)]
#[test]
fn termination_drains_an_admitted_pair_once_before_reaping() {
    let fixture = Fixture::new();
    let worker = Arc::new(fixture.worker("graceful_input"));
    let calling_worker = worker.clone();
    let caller = thread::spawn(move || calling_worker.call(b"{}", budget()));
    fixture.wait_ready();
    let stopping_worker = worker.clone();
    let stopper = thread::spawn(move || stopping_worker.stop());
    fixture.wait_file("closing");
    assert_eq!(
        fs::read_to_string(fixture.path.join("events")).unwrap(),
        "down\n"
    );
    let late = worker.call(b"{}", budget()).unwrap_err();
    assert!(!late.request_may_have_been_sent);
    fs::write(fixture.path.join("release"), "release").unwrap();
    stopper.join().unwrap().unwrap();
    let original = caller.join().unwrap().unwrap_err();
    assert!(original.request_may_have_been_sent);
    assert_eq!(
        fs::read_to_string(fixture.path.join("events")).unwrap(),
        "down\nup\n"
    );
    assert_reaped(&worker);
}

#[cfg(unix)]
#[test]
fn ignored_termination_has_a_bounded_forced_kill_and_reports_uncertain_cleanup() {
    let fixture = Fixture::new();
    let worker = Arc::new(fixture.worker("ignore_term"));
    let calling_worker = worker.clone();
    let caller = thread::spawn(move || calling_worker.call(b"{}", budget()));
    fixture.wait_ready();
    let began = Instant::now();
    let error = worker.stop().unwrap_err();
    assert!(
        matches!(error.kind, TransportErrorKind::Shutdown(ref text) if text.contains("forced termination"))
    );
    assert!(began.elapsed() >= TERMINATION_GRACE);
    assert!(began.elapsed() < Duration::from_secs(2));
    assert!(
        caller
            .join()
            .unwrap()
            .unwrap_err()
            .request_may_have_been_sent
    );
    assert_reaped(&worker);
    // Reaping does not erase an earlier failure to confirm native cleanup.
    assert_eq!(worker.stop().unwrap_err().kind, error.kind);
    assert_eq!(worker.cleanup_error(), Some(error));
}

#[test]
fn failed_native_drain_survives_idle_reaping_and_repeated_stop() {
    let fixture = Fixture::new();
    let worker = fixture.worker("cleanup_failed");
    fixture.wait_ready();
    let until = Instant::now() + Duration::from_secs(5);
    while !worker.process.stopped.load(Ordering::SeqCst) {
        assert!(Instant::now() < until);
        thread::sleep(Duration::from_millis(2));
    }
    assert_reaped(&worker);
    for _ in 0..2 {
        let error = worker.stop().unwrap_err();
        assert!(
            matches!(error.kind, TransportErrorKind::Shutdown(ref text) if text.contains("drain admitted input"))
        );
        assert_eq!(worker.cleanup_error(), Some(error));
    }
}

fn wait_for_idle_reaping(worker: &Worker) {
    let until = Instant::now() + Duration::from_secs(5);
    while !worker.process.stopped.load(Ordering::SeqCst) {
        assert!(Instant::now() < until, "idle child was not reaped");
        thread::sleep(Duration::from_millis(2));
    }
    assert_reaped(worker);
}

fn assert_cleanup_failure_persists(worker: &Worker, status: &str) {
    let first = worker.stop().unwrap_err();
    assert!(
        matches!(first.kind, TransportErrorKind::Shutdown(ref text)
        if text.contains(status) && text.contains("input cleanup is unconfirmed")),
        "{first:?}"
    );
    assert_eq!(worker.stop().unwrap_err().kind, first.kind);
}

#[test]
fn external_kill_is_not_silently_cleaned_up_by_idle_reaping() {
    let fixture = Fixture::new();
    let worker = fixture.worker("no_read");
    fixture.wait_ready();
    // Bypass Process.stop: this is an external kill from the supervisor's
    // perspective, not a requested graceful termination.
    worker
        .process
        .child
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .kill()
        .unwrap();
    wait_for_idle_reaping(&worker);
    #[cfg(unix)]
    assert_cleanup_failure_persists(&worker, "signal: 9");
    #[cfg(not(unix))]
    assert_cleanup_failure_persists(&worker, "exit code:");
}

#[cfg(unix)]
#[test]
fn only_supervisor_requested_sigterm_is_expected_cleanup() {
    let fixture = Fixture::new();
    let worker = fixture.worker("no_read");
    fixture.wait_ready();
    {
        let child = worker.process.child.lock().unwrap();
        request_termination(child.as_ref().unwrap()).unwrap();
    }
    wait_for_idle_reaping(&worker);
    assert_cleanup_failure_persists(&worker, "signal: 15");

    let requested_fixture = Fixture::new();
    let requested = requested_fixture.worker("no_read");
    requested_fixture.wait_ready();
    requested.stop().unwrap();
    assert_reaped(&requested);
    requested.stop().unwrap();
}

#[test]
fn nonzero_exit_survives_idle_reaping_and_repeated_stop() {
    let fixture = Fixture::new();
    let worker = fixture.worker("idle_nonzero");
    fixture.wait_ready();
    wait_for_idle_reaping(&worker);
    assert_cleanup_failure_persists(&worker, "17");
}

#[test]
fn in_flight_nonzero_exit_preserves_primary_failure_and_cleanup_diagnostic() {
    for (mode, status) in [("nonzero", "17"), ("nonzero_one", "1")] {
        let fixture = Fixture::new();
        let worker = fixture.worker(mode);
        let error = worker.call(b"{}", budget()).unwrap_err();
        assert_eq!(error.kind, TransportErrorKind::UnexpectedEof);
        assert!(error.request_may_have_been_sent);
        assert_reaped(&worker);
        assert_cleanup_failure_persists(&worker, status);
    }
}

#[test]
fn natural_exit_between_initial_poll_and_kill_keeps_the_actual_status() {
    for (mode, code) in [("gated_nonzero", 17), ("gated_exit_one", 1)] {
        let fixture = Fixture::new();
        let worker = fixture.worker(mode);
        // Hold the process owner lock so the idle reaper cannot consume exit.
        // Explicitly enter the same post-try_wait(None) fallback used by stop,
        // then make the child finish before kill: kill may still return Ok.
        let failure = {
            let mut slot = worker.process.child.lock().unwrap();
            let child = slot.as_mut().unwrap();
            fixture.wait_ready();
            assert!(child.try_wait().unwrap().is_none());
            fs::write(fixture.path.join("release"), "release").unwrap();
            let until = Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(status) = child.try_wait().unwrap() {
                    assert_eq!(status.code(), Some(code));
                    break;
                }
                assert!(Instant::now() < until, "owned fixture did not exit");
                thread::sleep(Duration::from_millis(2));
            }
            worker.process.force_stop(&mut slot).unwrap_err()
        };
        assert_eq!(worker.stop().unwrap_err().kind, failure);
        assert_reaped(&worker);
        assert_cleanup_failure_persists(&worker, &code.to_string());
    }
}

#[cfg(unix)]
#[test]
fn external_reaper_os_error_is_a_persistent_cleanup_failure() {
    let fixture = Fixture::new();
    let worker = fixture.worker("no_read");
    fixture.wait_ready();
    {
        // A competing waitpid-based reaper is a real embedding failure: it can
        // consume our child without updating std::process::Child's cached state.
        // Hold the ownership lock so the fixture reaps first, deterministically.
        let mut child = worker.process.child.lock().unwrap();
        child.as_mut().unwrap().kill().unwrap();
        unsafe extern "C" {
            fn waitpid(
                pid: std::ffi::c_int,
                status: *mut std::ffi::c_int,
                options: std::ffi::c_int,
            ) -> std::ffi::c_int;
        }
        let mut status = 0;
        assert_eq!(
            unsafe { waitpid(worker.id() as std::ffi::c_int, &mut status, 0) },
            worker.id() as std::ffi::c_int
        );
    }
    let first = worker.stop().unwrap_err();
    assert!(
        matches!(first.kind, TransportErrorKind::Shutdown(ref message)
        if message.contains("Worker cleanup failed:") && message.contains("input cleanup is unconfirmed"))
    );
    // Check the durable diagnostic, independently of another failed OS call.
    assert_eq!(worker.process.shutdown_result(), Err(first.kind.clone()));
    assert_eq!(worker.cleanup_error(), Some(first.clone()));
    assert_eq!(worker.stop().unwrap_err().kind, first.kind);
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn budget() -> impl Fn() -> Result<Duration, StopReason> {
    let deadline = Instant::now() + Duration::from_secs(5);
    move || {
        deadline
            .checked_duration_since(Instant::now())
            .ok_or(StopReason::TimedOut)
    }
}

fn assert_reaped(worker: &Worker) {
    assert!(worker.process.stopped.load(Ordering::SeqCst));
    assert!(worker.process.child.lock().unwrap().is_none());
    let late = worker.call(b"{}", budget()).unwrap_err();
    assert_eq!(late.kind, TransportErrorKind::Stopped);
    assert!(!late.request_may_have_been_sent);
}

fn assert_requested_fixture_stop(result: Result<(), TransportError>) {
    // An idle echo fixture can observe stdin EOF when the IO thread retires
    // and exit zero before the supervisor kills it. Live fixtures may instead
    // require forced termination; Windows must retain that nonzero status.
    #[cfg(target_os = "windows")]
    if let Err(error) = result {
        // Generic Python fixtures do not expose the native cooperative stop
        // event. The owned kill's nonzero exit retains cleanup uncertainty.
        assert!(
            matches!(error.kind, TransportErrorKind::Shutdown(ref text)
            if text.contains("(exit code: 1)") && text.contains("input cleanup is unconfirmed")),
            "{error:?}"
        );
    }
    #[cfg(not(target_os = "windows"))]
    result.unwrap();
}

#[cfg(target_os = "windows")]
#[test]
fn forced_generic_stop_retains_cleanup_uncertainty() {
    let fixture = Fixture::new();
    let worker = fixture.worker("no_read");
    fixture.wait_ready();
    // This fixture never consumes stdin, so EOF cannot produce a clean exit.
    let failure = worker.stop().unwrap_err();
    assert!(matches!(failure.kind, TransportErrorKind::Shutdown(_)));
    assert_reaped(&worker);
    assert_cleanup_failure_persists(&worker, "exit code: 1");
}

#[test]
fn persistent_worker_exchanges_frames_without_restart() {
    let fixture = Fixture::new();
    let worker = fixture.worker("echo");
    let pid = worker.id();
    assert_eq!(
        worker.call(br#"{"request":1}"#, budget()).unwrap(),
        br#"{"request":1}"#
    );
    assert_eq!(
        worker.call(br#"{"request":2}"#, budget()).unwrap(),
        br#"{"request":2}"#
    );
    assert_eq!(worker.id(), pid);
    assert_requested_fixture_stop(worker.stop());
    assert_reaped(&worker);
}

#[test]
fn spawn_and_local_rejections_never_claim_dispatch() {
    let missing = std::env::temp_dir().join("dtw-worker-does-not-exist-9f8576");
    let error = Worker::spawn(&missing, &[]).err().unwrap();
    assert!(matches!(error.kind, TransportErrorKind::Spawn(_)));
    assert!(!error.request_may_have_been_sent);
    let fixture = Fixture::new();
    let worker = fixture.worker("echo");
    for request in [&b""[..], &b"{}\n{}"[..], &b"\xff"[..]] {
        let error = worker.call(request, budget()).unwrap_err();
        assert_eq!(error.kind, TransportErrorKind::InvalidRequest);
        assert!(!error.request_may_have_been_sent);
    }
    let error = worker
        .call(&vec![b'x'; MAX_REQUEST_BYTES + 1], budget())
        .unwrap_err();
    assert_eq!(error.kind, TransportErrorKind::RequestTooLarge);
    assert!(!error.request_may_have_been_sent);
    assert_eq!(worker.call(b"{}", budget()).unwrap(), b"{}");
}

#[test]
fn cancellation_before_admission_reaps_without_writing() {
    let fixture = Fixture::new();
    let worker = fixture.worker("echo");
    let error = worker
        .call(b"{}", || Err(StopReason::Cancelled))
        .unwrap_err();
    assert_eq!(
        error.kind,
        TransportErrorKind::Control(StopReason::Cancelled)
    );
    assert!(!error.request_may_have_been_sent);
    assert_reaped(&worker);
}

#[test]
fn in_flight_cancel_is_singleflight_and_preserves_possible_dispatch() {
    let fixture = Fixture::new();
    let worker = Arc::new(fixture.worker("hang"));
    let cancelled = Arc::new(AtomicBool::new(false));
    let calling_worker = worker.clone();
    let calling_cancelled = cancelled.clone();
    let caller = thread::spawn(move || {
        calling_worker.call(b"{}", || {
            if calling_cancelled.load(Ordering::SeqCst) {
                Err(StopReason::Cancelled)
            } else {
                Ok(Duration::from_secs(5))
            }
        })
    });
    fixture.wait_ready();
    let rejected = worker.call(b"{}", budget()).unwrap_err();
    assert_eq!(rejected.kind, TransportErrorKind::Busy);
    assert!(!rejected.request_may_have_been_sent);
    cancelled.store(true, Ordering::SeqCst);
    let error = caller.join().unwrap().unwrap_err();
    assert_eq!(
        error.kind,
        TransportErrorKind::Control(StopReason::Cancelled)
    );
    assert!(error.request_may_have_been_sent);
    assert_reaped(&worker);
}

#[test]
fn timeout_controls_a_worker_that_does_not_read_stdin() {
    let fixture = Fixture::new();
    let worker = fixture.worker("no_read");
    fixture.wait_ready();
    let deadline = Instant::now() + Duration::from_millis(50);
    let started = Instant::now();
    let error = worker
        .call(&vec![b'x'; MAX_REQUEST_BYTES], || {
            deadline
                .checked_duration_since(Instant::now())
                .ok_or(StopReason::TimedOut)
        })
        .unwrap_err();
    assert_eq!(
        error.kind,
        TransportErrorKind::Control(StopReason::TimedOut)
    );
    assert!(error.request_may_have_been_sent);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_reaped(&worker);
}

#[test]
fn bad_frames_oversize_and_eof_retire_the_process() {
    for (mode, expected) in [
        ("oversize", TransportErrorKind::ResponseTooLarge),
        ("empty", TransportErrorKind::MalformedFrame),
        ("invalid_utf8", TransportErrorKind::MalformedFrame),
        ("multiple_frames", TransportErrorKind::MalformedFrame),
        ("partial_eof", TransportErrorKind::MalformedFrame),
        ("eof", TransportErrorKind::UnexpectedEof),
    ] {
        let fixture = Fixture::new();
        let worker = fixture.worker(mode);
        let error = worker.call(b"{}", budget()).unwrap_err();
        assert_eq!(error.kind, expected, "fixture: {mode}");
        assert!(error.request_may_have_been_sent);
        assert_reaped(&worker);
    }
}

#[test]
fn external_stop_interrupts_an_in_flight_request_and_is_idempotent() {
    let fixture = Fixture::new();
    let worker = Arc::new(fixture.worker("hang"));
    let calling_worker = worker.clone();
    let caller = thread::spawn(move || calling_worker.call(b"{}", budget()));
    fixture.wait_ready();
    assert_requested_fixture_stop(worker.stop());
    let error = caller.join().unwrap().unwrap_err();
    assert!(error.request_may_have_been_sent);
    assert_requested_fixture_stop(worker.stop());
    assert_reaped(&worker);
}

#[test]
fn concurrent_stops_and_cancellation_reap_without_waiting_for_each_other() {
    let fixture = Fixture::new();
    let worker = Arc::new(fixture.worker("hang"));
    let cancelled = Arc::new(AtomicBool::new(false));
    let calling_worker = worker.clone();
    let calling_cancelled = cancelled.clone();
    let caller = thread::spawn(move || {
        calling_worker.call(b"{}", || {
            if calling_cancelled.load(Ordering::SeqCst) {
                Err(StopReason::Cancelled)
            } else {
                Ok(Duration::from_secs(5))
            }
        })
    });
    fixture.wait_ready();

    let barrier = Arc::new(std::sync::Barrier::new(5));
    let (done, completions) = mpsc::channel();
    let mut stoppers = Vec::new();
    for _ in 0..4 {
        let stopping_worker = worker.clone();
        let stopping_barrier = barrier.clone();
        let stopping_done = done.clone();
        stoppers.push(thread::spawn(move || {
            stopping_barrier.wait();
            stopping_done.send(stopping_worker.stop()).unwrap();
        }));
    }
    cancelled.store(true, Ordering::SeqCst);
    barrier.wait();
    for _ in 0..4 {
        assert_requested_fixture_stop(completions.recv_timeout(Duration::from_secs(5)).unwrap());
    }
    for stopper in stoppers {
        stopper.join().unwrap();
    }
    let error = caller.join().unwrap().unwrap_err();
    assert!(error.request_may_have_been_sent);
    assert_reaped(&worker);
    let process = worker.process.clone();
    drop(worker);
    assert!(process.child.lock().unwrap().is_none());
}

#[test]
fn stop_does_not_need_the_callers_control_closure_to_return() {
    let fixture = Fixture::new();
    let worker = Arc::new(fixture.worker("hang"));
    let (entered, entry) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let calling_worker = worker.clone();
    let caller = thread::spawn(move || {
        let polls = std::cell::Cell::new(0);
        calling_worker.call(b"{}", || {
            polls.set(polls.get() + 1);
            if polls.get() == 2 {
                entered.send(()).unwrap();
                released.recv().unwrap();
            }
            Ok(Duration::from_secs(5))
        })
    });
    entry.recv_timeout(Duration::from_secs(5)).unwrap();
    fixture.wait_ready();
    let (done, completion) = mpsc::channel();
    let stopping_worker = worker.clone();
    let stopper = thread::spawn(move || {
        done.send(stopping_worker.stop()).unwrap();
    });
    let stopped_before_control_returned = completion.recv_timeout(Duration::from_secs(5));
    // Always release the caller, including when the assertion below will fail.
    release.send(()).unwrap();
    assert_requested_fixture_stop(stopped_before_control_returned.unwrap());
    stopper.join().unwrap();
    let error = caller.join().unwrap().unwrap_err();
    assert!(error.request_may_have_been_sent);
    assert_reaped(&worker);
}

#[test]
fn drop_reaps_the_worker_and_closes_the_request_channel() {
    let fixture = Fixture::new();
    let worker = fixture.worker("echo");
    assert_eq!(worker.call(b"{}", budget()).unwrap(), b"{}");
    let process = worker.process.clone();
    drop(worker);
    assert!(process.stopped.load(Ordering::SeqCst));
    assert!(process.child.lock().unwrap().is_none());
}

#[test]
fn an_idle_worker_exit_is_reaped_without_a_followup_call() {
    let fixture = Fixture::new();
    let worker = fixture.worker("idle_exit");
    fixture.wait_ready();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !worker.process.stopped.load(Ordering::SeqCst) {
        assert!(Instant::now() < deadline, "idle child was not reaped");
        thread::sleep(Duration::from_millis(2));
    }
    assert_reaped(&worker);
}

#[test]
fn control_can_borrow_non_send_state() {
    let fixture = Fixture::new();
    let worker = fixture.worker("echo");
    let polls = std::cell::Cell::new(0);
    assert_eq!(
        worker
            .call(b"{}", || {
                polls.set(polls.get() + 1);
                Ok(Duration::from_secs(5))
            })
            .unwrap(),
        b"{}"
    );
    assert!(polls.get() >= 3);
}

#[cfg(target_os = "windows")]
fn cooperative_windows_fixture(fixture: &Fixture, fail_cleanup: bool) -> Worker {
    let python = std::env::var_os("DTW_TEST_PYTHON").unwrap_or_else(|| "python".into());
    Worker::spawn_cooperative(
        Path::new(&python),
        &[
            "-u",
            "-c",
            r#"
import ctypes, os, pathlib, sys
from ctypes import wintypes
k=ctypes.WinDLL('kernel32', use_last_error=True)
k.CreateEventW.argtypes=[ctypes.c_void_p,wintypes.BOOL,wintypes.BOOL,wintypes.LPCWSTR]
k.CreateEventW.restype=wintypes.HANDLE
k.WaitForSingleObject.argtypes=[wintypes.HANDLE,wintypes.DWORD]
k.WaitForSingleObject.restype=wintypes.DWORD
h=k.CreateEventW(None,True,False,'Local\\DesktopWorld.NativeStop.'+str(os.getpid()))
if not h: raise ctypes.WinError(ctypes.get_last_error())
pathlib.Path(sys.argv[1]).write_text('ready')
if k.WaitForSingleObject(h,10000)!=0: os._exit(2)
os._exit(74 if sys.argv[2]=='fail' else 0)
"#,
            fixture.path.join("ready").to_str().unwrap(),
            if fail_cleanup { "fail" } else { "clean" },
        ],
    )
    .unwrap()
}

#[cfg(target_os = "windows")]
#[test]
fn generic_windows_forced_stop_preserves_cleanup_uncertainty() {
    let fixture = Fixture::new();
    let worker = Arc::new(fixture.worker("hang"));
    let calling_worker = worker.clone();
    let caller = thread::spawn(move || calling_worker.call(b"{}", budget()));
    fixture.wait_ready();
    let first = worker.stop().unwrap_err();
    assert!(
        matches!(first.kind, TransportErrorKind::Shutdown(ref text)
        if text.contains("(exit code: 1)") && text.contains("input cleanup is unconfirmed")),
        "{first:?}"
    );
    let original = caller.join().unwrap().unwrap_err();
    assert!(original.request_may_have_been_sent);
    assert_reaped(&worker);
    assert_eq!(worker.stop().unwrap_err(), first);
    assert_eq!(worker.cleanup_error(), Some(first));
}

#[cfg(target_os = "windows")]
#[test]
fn windows_native_stop_event_is_cooperative_and_reaped() {
    let fixture = Fixture::new();
    let worker = cooperative_windows_fixture(&fixture, false);
    fixture.wait_ready();
    worker.stop().unwrap();
    assert_eq!(worker.cleanup_error(), None);
    assert!(worker.process.child.lock().unwrap().is_none());
}

#[cfg(target_os = "windows")]
#[test]
fn windows_failed_native_drain_stays_visible_after_reaping() {
    let fixture = Fixture::new();
    let worker = cooperative_windows_fixture(&fixture, true);
    fixture.wait_ready();
    assert!(matches!(
        worker.stop().unwrap_err().kind,
        TransportErrorKind::Shutdown(_)
    ));
    assert!(worker.cleanup_error().is_some());
    assert!(worker.process.child.lock().unwrap().is_none());
    assert!(worker.stop().is_err());
}
