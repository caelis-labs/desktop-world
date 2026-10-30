use super::{ActionReceipt, Clock, Runtime};
use crate::computer::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[path = "scoped_text_tests.rs"]
mod scoped_text_tests;

// Time advances only when the test asks. Wall time is used solely by the
// synchronization watchdog, never to arrange an expiry/cancellation race.
#[derive(Clone)]
struct ManualClock(Arc<Mutex<Instant>>);

impl ManualClock {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(Instant::now())))
    }

    fn advance(&self, duration: Duration) {
        let mut now = self.0.lock().unwrap();
        *now += duration;
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Instant {
        *self.0.lock().unwrap()
    }
}

struct WorkerGate {
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}

impl WorkerGate {
    fn wait(self) {
        if self.entered.send(()).is_err() {
            return;
        }
        // Dropping the test-side release sender also unblocks the worker if an
        // assertion fails, so a failing test cannot leave a native stand-in stuck.
        let _ = self.release.recv();
    }
}

struct TestGate {
    entered: mpsc::Receiver<()>,
    release: mpsc::Sender<()>,
}

impl TestGate {
    fn pair() -> (WorkerGate, Self) {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        (
            WorkerGate {
                entered: entered_tx,
                release: release_rx,
            },
            Self {
                entered: entered_rx,
                release: release_tx,
            },
        )
    }

    fn wait_until_entered(&self) {
        self.entered
            .recv_timeout(Duration::from_secs(5))
            .expect("worker did not reach the controlled barrier");
    }

    fn release(self) {
        let _ = self.release.send(());
    }
}

struct NativeBinding {
    drops: Arc<AtomicUsize>,
    drop_gate: Option<WorkerGate>,
}

