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
        let deadline = Instant::now() + Duration::from_secs(5);
        while !self.path.join("ready").exists() {
            assert!(
                Instant::now() < deadline,
                "worker did not reach fixture handshake"
            );
            thread::sleep(Duration::from_millis(2));
        }
    }
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
    worker.stop().unwrap();
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
    worker.stop().unwrap();
    let error = caller.join().unwrap().unwrap_err();
    assert!(error.request_may_have_been_sent);
    worker.stop().unwrap();
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
        completions
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
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
    stopped_before_control_returned.unwrap().unwrap();
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
