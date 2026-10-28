use super::*;
use std::sync::{mpsc, Arc};

#[derive(Clone)]
struct InputBackend {
    calls: Arc<Mutex<Vec<String>>>,
    gate: Arc<Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>>>,
}
impl Backend for InputBackend {
    type Binding = ();
    fn stamp(&self) -> ProviderStamp {
        ProviderStamp {
            id: "input-test".into(),
            generation: 1,
        }
    }
    fn supports(&self, _: Capability) -> bool {
        true
    }
    fn observe(
        &self,
        target: &WindowTarget,
        _: &CallContext<'_>,
    ) -> Result<BackendObservation<()>, Error> {
        Ok(BackendObservation {
            target: target.clone(),
            binding: (),
            image: None,
            discovery: None,
            elements: vec![Element {
                id: ElementId(1),
                label: "Title".into(),
                role: "text_field".into(),
                name: Some("Title".into()),
                value: Some("Old".into()),
                enabled: Some(true),
                can_press: false,
                actions: vec!["setValue".into()],
            }],
        })
    }
    fn press(&self, _: &WindowTarget, _: &(), _: ElementId, _: &CallContext<'_>) -> ActionOutcome {
        panic!("unexpected press")
    }
    fn input(&self, _: &WindowTarget, input: &WindowInput, _: &CallContext<'_>) -> ActionOutcome {
        self.calls.lock().unwrap().push(format!("{input:?}"));
        if let Some((entered, release)) = self.gate.lock().unwrap().take() {
            entered.send(()).unwrap();
            let _ = release.recv();
        }
        dispatched()
    }
    fn set_value(
        &self,
        _: &WindowTarget,
        _: &(),
        _: ElementId,
        value: &str,
        _: &CallContext<'_>,
    ) -> ActionOutcome {
        self.calls.lock().unwrap().push(value.into());
        dispatched()
    }
}
fn dispatched() -> ActionOutcome {
    ActionOutcome {
        dispatch: DispatchState::Dispatched,
        evidence: EffectEvidence::Unverified,
        error: None,
    }
}
fn setup() -> (Arc<Runtime<InputBackend>>, InputBackend, WindowTarget) {
    let target = WindowTarget {
        app: AppInstance {
            identity: AppIdentity("input.app".into()),
            pid: 42,
            generation: 1,
        },
        native_id: 2,
        generation: 3,
    };
    let backend = InputBackend {
        calls: Arc::default(),
        gate: Arc::default(),
    };
    (
        Arc::new(Runtime::new(
            backend.clone(),
            target.app.clone(),
            Duration::from_secs(15),
        )),
        backend,
        target,
    )
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

#[test]
fn window_input_requires_exact_app_but_no_semantic_observation() {
    let (runtime, backend, target) = setup();
    let input = WindowInput::TypeText {
        text: "中文 🐦\nNext line".into(),
    };
    let mut wrong = target.clone();
    wrong.app.generation += 1;
    assert_eq!(
        runtime.input_until(&wrong, &input, deadline()).outcome,
        ActionOutcome::rejected(Error::WrongApp)
    );
    assert!(backend.calls.lock().unwrap().is_empty());
    assert_eq!(
        runtime.input_until(&target, &input, deadline()).outcome,
        dispatched()
    );
    assert_eq!(backend.calls.lock().unwrap().len(), 1);
}

#[test]
fn window_input_retires_target_observations_and_preserves_other_windows() {
    let (runtime, backend, target) = setup();
    let a = runtime.observe(&target).unwrap();
    let mut other = target.clone();
    other.native_id += 1;
    let b = runtime.observe(&other).unwrap();
    assert_eq!(
        runtime
            .input_until(
                &target,
                &WindowInput::Keypress {
                    keys: vec!["META".into(), "A".into()]
                },
                deadline()
            )
            .outcome,
        dispatched()
    );
    assert!(!runtime.retains_observation(a.reference));
    assert!(runtime.retains_observation(b.reference));
    assert_eq!(backend.calls.lock().unwrap().len(), 1);
}

#[test]
fn setter_checks_advertised_element_and_consumes_the_original_query_once() {
    let (runtime, backend, target) = setup();
    let observed = runtime.observe(&target).unwrap();
    assert_eq!(
        runtime
            .set_value_until(
                &target,
                observed.reference,
                ElementId(99),
                "new",
                deadline()
            )
            .outcome,
        ActionOutcome::rejected(Error::ElementNotFound)
    );
    assert!(runtime.retains_observation(observed.reference));
    assert_eq!(
        runtime
            .set_value_until(&target, observed.reference, ElementId(1), "", deadline())
            .outcome,
        dispatched()
    );
    assert_eq!(
        runtime
            .set_value_until(&target, observed.reference, ElementId(1), "new", deadline())
            .outcome,
        ActionOutcome::rejected(Error::StaleObservation)
    );
    assert_eq!(*backend.calls.lock().unwrap(), vec![String::new()]);
}

#[test]
fn cancelled_input_keeps_admission_until_late_receipt_and_never_replays() {
    let (runtime, backend, target) = setup();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    *backend.gate.lock().unwrap() = Some((entered_tx, release_rx));
    let child = runtime.clone();
    let chosen = target.clone();
    let thread =
        std::thread::spawn(move || child.input_until(&chosen, &WindowInput::Focus, deadline()));
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    runtime.cancel().unwrap();
    assert_eq!(
        runtime
            .input_until(&target, &WindowInput::Focus, deadline())
            .outcome,
        ActionOutcome::rejected(Error::Busy)
    );
    release_tx.send(()).unwrap();
    let result = thread.join().unwrap();
    assert_eq!(result.outcome, ActionOutcome::unknown(Error::Cancelled));
    assert_eq!(result.provider_outcome, Some(dispatched()));
    assert_eq!(result.interruption, Some(Error::Cancelled));
    assert_eq!(
        runtime
            .input_until(&target, &WindowInput::Focus, deadline())
            .outcome,
        ActionOutcome::rejected(Error::Cancelled)
    );
    assert_eq!(backend.calls.lock().unwrap().len(), 1);
}
