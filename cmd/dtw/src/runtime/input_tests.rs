use super::*;
use std::sync::{mpsc, Arc};

#[derive(Clone)]
struct InputBackend {
    calls: Arc<Mutex<Vec<String>>>,
    anchors: Arc<std::sync::atomic::AtomicBool>,
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
    fn supports_scroll_anchor(&self, _: ObservationKind) -> bool {
        self.anchors.load(std::sync::atomic::Ordering::Acquire)
    }
    fn capture(
        &self,
        target: &WindowTarget,
        _: &CallContext<'_>,
    ) -> Result<BackendObservation<()>, Error> {
        Ok(BackendObservation {
            target: target.clone(),
            binding: (),
            elements: vec![],
            discovery: None,
            image: Some(ImageFrame {
                width: 100,
                height: 80,
                png: vec![1],
            }),
        })
    }
    fn scroll_at(
        &self,
        _: &WindowTarget,
        _: &(),
        scroll: ScrollRequest,
        _: &CallContext<'_>,
    ) -> ActionOutcome {
        self.calls.lock().unwrap().push(format!("{scroll:?}"));
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
        delivery: None,
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
        anchors: Arc::default(),
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

#[test]
fn interruption_retains_the_provider_delivery_without_claiming_effect_success() {
    for dispatch in [
        DispatchState::Dispatched,
        DispatchState::Unknown,
        DispatchState::NotDispatched,
    ] {
        let provider = ActionOutcome {
            delivery: Some(InputDelivery {
                route: Some(InputRoute::Foreground),
                reason: Some("background_unavailable".into()),
                foreground_borrowed: true,
                focus_return: Some(FocusReturn::Restored),
            }),
            dispatch,
            evidence: EffectEvidence::Unverified,
            error: None,
        };
        let receipt = ActionReceipt::from_provider(provider.clone(), Some(Error::Cancelled));
        assert_eq!(receipt.outcome.delivery, provider.delivery);
        assert_eq!(receipt.provider_outcome, Some(provider));
        assert_eq!(
            receipt.outcome.dispatch,
            if dispatch == DispatchState::NotDispatched {
                DispatchState::NotDispatched
            } else {
                DispatchState::Unknown
            }
        );
        assert_eq!(receipt.interruption, Some(Error::Cancelled));
    }
    let absent = ActionReceipt::from_provider(dispatched(), Some(Error::Cancelled));
    assert!(absent.outcome.delivery.is_none());
}

fn anchored(anchor: ScrollAnchor) -> ScrollRequest {
    ScrollRequest {
        anchor,
        delta_x: 0,
        delta_y: 8,
    }
}
fn enable_anchors(backend: &InputBackend) {
    backend
        .anchors
        .store(true, std::sync::atomic::Ordering::Release);
}

#[test]
fn anchored_scroll_unsupported_preserves_semantic_and_pixel_bindings() {
    let (runtime, backend, target) = setup();
    let semantic = runtime.observe(&target).unwrap();
    let pixels = runtime.capture_until(&target, deadline()).unwrap();
    for (reference, anchor) in [
        (semantic.reference, ScrollAnchor::Element(ElementId(1))),
        (
            pixels.reference,
            ScrollAnchor::Pixels(PixelPoint { x: 10.0, y: 20.0 }),
        ),
    ] {
        assert_eq!(
            runtime
                .scroll_at_until(&target, reference, anchored(anchor), deadline())
                .outcome,
            ActionOutcome::rejected(Error::UnsupportedCapability)
        );
        assert!(runtime.retains_observation(semantic.reference));
        assert!(runtime.retains_observation(pixels.reference));
    }
    assert!(backend.calls.lock().unwrap().is_empty());
    enable_anchors(&backend);
    assert_eq!(
        runtime
            .scroll_at_until(
                &target,
                semantic.reference,
                anchored(ScrollAnchor::Element(ElementId(1))),
                deadline()
            )
            .outcome,
        dispatched()
    );
    assert!(!runtime.retains_observation(semantic.reference));
    assert!(!runtime.retains_observation(pixels.reference));
    assert_eq!(backend.calls.lock().unwrap().len(), 1);
}

#[test]
fn semantic_scroll_needs_membership_not_pressability_and_consumes_only_its_window() {
    let (runtime, backend, target) = setup();
    enable_anchors(&backend);
    let mut other = target.clone();
    other.native_id += 1;
    let original = runtime.observe(&target).unwrap();
    let neighbor = runtime.observe(&other).unwrap();
    assert!(!original.elements[0].can_press);
    let invalid = anchored(ScrollAnchor::Element(ElementId(99)));
    assert_eq!(
        runtime
            .scroll_at_until(&target, original.reference, invalid, deadline())
            .outcome,
        ActionOutcome::rejected(Error::ElementNotFound)
    );
    let valid = anchored(ScrollAnchor::Element(ElementId(1)));
    assert_eq!(
        runtime
            .scroll_at_until(&other, original.reference, valid, deadline())
            .outcome,
        ActionOutcome::rejected(Error::TargetChanged)
    );
    assert!(backend.calls.lock().unwrap().is_empty());
    assert_eq!(
        runtime
            .scroll_at_until(&target, original.reference, valid, deadline())
            .outcome,
        dispatched()
    );
    assert_eq!(
        runtime
            .scroll_at_until(&target, original.reference, valid, deadline())
            .outcome,
        ActionOutcome::rejected(Error::StaleObservation)
    );
    assert!(runtime.retains_observation(neighbor.reference));
    assert_eq!(backend.calls.lock().unwrap().len(), 1);
}

#[test]
fn pixel_scroll_requires_capture_kind_and_finite_in_image_anchor() {
    let (runtime, backend, target) = setup();
    enable_anchors(&backend);
    let semantic = runtime.observe(&target).unwrap();
    let pixels = runtime.capture_until(&target, deadline()).unwrap();
    let valid = anchored(ScrollAnchor::Pixels(PixelPoint { x: 99.5, y: 79.5 }));
    assert_eq!(
        runtime
            .scroll_at_until(&target, semantic.reference, valid, deadline())
            .outcome,
        ActionOutcome::rejected(Error::InvalidObservation)
    );
    assert_eq!(
        runtime
            .scroll_at_until(
                &target,
                pixels.reference,
                anchored(ScrollAnchor::Element(ElementId(1))),
                deadline()
            )
            .outcome,
        ActionOutcome::rejected(Error::InvalidObservation)
    );
    for point in [
        PixelPoint { x: -1.0, y: 1.0 },
        PixelPoint { x: 100.0, y: 1.0 },
        PixelPoint { x: 1.0, y: 80.0 },
        PixelPoint {
            x: f64::NAN,
            y: 1.0,
        },
        PixelPoint {
            x: 1.0,
            y: f64::INFINITY,
        },
    ] {
        assert_eq!(
            runtime
                .scroll_at_until(
                    &target,
                    pixels.reference,
                    anchored(ScrollAnchor::Pixels(point)),
                    deadline()
                )
                .outcome,
            ActionOutcome::rejected(Error::InvalidPoint)
        );
        assert!(runtime.retains_observation(pixels.reference));
    }
    assert!(backend.calls.lock().unwrap().is_empty());
    assert_eq!(
        runtime
            .scroll_at_until(&target, pixels.reference, valid, deadline())
            .outcome,
        dispatched()
    );
    assert_eq!(backend.calls.lock().unwrap().len(), 1);
}

#[test]
fn cancellation_of_anchored_scroll_keeps_original_receipt_and_admission_until_return() {
    let (runtime, backend, target) = setup();
    enable_anchors(&backend);
    let observed = runtime.observe(&target).unwrap();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    *backend.gate.lock().unwrap() = Some((entered_tx, release_rx));
    let child = runtime.clone();
    let chosen = target.clone();
    let scroll = anchored(ScrollAnchor::Element(ElementId(1)));
    let thread = std::thread::spawn(move || {
        child.scroll_at_until(&chosen, observed.reference, scroll, deadline())
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    runtime.cancel().unwrap();
    assert_eq!(
        runtime
            .scroll_at_until(&target, observed.reference, scroll, deadline())
            .outcome,
        ActionOutcome::rejected(Error::Busy)
    );
    release_tx.send(()).unwrap();
    let receipt = thread.join().unwrap();
    assert_eq!(receipt.outcome, ActionOutcome::unknown(Error::Cancelled));
    assert_eq!(receipt.provider_outcome, Some(dispatched()));
    assert_eq!(receipt.interruption, Some(Error::Cancelled));
    assert_eq!(backend.calls.lock().unwrap().len(), 1);
}