impl Drop for NativeBinding {
    fn drop(&mut self) {
        if let Some(gate) = self.drop_gate.take() {
            gate.wait();
        }
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

struct Model {
    provider: ProviderStamp,
    supports_observe: bool,
    supports_press: bool,
    pressed: bool,
    observed_target: Option<WindowTarget>,
    elements: Option<Vec<Element>>,
    observe_error: Option<Error>,
    press_outcome: Option<ActionOutcome>,
    observe_gate: Option<WorkerGate>,
    press_gate: Option<WorkerGate>,
    drop_gate: Option<WorkerGate>,
    panic_on_press: bool,
    shutdown_error: Option<Error>,
    cleanup_error: Option<Error>,
}

struct Shared {
    model: Mutex<Model>,
    observations: AtomicUsize,
    presses: AtomicUsize,
    effects: AtomicUsize,
    drops: Arc<AtomicUsize>,
}

#[derive(Clone)]
struct TestBackend(Arc<Shared>);

impl TestBackend {
    fn new() -> Self {
        Self(Arc::new(Shared {
            model: Mutex::new(Model {
                provider: ProviderStamp {
                    id: "semantic-test-provider".into(),
                    generation: 1,
                },
                supports_observe: true,
                supports_press: true,
                pressed: false,
                observed_target: None,
                elements: None,
                observe_error: None,
                press_outcome: None,
                observe_gate: None,
                press_gate: None,
                drop_gate: None,
                panic_on_press: false,
                shutdown_error: None,
                cleanup_error: None,
            }),
            observations: AtomicUsize::new(0),
            presses: AtomicUsize::new(0),
            effects: AtomicUsize::new(0),
            drops: Arc::new(AtomicUsize::new(0)),
        }))
    }

    fn edit(&self, change: impl FnOnce(&mut Model)) {
        change(&mut self.0.model.lock().unwrap());
    }

    fn counts(&self) -> (usize, usize, usize, usize) {
        (
            self.0.observations.load(Ordering::SeqCst),
            self.0.presses.load(Ordering::SeqCst),
            self.0.effects.load(Ordering::SeqCst),
            self.0.drops.load(Ordering::SeqCst),
        )
    }
}

impl Backend for TestBackend {
    type Binding = NativeBinding;
    fn cleanup_error(&self) -> Option<Error> {
        self.0.model.lock().unwrap().cleanup_error.clone()
    }
    fn shutdown(&self) -> Result<(), Error> {
        self.0
            .model
            .lock()
            .unwrap()
            .shutdown_error
            .clone()
            .map_or(Ok(()), Err)
    }

    fn stamp(&self) -> ProviderStamp {
        self.0.model.lock().unwrap().provider.clone()
    }

    fn supports(&self, capability: Capability) -> bool {
        let model = self.0.model.lock().unwrap();
        match capability {
            Capability::Observe => model.supports_observe,
            Capability::Press => model.supports_press,
            Capability::Windows => true,
            _ => false,
        }
    }

    fn observe(
        &self,
        target: &WindowTarget,
        _call: &CallContext<'_>,
    ) -> Result<BackendObservation<NativeBinding>, Error> {
        self.0.observations.fetch_add(1, Ordering::SeqCst);
        let (observed, gate) = {
            let mut model = self.0.model.lock().unwrap();
            if let Some(error) = model.observe_error.take() {
                return Err(error);
            }
            let elements = model.elements.clone().unwrap_or_else(|| {
                vec![Element {
                    id: ElementId(7),
                    label: if model.pressed { "on" } else { "off" }.into(),
                    role: "button".into(),
                    name: None,
                    value: None,
                    enabled: Some(true),
                    actions: vec!["press".into()],
                    can_press: true,
                }]
            });
            let observation = BackendObservation {
                target: model
                    .observed_target
                    .clone()
                    .unwrap_or_else(|| target.clone()),
                elements,
                image: None,
                discovery: None,
                binding: NativeBinding {
                    drops: self.0.drops.clone(),
                    drop_gate: model.drop_gate.take(),
                },
            };
            (observation, model.observe_gate.take())
        };
        if let Some(gate) = gate {
            gate.wait();
        }
        Ok(observed)
    }

    fn press(
        &self,
        _target: &WindowTarget,
        _binding: &NativeBinding,
        element: ElementId,
        _call: &CallContext<'_>,
    ) -> ActionOutcome {
        assert_eq!(element, ElementId(7));
        self.0.presses.fetch_add(1, Ordering::SeqCst);
        let (gate, outcome, panic_on_press) = {
            let mut model = self.0.model.lock().unwrap();
            (
                model.press_gate.take(),
                model.press_outcome.take().unwrap_or(ActionOutcome {
                    delivery: None,
                    dispatch: DispatchState::Dispatched,
                    evidence: EffectEvidence::Confirmed("button is on".into()),
                    error: None,
                }),
                model.panic_on_press,
            )
        };
        if let Some(gate) = gate {
            gate.wait();
        }
        assert!(!panic_on_press, "controlled backend panic");
        if outcome.dispatch != DispatchState::NotDispatched {
            self.0.model.lock().unwrap().pressed = true;
            self.0.effects.fetch_add(1, Ordering::SeqCst);
        }
        outcome
    }
}

type TestRuntime = Runtime<TestBackend, ManualClock>;

fn target() -> WindowTarget {
    WindowTarget {
        app: AppInstance {
            identity: AppIdentity("test.app".into()),
            pid: 42,
            generation: 3,
        },
        native_id: 101,
        generation: 5,
    }
}

fn runtime(backend: &TestBackend, clock: &ManualClock) -> Arc<TestRuntime> {
    Arc::new(Runtime::with_clock(
        backend.clone(),
        target().app,
        Duration::from_secs(3),
        clock.clone(),
    ))
}

fn fixture() -> (Arc<TestRuntime>, TestBackend, ManualClock) {
    let backend = TestBackend::new();
    let clock = ManualClock::new();
    let runtime = runtime(&backend, &clock);
    (runtime, backend, clock)
}

fn assert_rejected(receipt: ActionReceipt, expected: Error) {
    assert_eq!(receipt.outcome, ActionOutcome::rejected(expected));
    assert_eq!(receipt.provider_outcome, None);
    assert_eq!(receipt.interruption, None);
}

fn assert_interrupted(receipt: ActionReceipt, expected: Error, provider: ActionOutcome) {
    assert_eq!(receipt.outcome, ActionOutcome::unknown(expected.clone()));
    assert_eq!(receipt.provider_outcome, Some(provider));
    assert_eq!(receipt.interruption, Some(expected));
}

fn successful_press() -> ActionOutcome {
    ActionOutcome {
        delivery: None,
        dispatch: DispatchState::Dispatched,
        evidence: EffectEvidence::Confirmed("button is on".into()),
        error: None,
    }
}

#[test]
fn observe_press_reobserve_changes_real_backend_state_once() {
    let (runtime, backend, _) = fixture();
    let before = runtime.observe(&target()).unwrap();
    assert_eq!(before.elements[0].label, "off");
    let receipt = runtime.press(&target(), before.reference, ElementId(7));
    assert_eq!(receipt.provider_outcome, Some(successful_press()));
    assert_eq!(receipt.interruption, None);
    let outcome = receipt.outcome;
    assert_eq!(outcome.dispatch, DispatchState::Dispatched);
    assert_eq!(
        outcome.evidence,
        EffectEvidence::Confirmed("button is on".into())
    );
    assert_eq!(outcome.error, None);
    assert_eq!(backend.counts(), (1, 1, 1, 1));
    assert_rejected(
        runtime.press(&target(), before.reference, ElementId(7)),
        Error::StaleObservation,
    );
    let after = runtime.observe(&target()).unwrap();
    assert_eq!(after.elements[0].label, "on");
    assert_ne!(before.reference, after.reference);
    assert_eq!(backend.counts(), (2, 1, 1, 1));
}

#[test]
fn wrong_app_or_instance_never_dispatches_or_consumes_bound_reference() {
    let (runtime, backend, _) = fixture();
    let observation = runtime.observe(&target()).unwrap();
    let mut different_app = target();
    different_app.app.identity = AppIdentity("other.app".into());
    let mut different_pid = target();
    different_pid.app.pid += 1;
    let mut reused_pid = target();
    reused_pid.app.generation += 1;
    for wrong in [different_app, different_pid, reused_pid] {
        assert_eq!(runtime.observe(&wrong), Err(Error::WrongApp));
        assert_rejected(
            runtime.press(&wrong, observation.reference, ElementId(7)),
            Error::WrongApp,
        );
        assert_eq!(backend.counts(), (1, 0, 0, 0));
    }
    assert_eq!(
        runtime
            .press(&target(), observation.reference, ElementId(7))
            .outcome
            .error,
        None
    );
    assert_eq!(backend.counts(), (1, 1, 1, 1));
}

#[test]
fn wrong_window_and_window_generation_do_not_consume_valid_reference() {
    let (runtime, backend, _) = fixture();
    let observation = runtime.observe(&target()).unwrap();
    let mut sibling = target();
    sibling.native_id += 1;
    let mut replaced_window = target();
    replaced_window.generation += 1;
    for wrong in [sibling, replaced_window] {
        assert_rejected(
            runtime.press(&wrong, observation.reference, ElementId(7)),
            Error::TargetChanged,
        );
        assert_eq!(backend.counts(), (1, 0, 0, 0));
    }
    assert_eq!(
        runtime
            .press(&target(), observation.reference, ElementId(7))
            .outcome
            .error,
        None
    );
}

#[test]
fn observation_cannot_cross_contexts_even_with_same_backend_and_generation() {
    let (first, backend, clock) = fixture();
    let second = runtime(&backend, &clock);
    let a = first.observe(&target()).unwrap();
    let b = second.observe(&target()).unwrap();
    assert_ne!(a.reference, b.reference);
    assert_rejected(
        second.press(&target(), a.reference, ElementId(7)),
        Error::StaleObservation,
    );
    assert_rejected(
        first.press(&target(), b.reference, ElementId(7)),
        Error::StaleObservation,
    );
    assert_eq!(backend.counts(), (2, 0, 0, 0));
    assert_eq!(
        second
            .press(&target(), b.reference, ElementId(7))
            .outcome
            .error,
        None
    );
    assert_eq!(
        first
            .press(&target(), a.reference, ElementId(7))
            .outcome
            .error,
        None
    );
    assert_eq!(backend.counts(), (2, 2, 2, 2));
}

#[test]
fn provider_identity_and_generation_are_both_bound_to_observation() {
    for replace_identity in [true, false] {
        let (runtime, backend, _) = fixture();
        let observation = runtime.observe(&target()).unwrap();
        backend.edit(|model| {
            if replace_identity {
                model.provider.id = "replacement-provider".into();
            } else {
                model.provider.generation += 1;
            }
        });
        assert_rejected(
            runtime.press(&target(), observation.reference, ElementId(7)),
            Error::WrongProvider,
        );
        assert_eq!(backend.counts(), (1, 0, 0, 0));
        let fresh = runtime.observe(&target()).unwrap();
        assert_eq!(
            runtime
                .press(&target(), fresh.reference, ElementId(7))
                .outcome
                .error,
            None
        );
        assert_eq!(backend.counts(), (2, 1, 1, 2));
    }
}

#[test]
fn replacement_observation_releases_old_binding_and_stales_old_reference() {
    let (runtime, backend, _) = fixture();
    let old = runtime.observe(&target()).unwrap();
    let fresh = runtime.observe(&target()).unwrap();
    assert_eq!(backend.counts(), (2, 0, 0, 1));
    assert_rejected(
        runtime.press(&target(), old.reference, ElementId(7)),
        Error::StaleObservation,
    );
    assert_eq!(
        runtime
            .press(&target(), fresh.reference, ElementId(7))
            .outcome
            .error,
        None
    );
    assert_eq!(backend.counts(), (2, 1, 1, 2));
}

#[test]
fn backend_support_is_checked_before_dispatch_without_consuming_reference() {
    let (runtime, backend, _) = fixture();
    backend.edit(|model| model.supports_observe = false);
    assert_eq!(
        runtime.observe(&target()),
        Err(Error::UnsupportedCapability)
    );
    assert_eq!(backend.counts(), (0, 0, 0, 0));
    backend.edit(|model| model.supports_observe = true);
    let observation = runtime.observe(&target()).unwrap();
    backend.edit(|model| model.supports_press = false);
    assert_rejected(
        runtime.press(&target(), observation.reference, ElementId(7)),
        Error::UnsupportedCapability,
    );
    assert_eq!(backend.counts(), (1, 0, 0, 0));
    backend.edit(|model| model.supports_press = true);
    assert_eq!(
        runtime
            .press(&target(), observation.reference, ElementId(7))
            .outcome
            .error,
        None
    );
}

#[test]
fn observation_expiry_rejects_stale_input_without_expiring_the_app_context() {
    let (runtime, backend, clock) = fixture();
    let observation = runtime.observe(&target()).unwrap();
    clock.advance(Duration::from_secs(3));
    assert_rejected(
        runtime.press(&target(), observation.reference, ElementId(7)),
        Error::StaleObservation,
    );
    assert_eq!(backend.counts(), (1, 0, 0, 0));
    clock.advance(Duration::from_secs(30));
    let fresh = runtime.observe(&target()).unwrap();
    assert_eq!(
        runtime
            .press(&target(), fresh.reference, ElementId(7))
            .outcome
            .dispatch,
        DispatchState::Dispatched
    );
    assert_eq!(backend.counts(), (2, 1, 1, 2));
}

#[test]
fn element_validation_rejects_before_dispatch_without_consuming_reference() {
    let (runtime, backend, _) = fixture();
    backend.edit(|model| {
        model.elements = Some(vec![
            Element {
                id: ElementId(7),
                label: "button".into(),
                role: "button".into(),
                name: None,
                value: None,
                enabled: Some(true),
                actions: vec!["press".into()],
                can_press: true,
            },
            Element {
                id: ElementId(8),
                label: "label".into(),
                role: "static_text".into(),
                name: None,
                value: None,
                enabled: None,
                actions: vec![],
                can_press: false,
            },
        ]);
    });
    let observation = runtime.observe(&target()).unwrap();
    assert_rejected(
        runtime.press(&target(), observation.reference, ElementId(99)),
        Error::ElementNotFound,
    );
    assert_rejected(
        runtime.press(&target(), observation.reference, ElementId(8)),
        Error::ElementNotPressable,
    );
    assert_eq!(backend.counts(), (1, 0, 0, 0));
    assert_eq!(
        runtime
            .press(&target(), observation.reference, ElementId(7))
            .outcome
            .error,
        None
    );
}

#[test]
fn invalid_backend_observations_are_not_published_and_bindings_are_released() {
    for duplicate_elements in [false, true] {
        let (runtime, backend, _) = fixture();
        backend.edit(|model| {
            if duplicate_elements {
                let element = Element {
                    id: ElementId(7),
                    label: "duplicate".into(),
                    role: "button".into(),
                    name: None,
                    value: None,
                    enabled: Some(true),
                    actions: vec!["press".into()],
                    can_press: true,
                };
                model.elements = Some(vec![element.clone(), element]);
            } else {
                let mut wrong = target();
                wrong.native_id += 1;
                model.observed_target = Some(wrong);
            }
        });
        let expected = if duplicate_elements {
            Error::InvalidObservation
        } else {
            Error::TargetChanged
        };
        assert_eq!(runtime.observe(&target()), Err(expected));
        assert_eq!(backend.counts(), (1, 0, 0, 1));
        backend.edit(|model| {
            model.elements = None;
            model.observed_target = None;
        });
        let fresh = runtime.observe(&target()).unwrap();
        assert_eq!(
            runtime
                .press(&target(), fresh.reference, ElementId(7))
                .outcome
                .error,
            None
        );
    }
}

#[test]
fn observation_errors_release_admission_but_do_not_restore_retired_reference() {
    let (runtime, backend, _) = fixture();
    let old = runtime.observe(&target()).unwrap();
    backend.edit(|model| model.observe_error = Some(Error::BackendFailed("read failed".into())));
    assert_eq!(
        runtime.observe(&target()),
        Err(Error::BackendFailed("read failed".into()))
    );
    assert_rejected(
        runtime.press(&target(), old.reference, ElementId(7)),
        Error::StaleObservation,
    );
    assert_eq!(backend.counts(), (2, 0, 0, 1));
    let fresh = runtime.observe(&target()).unwrap();
    assert_eq!(
        runtime
            .press(&target(), fresh.reference, ElementId(7))
            .outcome
            .error,
        None
    );
}

#[test]
fn observation_replacement_rechecks_execution_budget_after_retired_binding_cleanup() {
    for expire_call in [false, true] {
        let (runtime, backend, clock) = fixture();
        let (worker_gate, gate) = TestGate::pair();
        backend.edit(|model| model.drop_gate = Some(worker_gate));
        let old = runtime.observe(&target()).unwrap();
        assert_eq!(old.elements[0].label, "off");
        let observing_runtime = runtime.clone();
        let observer = thread::spawn(move || observing_runtime.observe(&target()));
        gate.wait_until_entered();
        assert_eq!(runtime.observe(&target()), Err(Error::Busy));
        assert_eq!(backend.counts(), (1, 0, 0, 0));

        let mut canceller = None;
        let mut cancelled_before_cleanup = true;
        let expected = if expire_call {
            clock.advance(Duration::from_secs(15));
            Error::TimedOut
        } else {
            let (done_tx, done_rx) = mpsc::channel();
            let cancelling_runtime = runtime.clone();
            canceller = Some(thread::spawn(move || {
                cancelling_runtime.cancel().unwrap();
                let _ = done_tx.send(());
            }));
            cancelled_before_cleanup = done_rx.recv_timeout(Duration::from_secs(5)).is_ok();
            Error::Cancelled
        };
        let admission_before_cleanup = runtime.observe(&target());
        gate.release();
        let result = observer.join().unwrap();
        if let Some(canceller) = canceller {
            canceller.join().unwrap();
        }

        assert!(
            cancelled_before_cleanup,
            "retired binding cleanup blocked cancellation"
        );
        assert_eq!(admission_before_cleanup, Err(Error::Busy));
        assert_eq!(result, Err(expected.clone()));
        assert_eq!(backend.counts(), (1, 0, 0, 1));
        assert_rejected(
            runtime.press(&target(), old.reference, ElementId(7)),
            if expire_call {
                Error::StaleObservation
            } else {
                expected
            },
        );
        assert_eq!(backend.counts(), (1, 0, 0, 1));
    }
}

#[test]
fn backend_failures_and_uncertain_effects_are_preserved_without_retry() {
    let outcomes = [
        ActionOutcome::unknown(Error::TimedOut),
        ActionOutcome::rejected(Error::BackendFailed("no input sent".into())),
        ActionOutcome {
            delivery: None,
            dispatch: DispatchState::Dispatched,
            evidence: EffectEvidence::Mismatch {
                expected: "off".into(),
                observed: "on".into(),
            },
            error: None,
        },
    ];
    for expected in outcomes {
        let (runtime, backend, _) = fixture();
        let observation = runtime.observe(&target()).unwrap();
        backend.edit(|model| model.press_outcome = Some(expected.clone()));
        let receipt = runtime.press(&target(), observation.reference, ElementId(7));
        assert_eq!(receipt.outcome, expected);
        assert_eq!(receipt.provider_outcome, Some(expected.clone()));
        assert_eq!(receipt.interruption, None);
        assert_rejected(
            runtime.press(&target(), observation.reference, ElementId(7)),
            Error::StaleObservation,
        );
        let after = runtime.observe(&target()).unwrap();
        let effects = usize::from(expected.dispatch != DispatchState::NotDispatched);
        assert_eq!(
            after.elements[0].label,
            if effects == 1 { "on" } else { "off" }
        );
        assert_eq!(backend.counts(), (2, 1, effects, 1));
    }
}

#[test]
fn cancel_and_close_before_dispatch_are_terminal_and_drop_live_binding_once() {
    for close in [false, true] {
        let (runtime, backend, _) = fixture();
        let observation = runtime.observe(&target()).unwrap();
        let expected = if close {
            runtime.close().unwrap();
            Error::Closed
        } else {
            runtime.cancel().unwrap();
            Error::Cancelled
        };
        assert_rejected(
            runtime.press(&target(), observation.reference, ElementId(7)),
            expected.clone(),
        );
        assert_eq!(runtime.observe(&target()), Err(expected));
        runtime.cancel().unwrap();
        runtime.close().unwrap();
        assert_eq!(backend.counts(), (1, 0, 0, 1));
    }
}

#[test]
fn in_flight_press_is_exclusive_and_retains_native_binding() {
    let (runtime, backend, _) = fixture();
    let observation = runtime.observe(&target()).unwrap();
    let (worker_gate, gate) = TestGate::pair();
    backend.edit(|model| model.press_gate = Some(worker_gate));
    let worker_runtime = runtime.clone();
    let worker =
        thread::spawn(move || worker_runtime.press(&target(), observation.reference, ElementId(7)));
    gate.wait_until_entered();
    assert_eq!(runtime.observe(&target()), Err(Error::Busy));
    assert_rejected(
        runtime.press(&target(), observation.reference, ElementId(7)),
        Error::Busy,
    );
    assert_eq!(backend.counts(), (1, 1, 0, 0));
    gate.release();
    assert_eq!(worker.join().unwrap().outcome.error, None);
    assert_eq!(backend.counts(), (1, 1, 1, 1));
    assert_eq!(runtime.observe(&target()).unwrap().elements[0].label, "on");
}

#[test]
fn cancellation_or_close_during_dispatch_preserves_busy_and_late_unknown_effect() {
    for close in [false, true] {
        let (runtime, backend, clock) = fixture();
        let observation = runtime.observe(&target()).unwrap();
        let (worker_gate, gate) = TestGate::pair();
        backend.edit(|model| model.press_gate = Some(worker_gate));
        let worker_runtime = runtime.clone();
        let worker = thread::spawn(move || {
            worker_runtime.press(&target(), observation.reference, ElementId(7))
        });
        gate.wait_until_entered();
        let expected = if close {
            runtime.close().unwrap();
            Error::Closed
        } else {
            runtime.cancel().unwrap();
            Error::Cancelled
        };
        assert_eq!(runtime.observe(&target()), Err(Error::Busy));
        assert_rejected(
            runtime.press(&target(), observation.reference, ElementId(7)),
            Error::Busy,
        );
        assert_eq!(backend.counts(), (1, 1, 0, 0));
        gate.release();
        assert_interrupted(worker.join().unwrap(), expected.clone(), successful_press());
        assert_eq!(runtime.observe(&target()), Err(expected.clone()));
        assert_rejected(
            runtime.press(&target(), observation.reference, ElementId(7)),
            expected,
        );
        assert_eq!(backend.counts(), (1, 1, 1, 1));

        // A new execution context can see the actual
        // late effect, but cannot reuse the cancelled context's reference.
        let fresh_runtime = self::runtime(&backend, &clock);
        let after = fresh_runtime.observe(&target()).unwrap();
        assert_eq!(after.elements[0].label, "on");
        assert_rejected(
            fresh_runtime.press(&target(), observation.reference, ElementId(7)),
            Error::StaleObservation,
        );
        assert_eq!(backend.counts(), (2, 1, 1, 1));
    }
}

#[test]
fn expiry_or_provider_replacement_during_dispatch_keeps_late_success_unknown() {
    for expire_call in [false, true] {
        let (runtime, backend, clock) = fixture();
        let observation = runtime.observe(&target()).unwrap();
        let (worker_gate, gate) = TestGate::pair();
        backend.edit(|model| model.press_gate = Some(worker_gate));
        let worker_runtime = runtime.clone();
        let worker = thread::spawn(move || {
            worker_runtime.press(&target(), observation.reference, ElementId(7))
        });
        gate.wait_until_entered();
        let expected = if expire_call {
            clock.advance(Duration::from_secs(15));
            Error::TimedOut
        } else {
            backend.edit(|model| model.provider.generation += 1);
            Error::WrongProvider
        };
        gate.release();
        assert_interrupted(worker.join().unwrap(), expected, successful_press());
        assert_eq!(backend.counts(), (1, 1, 1, 1));
        assert!(runtime
            .press(&target(), observation.reference, ElementId(7))
            .outcome
            .error
            .is_some());
        assert_eq!(backend.counts(), (1, 1, 1, 1));
    }
}

#[test]
fn late_observations_are_rejected_after_cancel_expiry_or_provider_replacement() {
    for scenario in 0..4 {
        let (runtime, backend, clock) = fixture();
        let (worker_gate, gate) = TestGate::pair();
        backend.edit(|model| model.observe_gate = Some(worker_gate));
        let worker_runtime = runtime.clone();
        let worker = thread::spawn(move || worker_runtime.observe(&target()));
        gate.wait_until_entered();
        assert_eq!(runtime.observe(&target()), Err(Error::Busy));
        let expected = match scenario {
            0 => {
                runtime.cancel().unwrap();
                Error::Cancelled
            }
            1 => {
                clock.advance(Duration::from_secs(3));
                Error::StaleObservation
            }
            2 => {
                clock.advance(Duration::from_secs(15));
                Error::TimedOut
            }
            _ => {
                backend.edit(|model| model.provider.generation += 1);
                Error::WrongProvider
            }
        };
        assert_eq!(backend.counts(), (1, 0, 0, 0));
        gate.release();
        assert_eq!(worker.join().unwrap(), Err(expected));
        assert_eq!(backend.counts(), (1, 0, 0, 1));
    }
}

#[test]
fn rejected_observation_binding_cleanup_does_not_hold_runtime_lock() {
    let (runtime, backend, _) = fixture();
    let (worker_gate, gate) = TestGate::pair();
    backend.edit(|model| {
        let mut wrong = target();
        wrong.native_id += 1;
        model.observed_target = Some(wrong);
        model.drop_gate = Some(worker_gate);
    });
    let observing_runtime = runtime.clone();
    let observer = thread::spawn(move || observing_runtime.observe(&target()));
    gate.wait_until_entered();
    let during_cleanup = runtime.observe(&target());
    let (done_tx, done_rx) = mpsc::channel();
    let cancelling_runtime = runtime.clone();
    let canceller = thread::spawn(move || {
        cancelling_runtime.cancel().unwrap();
        let _ = done_tx.send(());
    });
    let cancel_finished_before_cleanup = done_rx.recv_timeout(Duration::from_secs(5)).is_ok();
    // Always release and join before asserting, including the regression case.
    gate.release();
    // Cancellation completed before the operation's cleanup finished, so this
    // context may no longer publish an observation or a normal read result.
    assert_eq!(observer.join().unwrap(), Err(Error::Cancelled));
    canceller.join().unwrap();
    assert_eq!(during_cleanup, Err(Error::Busy));
    assert!(
        cancel_finished_before_cleanup,
        "binding Drop held the runtime lock"
    );
    assert_eq!(backend.counts(), (1, 0, 0, 1));
}

#[test]
fn press_binding_cleanup_remains_exclusive_but_allows_cancellation() {
    let (runtime, backend, _) = fixture();
    let (worker_gate, gate) = TestGate::pair();
    backend.edit(|model| model.drop_gate = Some(worker_gate));
    let observation = runtime.observe(&target()).unwrap();
    let pressing_runtime = runtime.clone();
    let worker = thread::spawn(move || {
        pressing_runtime.press(&target(), observation.reference, ElementId(7))
    });
    gate.wait_until_entered();
    let observing_during_cleanup = runtime.observe(&target());
    let pressing_during_cleanup = runtime.press(&target(), observation.reference, ElementId(7));
    let counts_before_cleanup = backend.counts();
    let (done_tx, done_rx) = mpsc::channel();
    let cancelling_runtime = runtime.clone();
    let canceller = thread::spawn(move || {
        cancelling_runtime.cancel().unwrap();
        let _ = done_tx.send(());
    });
    let cancelled_before_cleanup = done_rx.recv_timeout(Duration::from_secs(5)).is_ok();
    gate.release();
    let receipt = worker.join().unwrap();
    canceller.join().unwrap();
    assert_eq!(observing_during_cleanup, Err(Error::Busy));
    assert_rejected(pressing_during_cleanup, Error::Busy);
    assert_eq!(counts_before_cleanup, (1, 1, 1, 0));
    assert!(
        cancelled_before_cleanup,
        "native cleanup held the runtime lock"
    );
    assert_interrupted(receipt, Error::Cancelled, successful_press());
    assert_eq!(backend.counts(), (1, 1, 1, 1));
}

#[test]
fn known_no_dispatch_is_preserved_even_when_context_is_interrupted() {
    for scenario in 0..4 {
        let (runtime, backend, clock) = fixture();
        let observation = runtime.observe(&target()).unwrap();
        let provider = ActionOutcome::rejected(Error::BackendFailed(
            "window ownership changed; no native input dispatched".into(),
        ));
        let (worker_gate, gate) = TestGate::pair();
        backend.edit(|model| {
            model.press_gate = Some(worker_gate);
            model.press_outcome = Some(provider.clone());
        });
        let worker_runtime = runtime.clone();
        let worker = thread::spawn(move || {
            worker_runtime.press(&target(), observation.reference, ElementId(7))
        });
        gate.wait_until_entered();
        let interruption = match scenario {
            0 => {
                runtime.cancel().unwrap();
                Error::Cancelled
            }
            1 => {
                runtime.close().unwrap();
                Error::Closed
            }
            2 => {
                clock.advance(Duration::from_secs(15));
                Error::TimedOut
            }
            _ => {
                backend.edit(|model| model.provider.generation += 1);
                Error::WrongProvider
            }
        };
        gate.release();
        let receipt = worker.join().unwrap();
        assert_eq!(receipt.outcome, provider);
        assert_eq!(receipt.provider_outcome, Some(provider));
        assert_eq!(receipt.interruption, Some(interruption));
        assert_eq!(backend.counts(), (1, 1, 0, 1));
        assert!(runtime
            .press(&target(), observation.reference, ElementId(7))
            .outcome
            .error
            .is_some());
        assert_eq!(backend.counts(), (1, 1, 0, 1));
    }
}

#[test]
fn interruption_preserves_provider_error_and_effect_evidence_without_retry() {
    let outcomes = [
        ActionOutcome::unknown(Error::TimedOut),
        ActionOutcome {
            delivery: None,
            dispatch: DispatchState::Dispatched,
            evidence: EffectEvidence::Mismatch {
                expected: "button off".into(),
                observed: "button on".into(),
            },
            error: Some(Error::BackendFailed("postcondition did not match".into())),
        },
    ];
    for provider in outcomes {
        let (runtime, backend, _) = fixture();
        let observation = runtime.observe(&target()).unwrap();
        let (worker_gate, gate) = TestGate::pair();
        backend.edit(|model| {
            model.press_gate = Some(worker_gate);
            model.press_outcome = Some(provider.clone());
        });
        let worker_runtime = runtime.clone();
        let worker = thread::spawn(move || {
            worker_runtime.press(&target(), observation.reference, ElementId(7))
        });
        gate.wait_until_entered();
        runtime.cancel().unwrap();
        gate.release();
        assert_interrupted(worker.join().unwrap(), Error::Cancelled, provider);
        assert_rejected(
            runtime.press(&target(), observation.reference, ElementId(7)),
            Error::Cancelled,
        );
        assert_eq!(backend.counts(), (1, 1, 1, 1));
    }
}

#[test]
fn dropping_runtime_releases_native_binding_once() {
    let (runtime, backend, _) = fixture();
    runtime.observe(&target()).unwrap();
    assert_eq!(backend.counts(), (1, 0, 0, 0));
    drop(runtime);
    assert_eq!(backend.counts(), (1, 0, 0, 1));
}

#[test]
fn dropping_runtime_cleans_native_binding_before_backend_resources() {
    struct OrderedBackend(Arc<Mutex<Vec<&'static str>>>);
    struct OrderedBinding(Arc<Mutex<Vec<&'static str>>>);

    impl Drop for OrderedBinding {
        fn drop(&mut self) {
            self.0.lock().unwrap().push("binding");
        }
    }

    impl Drop for OrderedBackend {
        fn drop(&mut self) {
            self.0.lock().unwrap().push("backend");
        }
    }

    impl Backend for OrderedBackend {
        type Binding = OrderedBinding;

        fn stamp(&self) -> ProviderStamp {
            ProviderStamp {
                id: "ordered-provider".into(),
                generation: 1,
            }
        }

        fn supports(&self, capability: Capability) -> bool {
            capability == Capability::Observe
        }

        fn observe(
            &self,
            target: &WindowTarget,
            _call: &CallContext<'_>,
        ) -> Result<BackendObservation<Self::Binding>, Error> {
            Ok(BackendObservation {
                target: target.clone(),
                elements: Vec::new(),
                image: None,
                discovery: None,
                binding: OrderedBinding(self.0.clone()),
            })
        }

        fn press(
            &self,
            _: &WindowTarget,
            _: &Self::Binding,
            _: ElementId,
            _: &CallContext<'_>,
        ) -> ActionOutcome {
            ActionOutcome::rejected(Error::UnsupportedCapability)
        }
    }

    let events = Arc::new(Mutex::new(Vec::new()));
    let clock = ManualClock::new();
    let runtime = Runtime::with_clock(
        OrderedBackend(events.clone()),
        target().app,
        Duration::from_secs(3),
        clock,
    );
    runtime.observe(&target()).unwrap();
    assert!(events.lock().unwrap().is_empty());
    // Exercise default ownership teardown, without cancel() or close() first.
    drop(runtime);
    assert_eq!(*events.lock().unwrap(), vec!["binding", "backend"]);
}

#[test]
fn backend_panic_releases_binding_and_closes_context_instead_of_retrying() {
    let (runtime, backend, _) = fixture();
    let observation = runtime.observe(&target()).unwrap();
    backend.edit(|model| model.panic_on_press = true);
    let panicking_runtime = runtime.clone();
    let worker = thread::spawn(move || {
        panicking_runtime.press(&target(), observation.reference, ElementId(7))
    });
    assert!(worker.join().is_err());
    assert_eq!(runtime.observe(&target()), Err(Error::Closed));
    assert_rejected(
        runtime.press(&target(), observation.reference, ElementId(7)),
        Error::Closed,
    );
    assert_eq!(backend.counts(), (1, 1, 0, 1));
}

// The pixel group shares admission and reference lifetime, never native geometry.
struct PixelBackend(TestBackend);
impl Backend for PixelBackend {
    type Binding = NativeBinding;
    fn stamp(&self) -> ProviderStamp {
        self.0.stamp()
    }
    fn supports(&self, _: Capability) -> bool {
        true
    }
    fn observe(
        &self,
        target: &WindowTarget,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<Self::Binding>, Error> {
        self.0.observe(target, call)
    }
    fn press(
        &self,
        target: &WindowTarget,
        binding: &Self::Binding,
        element: ElementId,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        self.0.press(target, binding, element, call)
    }
    fn capture(
        &self,
        target: &WindowTarget,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<Self::Binding>, Error> {
        let mut observed = self.0.observe(target, call)?;
        observed.elements.clear();
        observed.image = Some(ImageFrame {
            width: 100,
            height: 80,
            png: vec![1],
        });
        Ok(observed)
    }
    fn click(
        &self,
        target: &WindowTarget,
        binding: &Self::Binding,
        _: PixelPoint,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        self.0.press(target, binding, ElementId(7), call)
    }
    fn windows(&self, app: &AppInstance, call: &CallContext<'_>) -> Result<WindowInventory, Error> {
        call.remaining()?;
        Ok(test_inventory(vec![WindowInfo {
            target: WindowTarget {
                app: app.clone(),
                native_id: 99,
                generation: 1,
            },
            title: "test".into(),
            visible: true,
        }]))
    }
}
fn pixel_runtime(backend: TestBackend) -> Runtime<PixelBackend> {
    Runtime::new(PixelBackend(backend), target().app, Duration::from_secs(30))
}

struct PageBackend {
    inner: PixelBackend,
    pages: Mutex<std::collections::VecDeque<Result<BackendObservationPage, Error>>>,
    first: Mutex<DiscoveryPage>,
    inventory: Mutex<Result<WindowInventory, Error>>,
    next_gate: Mutex<Option<WorkerGate>>,
    continuation_budgets: Mutex<Vec<Duration>>,
    pressed_elements: Mutex<Vec<ElementId>>,
}

impl Backend for PageBackend {
    type Binding = NativeBinding;
    fn stamp(&self) -> ProviderStamp {
        self.inner.stamp()
    }
    fn supports(&self, _: Capability) -> bool {
        true
    }
    fn windows(&self, _: &AppInstance, call: &CallContext<'_>) -> Result<WindowInventory, Error> {
        call.remaining()?;
        self.inventory.lock().unwrap().clone()
    }
    fn observe(
        &self,
        target: &WindowTarget,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<Self::Binding>, Error> {
        self.inner.observe(target, call)
    }
    fn observe_page(
        &self,
        target: &WindowTarget,
        _: &ObserveQuery,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<Self::Binding>, Error> {
        let mut observed = self.inner.observe(target, call)?;
        observed.discovery = Some(self.first.lock().unwrap().clone());
        Ok(observed)
    }
    fn continue_observation(
        &self,
        _: &WindowTarget,
        _: &Self::Binding,
        call: &CallContext<'_>,
    ) -> Result<BackendObservationPage, Error> {
        self.continuation_budgets
            .lock()
            .unwrap()
            .push(call.remaining()?);
        let gate = self.next_gate.lock().unwrap().take();
        if let Some(gate) = gate {
            gate.wait();
        }
        self.pages
            .lock()
            .unwrap()
            .pop_front()
            .expect("controlled next page")
    }
    fn capture(
        &self,
        target: &WindowTarget,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<Self::Binding>, Error> {
        self.inner.capture(target, call)
    }
    fn press(
        &self,
        target: &WindowTarget,
        binding: &Self::Binding,
        element: ElementId,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        self.pressed_elements.lock().unwrap().push(element);
        self.inner.press(target, binding, ElementId(7), call)
    }
    fn click(
        &self,
        target: &WindowTarget,
        binding: &Self::Binding,
        point: PixelPoint,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        self.inner.click(target, binding, point, call)
    }
}

fn page(id: u64, visited: u32, has_more: bool) -> BackendObservationPage {
    BackendObservationPage {
        target: target(),
        elements: vec![Element {
            id: ElementId(id),
            label: "next button".into(),
            role: "button".into(),
            name: None,
            value: None,
            enabled: Some(true),
            actions: vec!["press".into()],
            can_press: true,
        }],
        discovery: DiscoveryPage {
            has_more,
            complete: !has_more,
            reason: None,
            visited,
        },
    }
}

fn page_fixture() -> (
    Arc<Runtime<PageBackend, ManualClock>>,
    TestBackend,
    ManualClock,
) {
    let model = TestBackend::new();
    let clock = ManualClock::new();
    let backend = PageBackend {
        inner: PixelBackend(model.clone()),
        pages: Mutex::new(Default::default()),
        first: Mutex::new(DiscoveryPage {
            has_more: true,
            complete: false,
            reason: None,
            visited: 1,
        }),
        inventory: Mutex::new(Ok(test_inventory(vec![WindowInfo {
            target: target(),
            title: "window".into(),
            visible: true,
        }]))),
        next_gate: Mutex::new(None),
        continuation_budgets: Mutex::new(Vec::new()),
        pressed_elements: Mutex::new(Vec::new()),
    };
    (
        Arc::new(Runtime::with_clock(
            backend,
            target().app,
            Duration::from_secs(15),
            clock.clone(),
        )),
        model,
        clock,
    )
}

#[test]
fn pages_retain_earlier_elements_across_capture_and_other_windows() {
    for selected in [7, 8] {
        let (runtime, model, clock) = page_fixture();
        let first = runtime.observe(&target()).unwrap();
        let pixels = runtime.capture(&target()).unwrap();
        let other = WindowTarget {
            native_id: 100,
            ..target()
        };
        let other_view = runtime.observe(&other).unwrap();
        runtime
            .backend
            .pages
            .lock()
            .unwrap()
            .push_back(Ok(page(8, 2, false)));
        let next = runtime
            .continue_observation_until(
                &target(),
                first.reference,
                clock.now() + Duration::from_secs(15),
            )
            .unwrap();
        assert_eq!(next.reference, first.reference);
        assert_eq!(
            next.elements.iter().map(|e| e.id).collect::<Vec<_>>(),
            [ElementId(8)]
        );
        assert_eq!(
            runtime.continue_observation_until(
                &target(),
                first.reference,
                clock.now() + Duration::from_secs(15)
            ),
            Err(Error::InvalidObservation)
        );
        assert_eq!(
            runtime
                .press(&target(), first.reference, ElementId(selected))
                .outcome
                .dispatch,
            DispatchState::Dispatched
        );
        assert_eq!(
            *runtime.backend.pressed_elements.lock().unwrap(),
            [ElementId(selected)]
        );
        assert_rejected(
            runtime.click(&target(), pixels.reference, PixelPoint { x: 1.0, y: 1.0 }),
            Error::StaleObservation,
        );
        assert_rejected(
            runtime.press(&target(), next.reference, ElementId(8)),
            Error::StaleObservation,
        );
        assert_eq!(
            runtime
                .press(&other, other_view.reference, ElementId(7))
                .outcome
                .dispatch,
            DispatchState::Dispatched
        );
        assert_eq!(model.counts(), (3, 2, 2, 3));
    }
}

#[test]
fn inventory_preserves_bindings_until_complete_target_loss_is_proven() {
    for outcome in 0..4 {
        let (runtime, model, _) = page_fixture();
        let observed = runtime.observe(&target()).unwrap();
        *runtime.backend.inventory.lock().unwrap() = match outcome {
            0 => Ok(test_inventory(vec![WindowInfo {
                target: target(),
                title: "same".into(),
                visible: true,
            }])),
            1 => Err(Error::InventoryUnproven(test_inventory(Vec::new()).report)),
            2 => Ok(test_inventory(Vec::new())),
            _ => Ok(test_inventory(vec![WindowInfo {
                target: WindowTarget {
                    generation: target().generation + 1,
                    ..target()
                },
                title: "reused".into(),
                visible: true,
            }])),
        };
        assert_eq!(runtime.windows().is_ok(), outcome != 1);
        let receipt = runtime.press(&target(), observed.reference, ElementId(7));
        if outcome < 2 {
            assert_eq!(receipt.outcome.dispatch, DispatchState::Dispatched);
        } else {
            assert_rejected(receipt, Error::StaleObservation);
            assert_eq!(model.counts().1, 0);
        }
        assert_eq!(model.counts().3, 1);
    }
}

#[test]
fn continuation_preserves_original_expiry_and_scan_limit_keeps_elements_actionable() {
    let (runtime, model, clock) = page_fixture();
    let first = runtime.observe(&target()).unwrap();
    clock.advance(Duration::from_secs(14));
    runtime
        .backend
        .pages
        .lock()
        .unwrap()
        .push_back(Ok(page(8, 2, false)));
    runtime
        .continue_observation_until(
            &target(),
            first.reference,
            clock.now() + Duration::from_secs(30),
        )
        .unwrap();
    assert_eq!(
        *runtime.backend.continuation_budgets.lock().unwrap(),
        [Duration::from_secs(1)]
    );
    clock.advance(Duration::from_secs(1));
    assert_rejected(
        runtime.press(&target(), first.reference, ElementId(7)),
        Error::StaleObservation,
    );
    assert_eq!(model.counts().1, 0);

    let (runtime, _, clock) = page_fixture();
    let first = runtime.observe(&target()).unwrap();
    let mut limited = page(8, super::MAX_QUERY_VISITED, false);
    limited.discovery.complete = false;
    limited.discovery.reason = Some("scan_limit".into());
    runtime.backend.pages.lock().unwrap().push_back(Ok(limited));
    let last = runtime
        .continue_observation_until(
            &target(),
            first.reference,
            clock.now() + Duration::from_secs(15),
        )
        .unwrap();
    assert!(!last.discovery.unwrap().complete);
    assert_eq!(
        runtime
            .press(&target(), first.reference, ElementId(8))
            .outcome
            .dispatch,
        DispatchState::Dispatched
    );
}

#[test]
fn malformed_or_failed_continuation_retires_only_its_query() {
    for case in 0..5 {
        let (runtime, model, clock) = page_fixture();
        let first = runtime.observe(&target()).unwrap();
        let pixels = runtime.capture(&target()).unwrap();
        let mut invalid = page(8, 2, false);
        match case {
            0 => invalid.elements[0].id = ElementId(7),
            1 => invalid.discovery.visited = 0,
            2 => invalid.target.generation += 1,
            3 => invalid.discovery.visited = super::MAX_QUERY_VISITED + 1,
            _ => {}
        }
        runtime
            .backend
            .pages
            .lock()
            .unwrap()
            .push_back(if case == 4 {
                Err(Error::BackendFailed("native traversal failed".into()))
            } else {
                Ok(invalid)
            });
        assert!(runtime
            .continue_observation_until(
                &target(),
                first.reference,
                clock.now() + Duration::from_secs(15)
            )
            .is_err());
        assert_rejected(
            runtime.press(&target(), first.reference, ElementId(7)),
            Error::StaleObservation,
        );
        assert_eq!(model.counts().3, 1);
        assert_eq!(
            runtime
                .click(&target(), pixels.reference, PixelPoint { x: 1.0, y: 1.0 })
                .outcome
                .dispatch,
            DispatchState::Dispatched
        );
        assert_eq!(
            runtime.backend.continuation_budgets.lock().unwrap().len(),
            1
        );
    }
}

#[test]
fn observation_slot_capacity_rejects_without_evicting_a_live_reference() {
    let (runtime, model, clock) = page_fixture();
    let first = runtime.observe(&target()).unwrap();
    for id in 1000..1031 {
        runtime
            .observe(&WindowTarget {
                native_id: id,
                ..target()
            })
            .unwrap();
    }
    assert!(
        matches!(runtime.observe(&WindowTarget { native_id: 200, ..target() }), Err(Error::NativeFailure { code, .. }) if code == "resource_limit")
    );
    assert_eq!(model.counts().0, 32);
    assert_eq!(model.counts().3, 0);
    assert_eq!(
        runtime
            .press(&target(), first.reference, ElementId(7))
            .outcome
            .dispatch,
        DispatchState::Dispatched
    );
    clock.advance(Duration::from_secs(15));
    runtime
        .observe(&WindowTarget {
            native_id: 200,
            ..target()
        })
        .unwrap();
    assert_eq!(model.counts().3, 32);
}

#[test]
fn query_validation_and_default_backend_never_fake_filtered_or_paged_discovery() {
    let (runtime, model, clock) = fixture();
    for query in [
        ObserveQuery {
            limit: 0,
            ..Default::default()
        },
        ObserveQuery {
            limit: 101,
            ..Default::default()
        },
        ObserveQuery {
            role: Some(String::new()),
            ..Default::default()
        },
        ObserveQuery {
            text: Some(String::new()),
            ..Default::default()
        },
        ObserveQuery {
            text: Some("x".repeat(257)),
            ..Default::default()
        },
    ] {
        assert!(
            matches!(runtime.observe_query_until(&target(), &query, clock.now() + Duration::from_secs(1)), Err(Error::NativeFailure { code, .. }) if code == "invalid_query")
        );
    }
    let query = ObserveQuery {
        role: Some("button".into()),
        ..Default::default()
    };
    assert_eq!(
        runtime.observe_query_until(&target(), &query, clock.now() + Duration::from_secs(1)),
        Err(Error::UnsupportedCapability)
    );
    assert_eq!(model.counts().0, 0);
    let mut elements = page(8, 2, false).elements;
    elements.push(page(9, 3, false).elements.pop().unwrap());
    model.edit(|state| state.elements = Some(elements));
    assert_eq!(
        runtime.observe_query_until(
            &target(),
            &ObserveQuery {
                limit: 1,
                ..Default::default()
            },
            clock.now() + Duration::from_secs(1)
        ),
        Err(Error::UnsupportedCapability)
    );
    assert_eq!(model.counts().3, 1);
}

#[test]
fn late_continuation_cannot_publish_after_cancel_expiry_or_provider_change() {
    for cause in 0..3 {
        let (runtime, model, clock) = page_fixture();
        let first = runtime.observe(&target()).unwrap();
        runtime
            .backend
            .pages
            .lock()
            .unwrap()
            .push_back(Ok(page(8, 2, false)));
        let (gate, control) = TestGate::pair();
        *runtime.backend.next_gate.lock().unwrap() = Some(gate);
        let active = runtime.clone();
        let deadline = clock.now() + Duration::from_secs(15);
        let worker = thread::spawn(move || {
            active.continue_observation_until(&target(), first.reference, deadline)
        });
        control.wait_until_entered();
        assert_eq!(runtime.capture(&target()), Err(Error::Busy));
        let expected = match cause {
            0 => {
                runtime.cancel().unwrap();
                Error::Cancelled
            }
            1 => {
                clock.advance(Duration::from_secs(15));
                Error::TimedOut
            }
            _ => {
                model.edit(|state| state.provider.generation += 1);
                Error::WrongProvider
            }
        };
        control.release();
        assert_eq!(worker.join().unwrap(), Err(expected));
        assert_eq!(model.counts().3, 1);
        assert_eq!(model.counts().1, 0);
    }
}

#[test]
fn failed_continuation_binding_cleanup_keeps_busy_but_does_not_hold_state_lock() {
    let (runtime, model, clock) = page_fixture();
    let (gate, control) = TestGate::pair();
    model.edit(|state| state.drop_gate = Some(gate));
    let first = runtime.observe(&target()).unwrap();
    runtime
        .backend
        .pages
        .lock()
        .unwrap()
        .push_back(Err(Error::BackendFailed("broken page".into())));
    let active = runtime.clone();
    let deadline = clock.now() + Duration::from_secs(15);
    let worker = thread::spawn(move || {
        active.continue_observation_until(&target(), first.reference, deadline)
    });
    control.wait_until_entered();
    assert_eq!(runtime.observe(&target()), Err(Error::Busy));
    runtime.cancel().unwrap();
    clock.advance(Duration::from_secs(15));
    control.release();
    assert_eq!(worker.join().unwrap(), Err(Error::Cancelled));
    assert_eq!(model.counts().3, 1);
}

#[test]
fn continuation_deadline_rejects_before_dispatch_and_shrinks_native_budget() {
    let (runtime, model, clock) = page_fixture();
    let first = runtime.observe(&target()).unwrap();
    assert_eq!(
        runtime.continue_observation_until(&target(), first.reference, clock.now()),
        Err(Error::TimedOut)
    );
    assert!(runtime
        .backend
        .continuation_budgets
        .lock()
        .unwrap()
        .is_empty());
    runtime
        .backend
        .pages
        .lock()
        .unwrap()
        .push_back(Ok(page(8, 2, false)));
    runtime
        .continue_observation_until(
            &target(),
            first.reference,
            clock.now() + Duration::from_millis(75),
        )
        .unwrap();
    assert_eq!(
        *runtime.backend.continuation_budgets.lock().unwrap(),
        [Duration::from_millis(75)]
    );
    assert_eq!(
        runtime
            .press(&target(), first.reference, ElementId(8))
            .outcome
            .dispatch,
        DispatchState::Dispatched
    );
    assert_eq!(model.counts().3, 1);
}

#[test]
fn retained_metadata_limit_rejects_a_page_and_releases_its_query() {
    let (runtime, model, clock) = page_fixture();
    let first = runtime.observe(&target()).unwrap();
    let mut large = page(8, 2, false);
    large.elements[0].label = "x".repeat(super::MAX_OBSERVATION_BYTES);
    runtime.backend.pages.lock().unwrap().push_back(Ok(large));
    assert!(
        matches!(runtime.continue_observation_until(&target(), first.reference, clock.now() + Duration::from_secs(1)), Err(Error::NativeFailure { code, .. }) if code == "resource_limit")
    );
    assert_rejected(
        runtime.press(&target(), first.reference, ElementId(7)),
        Error::StaleObservation,
    );
    assert_eq!(model.counts().3, 1);
    let next = runtime.observe(&target()).unwrap();
    assert_eq!(
        runtime
            .press(&target(), next.reference, ElementId(7))
            .outcome
            .dispatch,
        DispatchState::Dispatched
    );
}

#[test]
fn action_rechecks_deadline_after_retiring_the_other_observation_kind() {
    let (runtime, model, clock) = page_fixture();
    let (gate, control) = TestGate::pair();
    model.edit(|state| state.drop_gate = Some(gate));
    let pixels = runtime.capture(&target()).unwrap();
    let semantic = runtime.observe(&target()).unwrap();
    let active = runtime.clone();
    let deadline = clock.now() + Duration::from_secs(1);
    let worker = thread::spawn(move || {
        active.press_until(&target(), semantic.reference, ElementId(7), deadline)
    });
    control.wait_until_entered();
    assert_eq!(runtime.observe(&target()), Err(Error::Busy));
    clock.advance(Duration::from_secs(1));
    control.release();
    let receipt = worker.join().unwrap();
    assert_eq!(receipt.outcome, ActionOutcome::rejected(Error::TimedOut));
    assert_eq!(model.counts(), (2, 0, 0, 2));
    assert_rejected(
        runtime.click(&target(), pixels.reference, PixelPoint { x: 1.0, y: 1.0 }),
        Error::StaleObservation,
    );
}
#[test]
fn observation_kind_cannot_cross_semantic_and_pixel_actions() {
    let backend = TestBackend::new();
    let runtime = pixel_runtime(backend.clone());
    let pixels = runtime.capture(&target()).unwrap();
    assert_eq!(
        runtime
            .press(&target(), pixels.reference, ElementId(7))
            .outcome,
        ActionOutcome::rejected(Error::InvalidObservation)
    );
    assert_eq!(
        runtime
            .click(&target(), pixels.reference, PixelPoint { x: 50.0, y: 40.0 })
            .outcome
            .dispatch,
        DispatchState::Dispatched
    );
    assert_eq!(
        runtime
            .click(&target(), pixels.reference, PixelPoint { x: 50.0, y: 40.0 })
            .outcome,
        ActionOutcome::rejected(Error::StaleObservation)
    );
    let semantic = runtime.observe(&target()).unwrap();
    assert_eq!(
        runtime
            .click(&target(), semantic.reference, PixelPoint { x: 1.0, y: 1.0 })
            .outcome,
        ActionOutcome::rejected(Error::InvalidObservation)
    );
    assert_eq!(backend.counts().1, 1);
}
#[test]
fn pixel_bounds_are_rejected_before_dispatch_and_preserve_valid_reference() {
    let backend = TestBackend::new();
    let runtime = pixel_runtime(backend.clone());
    let pixels = runtime.capture(&target()).unwrap();
    assert!(runtime.retains_observation(pixels.reference));
    for point in [
        PixelPoint {
            x: f64::NAN,
            y: 0.0,
        },
        PixelPoint {
            x: 0.0,
            y: f64::INFINITY,
        },
        PixelPoint { x: -1.0, y: 0.0 },
        PixelPoint { x: 100.0, y: 0.0 },
        PixelPoint { x: 0.0, y: 80.0 },
    ] {
        assert_eq!(
            runtime.click(&target(), pixels.reference, point).outcome,
            ActionOutcome::rejected(Error::InvalidPoint)
        );
        assert!(runtime.retains_observation(pixels.reference));
    }
    assert_eq!(backend.counts().1, 0);
    assert_eq!(
        runtime
            .click(&target(), pixels.reference, PixelPoint { x: 0.0, y: 0.0 })
            .outcome
            .dispatch,
        DispatchState::Dispatched
    );
    assert!(!runtime.retains_observation(pixels.reference));
}
#[test]
fn complete_inventory_retires_a_missing_windows_pixel_observation() {
    let backend = TestBackend::new();
    let runtime = pixel_runtime(backend.clone());
    let pixels = runtime.capture(&target()).unwrap();
    assert_eq!(
        runtime.windows().unwrap().windows[0].target.app,
        target().app
    );
    assert_eq!(backend.counts().3, 1);
    assert_rejected(
        runtime.click(&target(), pixels.reference, PixelPoint { x: 1.0, y: 1.0 }),
        Error::StaleObservation,
    );
    assert_eq!(backend.counts().1, 0);
}
#[test]
fn semantic_read_preserves_pixels_until_either_action_consumes_the_window() {
    let backend = TestBackend::new();
    let runtime = pixel_runtime(backend.clone());
    let pixels = runtime.capture(&target()).unwrap();
    let semantic = runtime.observe(&target()).unwrap();
    assert_eq!(
        runtime
            .click(&target(), pixels.reference, PixelPoint { x: 1.0, y: 1.0 })
            .outcome
            .dispatch,
        DispatchState::Dispatched
    );
    assert_rejected(
        runtime.press(&target(), semantic.reference, ElementId(7)),
        Error::StaleObservation,
    );
    assert_eq!(backend.counts().3, 2);
}

#[test]
fn action_deadline_retains_the_late_provider_evidence_without_accepting_success() {
    let (runtime, backend, clock) = fixture();
    let observation = runtime.observe(&target()).unwrap();
    let (gate, control) = TestGate::pair();
    backend.0.model.lock().unwrap().press_gate = Some(gate);
    let active = runtime.clone();
    let worker =
        thread::spawn(move || active.press(&target(), observation.reference, ElementId(7)));
    control.wait_until_entered();
    clock.advance(Duration::from_secs(3));
    control.release();
    assert_interrupted(worker.join().unwrap(), Error::TimedOut, successful_press());
    assert_eq!(backend.counts().1, 1);
}

#[test]
fn cleanup_error_is_independent_of_native_receipts_and_has_no_side_effects() {
    let (runtime, backend, _) = fixture();
    assert_eq!(runtime.cleanup_error(), None);
    let observation = runtime.observe(&target()).unwrap();
    let native = ActionOutcome::unknown(Error::NativeFailure {
        code: "native_condition".into(),
        message: "original native failure".into(),
    });
    backend.edit(|model| model.press_outcome = Some(native.clone()));
    let receipt = runtime.press(&target(), observation.reference, ElementId(7));
    let counts = backend.counts();
    let cleanup = Error::BackendFailed("independent resource cleanup failure".into());
    backend.edit(|model| model.cleanup_error = Some(cleanup.clone()));
    assert_eq!(runtime.cleanup_error(), Some(cleanup.clone()));
    assert_eq!(runtime.cleanup_error(), Some(cleanup));
    assert_eq!(backend.counts(), counts);
    assert_eq!(receipt.outcome, native);
    assert_eq!(receipt.provider_outcome, Some(native));
    assert_eq!(receipt.interruption, None);
}

#[test]
fn shutdown_failure_is_reported_without_reopening_execution() {
    let (runtime, backend, _) = fixture();
    let observation = runtime.observe(&target()).unwrap();
    let failure = Error::BackendFailed("controlled stop failure".into());
    backend.0.model.lock().unwrap().shutdown_error = Some(failure.clone());
    assert_eq!(runtime.cancel(), Err(failure.clone()));
    assert_eq!(runtime.observe(&target()), Err(Error::Cancelled));
    assert_rejected(
        runtime.press(&target(), observation.reference, ElementId(7)),
        Error::Cancelled,
    );
    assert_eq!(runtime.close(), Err(failure));
    assert_eq!(runtime.windows(), Err(Error::Closed));
    assert_eq!(backend.counts(), (1, 0, 0, 1));
}

// Records the live budget at the native boundary. The optional gate models a
// blocking provider that polls CallContext while waiting for its response.
struct BudgetBackend {
    inner: PixelBackend,
    entries: AtomicUsize,
    budgets: Arc<Mutex<Vec<Duration>>>,
    wait: Mutex<Option<WorkerGate>>,
    windows_gate: Mutex<Option<WorkerGate>>,
}
impl BudgetBackend {
    fn new(inner: TestBackend) -> Self {
        Self {
            inner: PixelBackend(inner),
            entries: AtomicUsize::new(0),
            budgets: Arc::new(Mutex::new(Vec::new())),
            wait: Mutex::new(None),
            windows_gate: Mutex::new(None),
        }
    }
    fn enter(&self, call: &CallContext<'_>) -> Result<(), Error> {
        self.entries.fetch_add(1, Ordering::SeqCst);
        self.budgets.lock().unwrap().push(call.remaining()?);
        let gate = self.wait.lock().unwrap().take();
        if let Some(gate) = gate {
            gate.entered.send(()).unwrap();
            loop {
                let left = call.remaining()?;
                match gate
                    .release
                    .recv_timeout(left.min(Duration::from_millis(10)))
                {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
        }
        Ok(())
    }
}
impl Backend for BudgetBackend {
    type Binding = NativeBinding;
    fn stamp(&self) -> ProviderStamp {
        self.inner.stamp()
    }
    fn supports(&self, capability: Capability) -> bool {
        self.inner.supports(capability)
    }
    fn windows(&self, app: &AppInstance, call: &CallContext<'_>) -> Result<WindowInventory, Error> {
        self.enter(call)?;
        let gate = self.windows_gate.lock().unwrap().take();
        if let Some(gate) = gate {
            gate.wait();
        }
        Ok(test_inventory(vec![WindowInfo {
            target: WindowTarget {
                app: app.clone(),
                ..target()
            },
            title: "budget window".into(),
            visible: true,
        }]))
    }
    fn observe(
        &self,
        target: &WindowTarget,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<Self::Binding>, Error> {
        self.enter(call)?;
        self.inner.observe(target, call)
    }
    fn capture(
        &self,
        target: &WindowTarget,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<Self::Binding>, Error> {
        self.enter(call)?;
        self.inner.capture(target, call)
    }
    fn press(
        &self,
        target: &WindowTarget,
        binding: &Self::Binding,
        element: ElementId,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        if let Err(error) = self.enter(call) {
            return ActionOutcome::unknown(error);
        }
        self.inner.press(target, binding, element, call)
    }
    fn click(
        &self,
        target: &WindowTarget,
        binding: &Self::Binding,
        point: PixelPoint,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        if let Err(error) = self.enter(call) {
            return ActionOutcome::unknown(error);
        }
        self.inner.click(target, binding, point, call)
    }
}
fn budget_fixture() -> (
    Arc<Runtime<BudgetBackend, ManualClock>>,
    TestBackend,
    ManualClock,
) {
    let model = TestBackend::new();
    let clock = ManualClock::new();
    let runtime = Runtime::with_clock(
        BudgetBackend::new(model.clone()),
        target().app,
        Duration::from_secs(3),
        clock.clone(),
    );
    (Arc::new(runtime), model, clock)
}

#[test]
fn elapsed_call_deadlines_reject_all_operations_before_native_dispatch() {
    let (runtime, model, clock) = budget_fixture();
    let deadline = clock.now();
    assert_eq!(runtime.windows_until(deadline), Err(Error::TimedOut));
    assert_eq!(
        runtime.observe_until(&target(), deadline),
        Err(Error::TimedOut)
    );
    assert_eq!(
        runtime.capture_until(&target(), deadline),
        Err(Error::TimedOut)
    );
    assert_eq!(runtime.backend.entries.load(Ordering::SeqCst), 0);
    let semantic = runtime.observe(&target()).unwrap();
    assert_eq!(runtime.windows_until(deadline), Err(Error::TimedOut));
    assert_eq!(
        runtime.observe_until(&target(), deadline),
        Err(Error::TimedOut)
    );
    assert_eq!(
        runtime.capture_until(&target(), deadline),
        Err(Error::TimedOut)
    );
    assert_rejected(
        runtime.press_until(&target(), semantic.reference, ElementId(7), deadline),
        Error::TimedOut,
    );
    assert_eq!(model.counts().1, 0);
    assert_eq!(runtime.backend.entries.load(Ordering::SeqCst), 1);
    // An expired call budget neither consumes a valid reference nor expires the app context.
    assert_eq!(
        runtime
            .press(&target(), semantic.reference, ElementId(7))
            .outcome
            .dispatch,
        DispatchState::Dispatched
    );
    let pixels = runtime.capture(&target()).unwrap();
    assert_rejected(
        runtime.click_until(
            &target(),
            pixels.reference,
            PixelPoint { x: 1.0, y: 1.0 },
            deadline,
        ),
        Error::TimedOut,
    );
    assert_eq!(model.counts().1, 1);
    assert_eq!(runtime.backend.entries.load(Ordering::SeqCst), 3);
    assert_eq!(
        runtime
            .click(&target(), pixels.reference, PixelPoint { x: 1.0, y: 1.0 })
            .outcome
            .dispatch,
        DispatchState::Dispatched
    );
    assert_eq!(runtime.backend.entries.load(Ordering::SeqCst), 4);
}

#[test]
fn shared_call_deadline_shrinks_across_every_native_operation() {
    let (runtime, _, clock) = budget_fixture();
    let deadline = clock.now() + Duration::from_secs(1);
    runtime.windows_until(deadline).unwrap();
    clock.advance(Duration::from_millis(200));
    let semantic = runtime.observe_until(&target(), deadline).unwrap();
    clock.advance(Duration::from_millis(200));
    assert_eq!(
        runtime
            .press_until(&target(), semantic.reference, ElementId(7), deadline)
            .outcome
            .dispatch,
        DispatchState::Dispatched
    );
    clock.advance(Duration::from_millis(200));
    let pixels = runtime.capture_until(&target(), deadline).unwrap();
    clock.advance(Duration::from_millis(200));
    assert_eq!(
        runtime
            .click_until(
                &target(),
                pixels.reference,
                PixelPoint { x: 1.0, y: 1.0 },
                deadline
            )
            .outcome
            .dispatch,
        DispatchState::Dispatched
    );
    assert_eq!(
        *runtime.backend.budgets.lock().unwrap(),
        [1000, 800, 600, 400, 200].map(Duration::from_millis)
    );
}

#[test]
fn longer_call_deadline_cannot_extend_atomic_budget_or_observation_lifetime() {
    let (runtime, _, clock) = budget_fixture();
    let deadline = clock.now() + Duration::from_secs(60);
    runtime.windows_until(deadline).unwrap();
    let semantic = runtime.observe_until(&target(), deadline).unwrap();
    clock.advance(Duration::from_secs(2));
    assert_eq!(
        runtime
            .press_until(&target(), semantic.reference, ElementId(7), deadline)
            .outcome
            .dispatch,
        DispatchState::Dispatched
    );
    assert_eq!(
        *runtime.backend.budgets.lock().unwrap(),
        [15, 3, 1].map(Duration::from_secs)
    );
    let pixels = runtime.capture_until(&target(), deadline).unwrap();
    clock.advance(Duration::from_secs(3));
    assert_rejected(
        runtime.click_until(
            &target(),
            pixels.reference,
            PixelPoint { x: 1.0, y: 1.0 },
            deadline,
        ),
        Error::StaleObservation,
    );
    clock.advance(Duration::from_secs(5));
    assert!(runtime.windows_until(deadline).is_ok());
    assert!(runtime.observe_until(&target(), deadline).is_ok());
}

#[test]
fn deadline_is_rechecked_after_retired_binding_cleanup_before_all_reads() {
    for operation in 0..3 {
        let (runtime, model, clock) = budget_fixture();
        let (gate, control) = TestGate::pair();
        model.edit(|state| state.drop_gate = Some(gate));
        runtime.observe(&target()).unwrap();
        // Every read prunes expired bindings; a live different-kind binding is
        // now intentionally retained instead of being retired by a capture.
        clock.advance(Duration::from_secs(3));
        let deadline = clock.now() + Duration::from_secs(1);
        let active = runtime.clone();
        let worker = thread::spawn(move || match operation {
            0 => active.windows_until(deadline).map(|_| ()),
            1 => active.observe_until(&target(), deadline).map(|_| ()),
            _ => active.capture_until(&target(), deadline).map(|_| ()),
        });
        control.wait_until_entered();
        clock.advance(Duration::from_secs(1));
        assert_eq!(runtime.windows(), Err(Error::Busy));
        control.release();
        assert_eq!(worker.join().unwrap(), Err(Error::TimedOut));
        assert_eq!(runtime.backend.entries.load(Ordering::SeqCst), 1);
        assert_eq!(model.counts(), (1, 0, 0, 1));
        runtime.windows().unwrap();
    }
}

#[test]
fn native_read_replies_after_call_deadline_are_never_published() {
    for operation in 0..3 {
        let (runtime, model, clock) = budget_fixture();
        let prior = runtime.observe(&target()).unwrap();
        let (gate, control) = TestGate::pair();
        if operation == 0 {
            *runtime.backend.windows_gate.lock().unwrap() = Some(gate);
        } else {
            model.edit(|state| state.observe_gate = Some(gate));
        }
        let deadline = clock.now() + Duration::from_secs(1);
        let active = runtime.clone();
        let worker = thread::spawn(move || match operation {
            0 => active.windows_until(deadline).map(|_| ()),
            1 => active.observe_until(&target(), deadline).map(|_| ()),
            _ => active.capture_until(&target(), deadline).map(|_| ()),
        });
        control.wait_until_entered();
        clock.advance(Duration::from_secs(1));
        control.release();
        assert_eq!(worker.join().unwrap(), Err(Error::TimedOut));
        if operation == 1 {
            assert_rejected(
                runtime.press(&target(), prior.reference, ElementId(7)),
                Error::StaleObservation,
            );
        } else {
            assert_eq!(
                runtime
                    .press(&target(), prior.reference, ElementId(7))
                    .outcome
                    .dispatch,
                DispatchState::Dispatched
            );
        }
        assert_eq!(model.counts().3, if operation == 0 { 1 } else { 2 });
        runtime.observe(&target()).unwrap();
    }
}

#[test]
fn action_call_deadline_preserves_late_provider_evidence_and_known_no_dispatch() {
    for pixels in [false, true] {
        for provider in [
            successful_press(),
            ActionOutcome::unknown(Error::BackendFailed("late uncertain provider".into())),
            ActionOutcome::rejected(Error::TargetChanged),
        ] {
            let (runtime, model, clock) = budget_fixture();
            let observation = if pixels {
                runtime.capture(&target())
            } else {
                runtime.observe(&target())
            }
            .unwrap();
            let (gate, control) = TestGate::pair();
            model.edit(|state| {
                state.press_gate = Some(gate);
                state.press_outcome = Some(provider.clone());
            });
            let deadline = clock.now() + Duration::from_secs(1);
            let active = runtime.clone();
            let worker = thread::spawn(move || {
                if pixels {
                    active.click_until(
                        &target(),
                        observation.reference,
                        PixelPoint { x: 1.0, y: 1.0 },
                        deadline,
                    )
                } else {
                    active.press_until(&target(), observation.reference, ElementId(7), deadline)
                }
            });
            control.wait_until_entered();
            clock.advance(Duration::from_secs(1));
            control.release();
            let receipt = worker.join().unwrap();
            if provider.dispatch == DispatchState::NotDispatched {
                assert_eq!(receipt.outcome, provider);
                assert_eq!(receipt.provider_outcome, Some(provider));
                assert_eq!(receipt.interruption, Some(Error::TimedOut));
            } else {
                assert_interrupted(receipt, Error::TimedOut, provider);
            }
            assert_eq!(model.counts().1, 1);
            assert_rejected(
                runtime.press(&target(), observation.reference, ElementId(7)),
                Error::StaleObservation,
            );
        }
    }
}

#[test]
fn action_binding_cleanup_is_inside_the_call_deadline_and_busy_slot() {
    for pixels in [false, true] {
        let (runtime, model, clock) = budget_fixture();
        let (gate, control) = TestGate::pair();
        model.edit(|state| state.drop_gate = Some(gate));
        let observation = if pixels {
            runtime.capture(&target())
        } else {
            runtime.observe(&target())
        }
        .unwrap();
        let deadline = clock.now() + Duration::from_secs(1);
        let active = runtime.clone();
        let worker = thread::spawn(move || {
            if pixels {
                active.click_until(
                    &target(),
                    observation.reference,
                    PixelPoint { x: 1.0, y: 1.0 },
                    deadline,
                )
            } else {
                active.press_until(&target(), observation.reference, ElementId(7), deadline)
            }
        });
        control.wait_until_entered();
        clock.advance(Duration::from_secs(1));
        assert_eq!(runtime.windows(), Err(Error::Busy));
        control.release();
        assert_interrupted(worker.join().unwrap(), Error::TimedOut, successful_press());
        assert_eq!(model.counts(), (1, 1, 1, 1));
    }
}

#[test]
fn rejected_native_binding_cleanup_rechecks_call_deadline() {
    let (runtime, model, clock) = budget_fixture();
    let (gate, control) = TestGate::pair();
    model.edit(|state| {
        state.drop_gate = Some(gate);
        state.observed_target = Some(WindowTarget {
            native_id: 987,
            ..target()
        });
    });
    let deadline = clock.now() + Duration::from_secs(1);
    let active = runtime.clone();
    let worker = thread::spawn(move || active.observe_until(&target(), deadline));
    control.wait_until_entered();
    clock.advance(Duration::from_secs(1));
    assert_eq!(runtime.windows(), Err(Error::Busy));
    control.release();
    assert_eq!(worker.join().unwrap(), Err(Error::TimedOut));
    assert_eq!(model.counts(), (1, 0, 0, 1));
}

#[test]
fn call_deadline_interrupts_a_native_style_wait_without_context_expiry() {
    let model = TestBackend::new();
    let runtime = Arc::new(Runtime::new(
        BudgetBackend::new(model.clone()),
        target().app,
        Duration::from_secs(30),
    ));
    let observation = runtime.observe(&target()).unwrap();
    let (gate, control) = TestGate::pair();
    *runtime.backend.wait.lock().unwrap() = Some(gate);
    let active = runtime.clone();
    let (done_tx, done_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let receipt = active.press_until(
            &target(),
            observation.reference,
            ElementId(7),
            Instant::now() + Duration::from_millis(200),
        );
        done_tx.send(receipt).unwrap();
    });
    control.wait_until_entered();
    // No response is released: only the supplied budget can terminate the wait.
    let result = done_rx.recv_timeout(Duration::from_secs(2));
    control.release();
    worker.join().unwrap();
    assert_interrupted(
        result.expect("provider wait ignored the call deadline"),
        Error::TimedOut,
        ActionOutcome::unknown(Error::TimedOut),
    );
    assert_eq!(model.counts(), (1, 0, 0, 1));
    runtime.observe(&target()).unwrap();
}

#[derive(Clone)]
struct MultiInstanceBackend {
    inner: TestBackend,
    inventories: Arc<Mutex<Vec<(AppInstance, Result<WindowInventory, Error>)>>>,
    routed: Arc<Mutex<Vec<WindowTarget>>>,
    shutdowns: Arc<AtomicUsize>,
}
impl MultiInstanceBackend {
    fn new() -> Self {
        Self {
            inner: TestBackend::new(),
            inventories: Arc::new(Mutex::new(Vec::new())),
            routed: Arc::new(Mutex::new(Vec::new())),
            shutdowns: Arc::new(AtomicUsize::new(0)),
        }
    }
    fn set_inventory(&self, app: AppInstance, result: Result<WindowInventory, Error>) {
        let mut inventories = self.inventories.lock().unwrap();
        inventories.retain(|(member, _)| *member != app);
        inventories.push((app, result));
    }
}
impl Backend for MultiInstanceBackend {
    type Binding = NativeBinding;
    fn stamp(&self) -> ProviderStamp {
        self.inner.stamp()
    }
    fn supports(&self, _: Capability) -> bool {
        true
    }
    fn cleanup_error(&self) -> Option<Error> {
        self.inner.cleanup_error()
    }
    fn shutdown(&self) -> Result<(), Error> {
        self.shutdowns.fetch_add(1, Ordering::SeqCst);
        self.inner.shutdown()
    }
    fn windows(&self, app: &AppInstance, call: &CallContext<'_>) -> Result<WindowInventory, Error> {
        call.remaining()?;
        if let Some((_, result)) = self
            .inventories
            .lock()
            .unwrap()
            .iter()
            .find(|(member, _)| member == app)
        {
            return result.clone();
        }
        Ok(test_inventory(vec![WindowInfo {
            target: WindowTarget {
                app: app.clone(),
                ..target()
            },
            title: "same title".into(),
            visible: true,
        }]))
    }
    fn observe(
        &self,
        target: &WindowTarget,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<Self::Binding>, Error> {
        self.inner.observe(target, call)
    }
    fn capture(
        &self,
        target: &WindowTarget,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<Self::Binding>, Error> {
        PixelBackend(self.inner.clone()).capture(target, call)
    }
    fn press(
        &self,
        target: &WindowTarget,
        binding: &Self::Binding,
        element: ElementId,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        self.routed.lock().unwrap().push(target.clone());
        self.inner.press(target, binding, element, call)
    }
    fn click(
        &self,
        target: &WindowTarget,
        binding: &Self::Binding,
        _: PixelPoint,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        self.press(target, binding, ElementId(7), call)
    }
}
fn second_instance() -> WindowTarget {
    let mut second = target();
    second.app.pid += 1;
    second.app.identity = AppIdentity("different-install-path/same-public-app".into());
    second
}

#[test]
fn multi_instance_same_native_window_ids_route_by_complete_target_and_keep_intersection() {
    let backend = MultiInstanceBackend::new();
    let second = second_instance();
    let runtime = Runtime::with_apps(
        backend.clone(),
        vec![target().app, second.app.clone(), target().app],
        Duration::from_secs(30),
    )
    .unwrap();
    assert_eq!(runtime.instances().len(), 2);
    assert_eq!(runtime.windows().unwrap().windows.len(), 2);
    let a = runtime.observe(&target()).unwrap();
    let b = runtime.capture(&second).unwrap();
    let mut added = second.app.clone();
    added.pid += 1;
    runtime
        .reconcile_instances(vec![second.app.clone(), added, target().app])
        .unwrap();
    assert!(runtime.retains_observation(a.reference));
    assert!(runtime.retains_observation(b.reference));
    assert_eq!(
        runtime
            .press(&target(), a.reference, ElementId(7))
            .outcome
            .dispatch,
        DispatchState::Dispatched
    );
    assert_eq!(
        runtime
            .click(&second, b.reference, PixelPoint { x: 1.0, y: 1.0 })
            .outcome
            .dispatch,
        DispatchState::Dispatched
    );
    assert_eq!(*backend.routed.lock().unwrap(), vec![target(), second]);
    assert_eq!(backend.shutdowns.load(Ordering::SeqCst), 0);
}

#[test]
fn multi_instance_removal_and_pid_reuse_retire_only_original_member() {
    let backend = MultiInstanceBackend::new();
    let second = second_instance();
    let runtime = Runtime::with_apps(
        backend.clone(),
        vec![target().app, second.app.clone()],
        Duration::from_secs(30),
    )
    .unwrap();
    let a = runtime.observe(&target()).unwrap();
    let b = runtime.capture(&second).unwrap();
    let mut replacement = second.app.clone();
    replacement.generation += 1;
    runtime
        .reconcile_instances(vec![target().app, replacement])
        .unwrap();
    assert!(runtime.retains_observation(a.reference));
    assert!(!runtime.retains_observation(b.reference));
    assert_eq!(backend.inner.counts().3, 1);
    assert_rejected(
        runtime.click(&second, b.reference, PixelPoint { x: 1.0, y: 1.0 }),
        Error::WrongApp,
    );
    assert_eq!(
        runtime
            .press(&target(), a.reference, ElementId(7))
            .outcome
            .dispatch,
        DispatchState::Dispatched
    );
    assert_eq!(backend.shutdowns.load(Ordering::SeqCst), 0);
}

#[test]
fn multi_instance_partial_inventory_preserves_failed_member_and_retires_proven_missing() {
    let backend = MultiInstanceBackend::new();
    let second = second_instance();
    let runtime = Runtime::with_apps(
        backend.clone(),
        vec![target().app, second.app.clone()],
        Duration::from_secs(30),
    )
    .unwrap();
    let a = runtime.capture(&target()).unwrap();
    let b = runtime.observe(&second).unwrap();
    backend.set_inventory(
        second.app.clone(),
        Err(Error::NativeFailure {
            code: "member_unavailable".into(),
            message: "native member failed".into(),
        }),
    );
    let partial = runtime.windows().unwrap();
    assert_eq!(partial.windows.len(), 1);
    assert_eq!(partial.windows[0].target, target());
    assert!(partial.instances[0].report.is_ok());
    assert!(partial.instances[1].report.is_err());
    assert!(runtime.retains_observation(a.reference));
    assert!(runtime.retains_observation(b.reference));
    backend.set_inventory(target().app, Ok(test_inventory(vec![])));
    assert!(runtime.windows().unwrap().windows.is_empty());
    assert!(!runtime.retains_observation(a.reference));
    assert!(runtime.retains_observation(b.reference));
    assert_eq!(
        runtime
            .press(&second, b.reference, ElementId(7))
            .outcome
            .dispatch,
        DispatchState::Dispatched
    );
}

#[test]
fn multi_instance_all_failures_are_errors_and_member_spoofing_is_excluded() {
    let backend = MultiInstanceBackend::new();
    let second = second_instance();
    let runtime = Runtime::with_apps(
        backend.clone(),
        vec![target().app, second.app.clone()],
        Duration::from_secs(30),
    )
    .unwrap();
    backend.set_inventory(
        target().app,
        Ok(test_inventory(vec![WindowInfo {
            target: second.clone(),
            title: "spoofed".into(),
            visible: true,
        }])),
    );
    let partial = runtime.windows().unwrap();
    assert_eq!(partial.instances[0].report, Err(Error::WrongApp));
    assert_eq!(partial.windows.len(), 1);
    assert_eq!(partial.windows[0].target, second);
    backend.set_inventory(second.app, Err(Error::TimedOut));
    assert_eq!(
        runtime.windows(),
        Err(Error::AppInventoryUnproven(vec![
            Error::WrongApp,
            Error::TimedOut
        ]))
    );
}

#[test]
fn multi_instance_invalid_reconciliation_preserves_members_and_observations() {
    let backend = MultiInstanceBackend::new();
    let runtime = Runtime::with_apps(backend, vec![target().app], Duration::from_secs(30)).unwrap();
    let a = runtime.observe(&target()).unwrap();
    let mut reused = target().app;
    reused.generation += 1;
    assert!(runtime
        .reconcile_instances(vec![target().app, reused])
        .is_err());
    assert!(runtime.reconcile_instances(vec![]).is_err());
    let mut contradictory = target().app;
    contradictory.identity = AppIdentity("same-pid-birth-other-path".into());
    assert!(runtime
        .reconcile_instances(vec![target().app, contradictory])
        .is_err());
    assert_eq!(runtime.instances(), vec![target().app]);
    assert!(runtime.retains_observation(a.reference));
    let too_many = (0..33)
        .map(|index| AppInstance {
            pid: index + 100,
            ..target().app
        })
        .collect::<Vec<_>>();
    assert!(validate_app_instances(&too_many).is_err());
}

#[test]
fn multi_instance_reconciliation_holds_admission_through_binding_cleanup_and_cancel() {
    let backend = MultiInstanceBackend::new();
    let second = second_instance();
    let runtime = Arc::new(
        Runtime::with_apps(
            backend.clone(),
            vec![target().app, second.app.clone()],
            Duration::from_secs(30),
        )
        .unwrap(),
    );
    let (worker_gate, gate) = TestGate::pair();
    backend
        .inner
        .edit(|model| model.drop_gate = Some(worker_gate));
    runtime.observe(&target()).unwrap();
    let b = runtime.observe(&second).unwrap();
    let active = runtime.clone();
    let remaining = second.app.clone();
    let worker = thread::spawn(move || active.reconcile_instances(vec![remaining]));
    gate.wait_until_entered();
    assert_eq!(runtime.windows(), Err(Error::Busy));
    assert_eq!(
        runtime.reconcile_instances(vec![second.app.clone()]),
        Err(Error::Busy)
    );
    assert!(runtime.retains_observation(b.reference));
    runtime.cancel().unwrap();
    gate.release();
    assert_eq!(worker.join().unwrap(), Err(Error::Cancelled));
    assert_rejected(
        runtime.press(&second, b.reference, ElementId(7)),
        Error::Cancelled,
    );
    assert!(backend.routed.lock().unwrap().is_empty());
}

#[test]
fn multi_instance_fatal_context_errors_never_become_partial_inventory() {
    for error in [Error::Closed, Error::Cancelled, Error::WrongProvider] {
        let backend = MultiInstanceBackend::new();
        let second = second_instance();
        let runtime = Runtime::with_apps(
            backend.clone(),
            vec![target().app, second.app.clone()],
            Duration::from_secs(30),
        )
        .unwrap();
        backend.set_inventory(second.app, Err(error.clone()));
        assert_eq!(runtime.windows(), Err(error));
    }
}
