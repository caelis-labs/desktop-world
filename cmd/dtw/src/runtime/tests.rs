use super::{ActionReceipt, Clock, Grant, Runtime};
use crate::computer::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

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
        self.entered.send(()).unwrap();
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

    fn stamp(&self) -> ProviderStamp {
        self.0.model.lock().unwrap().provider.clone()
    }

    fn supports(&self, capability: Capability) -> bool {
        let model = self.0.model.lock().unwrap();
        match capability {
            Capability::Observe => model.supports_observe,
            Capability::Press => model.supports_press,
        }
    }

    fn observe(&self, target: &WindowTarget) -> Result<BackendObservation<NativeBinding>, Error> {
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
                    can_press: true,
                }]
            });
            let observation = BackendObservation {
                target: model
                    .observed_target
                    .clone()
                    .unwrap_or_else(|| target.clone()),
                elements,
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
    ) -> ActionOutcome {
        assert_eq!(element, ElementId(7));
        self.0.presses.fetch_add(1, Ordering::SeqCst);
        let (gate, outcome, panic_on_press) = {
            let mut model = self.0.model.lock().unwrap();
            (
                model.press_gate.take(),
                model.press_outcome.take().unwrap_or(ActionOutcome {
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

fn runtime(backend: &TestBackend, clock: &ManualClock, caps: Vec<Capability>) -> Arc<TestRuntime> {
    Arc::new(Runtime::with_clock(
        backend.clone(),
        Grant::new(target().app, caps, clock.now() + Duration::from_secs(10)),
        Duration::from_secs(3),
        clock.clone(),
    ))
}

fn fixture() -> (Arc<TestRuntime>, TestBackend, ManualClock) {
    let backend = TestBackend::new();
    let clock = ManualClock::new();
    let runtime = runtime(
        &backend,
        &clock,
        vec![Capability::Observe, Capability::Press],
    );
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
fn wrong_app_or_instance_never_dispatches_or_consumes_authorized_reference() {
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
    let second = runtime(
        &backend,
        &clock,
        vec![Capability::Observe, Capability::Press],
    );
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
fn capabilities_are_authorized_separately_from_backend_support() {
    let backend = TestBackend::new();
    let clock = ManualClock::new();
    let observe_only = runtime(&backend, &clock, vec![Capability::Observe]);
    let observation = observe_only.observe(&target()).unwrap();
    assert_rejected(
        observe_only.press(&target(), observation.reference, ElementId(7)),
        Error::CapabilityDenied,
    );
    let press_only = runtime(&backend, &clock, vec![Capability::Press]);
    assert_eq!(press_only.observe(&target()), Err(Error::CapabilityDenied));
    assert_eq!(backend.counts(), (1, 0, 0, 0));

    let both = runtime(
        &backend,
        &clock,
        vec![Capability::Observe, Capability::Press],
    );
    backend.edit(|model| model.supports_observe = false);
    assert_eq!(both.observe(&target()), Err(Error::UnsupportedCapability));
    backend.edit(|model| model.supports_observe = true);
    let observation = both.observe(&target()).unwrap();
    backend.edit(|model| model.supports_press = false);
    assert_rejected(
        both.press(&target(), observation.reference, ElementId(7)),
        Error::UnsupportedCapability,
    );
    assert_eq!(backend.counts(), (2, 0, 0, 0));
    backend.edit(|model| model.supports_press = true);
    assert_eq!(
        both.press(&target(), observation.reference, ElementId(7))
            .outcome
            .error,
        None
    );
}

#[test]
fn grant_and_observation_expiry_reject_at_exact_deadline_without_dispatch() {
    let (runtime, backend, clock) = fixture();
    let observation = runtime.observe(&target()).unwrap();
    clock.advance(Duration::from_secs(3));
    assert_rejected(
        runtime.press(&target(), observation.reference, ElementId(7)),
        Error::StaleObservation,
    );
    assert_eq!(backend.counts(), (1, 0, 0, 0));
    clock.advance(Duration::from_secs(5));
    let fresh = runtime.observe(&target()).unwrap();
    clock.advance(Duration::from_secs(2));
    assert_rejected(
        runtime.press(&target(), fresh.reference, ElementId(7)),
        Error::GrantExpired,
    );
    assert_eq!(runtime.observe(&target()), Err(Error::GrantExpired));
    assert_eq!(backend.counts(), (2, 0, 0, 1));
}

#[test]
fn element_validation_rejects_before_dispatch_without_consuming_reference() {
    let (runtime, backend, _) = fixture();
    backend.edit(|model| {
        model.elements = Some(vec![
            Element {
                id: ElementId(7),
                label: "button".into(),
                can_press: true,
            },
            Element {
                id: ElementId(8),
                label: "label".into(),
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
fn observation_replacement_rechecks_authority_after_retired_binding_cleanup() {
    for expire_grant in [false, true] {
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
        let expected = if expire_grant {
            clock.advance(Duration::from_secs(10));
            Error::GrantExpired
        } else {
            let (done_tx, done_rx) = mpsc::channel();
            let cancelling_runtime = runtime.clone();
            canceller = Some(thread::spawn(move || {
                cancelling_runtime.cancel();
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
            expected,
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
            runtime.close();
            Error::Closed
        } else {
            runtime.cancel();
            Error::Cancelled
        };
        assert_rejected(
            runtime.press(&target(), observation.reference, ElementId(7)),
            expected.clone(),
        );
        assert_eq!(runtime.observe(&target()), Err(expected));
        runtime.cancel();
        runtime.close();
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
            runtime.close();
            Error::Closed
        } else {
            runtime.cancel();
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

        // A trusted embedding must create a new context. It can see the actual
        // late effect, but cannot reuse the cancelled context's reference.
        let fresh_runtime = self::runtime(
            &backend,
            &clock,
            vec![Capability::Observe, Capability::Press],
        );
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
    for expire_grant in [false, true] {
        let (runtime, backend, clock) = fixture();
        let observation = runtime.observe(&target()).unwrap();
        let (worker_gate, gate) = TestGate::pair();
        backend.edit(|model| model.press_gate = Some(worker_gate));
        let worker_runtime = runtime.clone();
        let worker = thread::spawn(move || {
            worker_runtime.press(&target(), observation.reference, ElementId(7))
        });
        gate.wait_until_entered();
        let expected = if expire_grant {
            clock.advance(Duration::from_secs(10));
            Error::GrantExpired
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
                runtime.cancel();
                Error::Cancelled
            }
            1 => {
                clock.advance(Duration::from_secs(3));
                Error::StaleObservation
            }
            2 => {
                clock.advance(Duration::from_secs(10));
                Error::GrantExpired
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
        cancelling_runtime.cancel();
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
        cancelling_runtime.cancel();
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
                runtime.cancel();
                Error::Cancelled
            }
            1 => {
                runtime.close();
                Error::Closed
            }
            2 => {
                clock.advance(Duration::from_secs(10));
                Error::GrantExpired
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
        runtime.cancel();
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
        ) -> Result<BackendObservation<Self::Binding>, Error> {
            Ok(BackendObservation {
                target: target.clone(),
                elements: Vec::new(),
                binding: OrderedBinding(self.0.clone()),
            })
        }

        fn press(&self, _: &WindowTarget, _: &Self::Binding, _: ElementId) -> ActionOutcome {
            ActionOutcome::rejected(Error::UnsupportedCapability)
        }
    }

    let events = Arc::new(Mutex::new(Vec::new()));
    let clock = ManualClock::new();
    let runtime = Runtime::with_clock(
        OrderedBackend(events.clone()),
        Grant::new(
            target().app,
            vec![Capability::Observe],
            clock.now() + Duration::from_secs(10),
        ),
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
