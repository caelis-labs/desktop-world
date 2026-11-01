use super::*;

fn occluded() -> Error {
    Error::NativeFailure {
        code: "windows_capture_occluded".into(),
        message: "A foreign window covers the target".into(),
    }
}

fn capture_script<B: Backend + 'static>(owner: &Arc<Coordinator<B>>) -> Reply {
    run(owner, "capture-preparation", None, Command::Script {
        source: "const w=(await app.windows()).windows[0]; return await app.capture({window_ref:w.window_ref});".into(),
        timeout_ms: 5_000, max_calls: 8,
    })
}

#[test]
fn atomic_capture_without_a_script_ledger_never_prepares_focus() {
    let (owner, backend) = coordinator();
    let window = windows(&owner, "windows").remove(0);
    backend
        .0
        .model
        .lock()
        .unwrap()
        .capture_errors
        .push_back(occluded());
    let reply = run(&owner, "atomic-occluded", Some(&window), Command::Capture);
    assert_code(&reply, "windows_capture_occluded");
    assert!(reply.image.is_none());
    assert_eq!(backend.0.captures.load(Ordering::SeqCst), 1);
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 0);
    assert_eq!(result(&owner, "atomic-occluded").data, reply.data);
}

#[test]
fn occluded_capture_prepares_focus_once_and_retains_action_evidence() {
    let (owner, backend) = coordinator();
    {
        let mut model = backend.0.model.lock().unwrap();
        model.capture_errors.push_back(occluded());
        model.outcome.delivery = Some(InputDelivery {
            route: Some(InputRoute::Foreground),
            reason: Some("explicit_focus".into()),
            foreground_borrowed: true,
            focus_return: Some(FocusReturn::Pending),
        });
    }
    let reply = capture_script(&owner);
    assert!(!reply.is_error, "{}", reply.data);
    assert!(reply.image.is_some());
    assert_eq!(backend.0.captures.load(Ordering::SeqCst), 2);
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 1);
    let resolution = &reply.data["script"]["steps"][1]["resolution"];
    assert_eq!(resolution["status"], "captured");
    assert_eq!(
        resolution["focus_preparation"]["trigger"]["code"],
        "windows_capture_occluded"
    );
    assert_eq!(
        resolution["focus_preparation"]["receipt"]["outcome"]["dispatch"],
        "dispatched"
    );
    assert_eq!(result(&owner, "capture-preparation").data, reply.data);
}

#[test]
fn visible_or_non_occlusion_capture_never_borrows_focus() {
    for error in [
        None,
        Some(Error::TargetChanged),
        Some(Error::NativeFailure {
            code: "capture_unsupported".into(),
            message: "Unsupported renderer".into(),
        }),
    ] {
        let (owner, backend) = coordinator();
        if let Some(error) = error.clone() {
            backend
                .0
                .model
                .lock()
                .unwrap()
                .capture_errors
                .push_back(error);
        }
        let reply = capture_script(&owner);
        assert_eq!(reply.is_error, error.is_some());
        assert_eq!(backend.0.captures.load(Ordering::SeqCst), 1);
        assert_eq!(backend.0.actions.load(Ordering::SeqCst), 0);
        assert!(reply.data["script"]["steps"][1].get("resolution").is_none());
    }
}

#[test]
fn capture_stops_after_unproven_or_rejected_focus_without_a_second_read() {
    for outcome in [
        ActionOutcome::rejected(Error::NativeFailure {
            code: "foreground_required".into(),
            message: "Service policy denies focus".into(),
        }),
        ActionOutcome::unknown(Error::TargetChanged),
        ActionOutcome {
            dispatch: DispatchState::Unknown,
            delivery: None,
            evidence: EffectEvidence::Unverified,
            error: None,
        },
    ] {
        let (owner, backend) = coordinator();
        {
            let mut model = backend.0.model.lock().unwrap();
            model.capture_errors.push_back(occluded());
            model.outcome = outcome;
        }
        let reply = capture_script(&owner);
        assert!(reply.is_error, "{}", reply.data);
        assert_eq!(backend.0.captures.load(Ordering::SeqCst), 1);
        assert_eq!(backend.0.actions.load(Ordering::SeqCst), 1);
        let resolution = &reply.data["script"]["steps"][1]["resolution"];
        assert_eq!(resolution["status"], "focus_failed");
        assert!(resolution["focus_preparation"]["receipt"].is_object());
    }
}

#[test]
fn still_occluded_after_focus_is_not_retried() {
    let (owner, backend) = coordinator();
    backend
        .0
        .model
        .lock()
        .unwrap()
        .capture_errors
        .extend([occluded(), occluded()]);
    let reply = capture_script(&owner);
    assert!(reply.is_error, "{}", reply.data);
    assert_eq!(backend.0.captures.load(Ordering::SeqCst), 2);
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(
        reply.data["script"]["steps"][1]["error"]["code"],
        "windows_capture_occluded"
    );
    assert_eq!(
        reply.data["script"]["steps"][1]["resolution"]["status"],
        "capture_failed"
    );
}

#[test]
fn cancellation_during_capture_focus_retains_receipt_without_a_second_read() {
    let (owner, backend) = coordinator();
    {
        let mut model = backend.0.model.lock().unwrap();
        model.capture_errors.push_back(occluded());
        model.poll_action = true;
    }
    let gate = backend.action_gate();
    start(&owner, submit(&owner, "capture-cancel", None, Command::Script {
        source: "const w=(await app.windows()).windows[0]; return await app.capture({window_ref:w.window_ref});".into(),
        timeout_ms: 5_000, max_calls: 8,
    }));
    // The barrier establishes that the initial capture failed and focus entered
    // its provider. Cancellation polls that exact action; no timed sleep/replay.
    gate.entered();
    owner.handle(Request::Cancel {
        request_id: "capture-cancel".into(),
    });
    gate.release();
    let reply = finish(&owner, "capture-cancel");
    assert!(reply.is_error, "{}", reply.data);
    assert!(reply.image.is_none());
    assert_eq!(backend.0.captures.load(Ordering::SeqCst), 1);
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 1);
    let resolution = &reply.data["script"]["steps"][1]["resolution"];
    assert_eq!(resolution["status"], "focus_failed");
    let receipt = &resolution["focus_preparation"]["receipt"];
    assert_eq!(receipt["outcome"]["dispatch"], "unknown");
    assert_eq!(receipt["outcome"]["error"]["code"], "cancelled");
    assert_eq!(receipt["interruption"]["code"], "cancelled");
    assert!(receipt["provider_outcome"].is_object());
    assert_eq!(result(&owner, "capture-cancel").data, reply.data);
}

#[test]
fn capture_policy_refusal_keeps_not_dispatched_and_original_delivery() {
    let (owner, backend) = coordinator();
    {
        let mut model = backend.0.model.lock().unwrap();
        model.capture_errors.push_back(occluded());
        model.outcome = ActionOutcome::rejected(Error::NativeFailure {
            code: "foreground_required".into(),
            message: "Service input policy denies foreground preparation".into(),
        });
        model.outcome.delivery = Some(InputDelivery {
            route: None,
            reason: Some("service_input_policy".into()),
            foreground_borrowed: false,
            focus_return: Some(FocusReturn::Unchanged),
        });
    }
    let reply = capture_script(&owner);
    assert!(reply.is_error, "{}", reply.data);
    assert!(reply.image.is_none());
    assert_eq!(backend.0.captures.load(Ordering::SeqCst), 1);
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 1);
    let step = &reply.data["script"]["steps"][1];
    assert_eq!(step["error"]["code"], "foreground_required");
    let receipt = &step["resolution"]["focus_preparation"]["receipt"];
    assert_eq!(receipt["outcome"]["dispatch"], "not_dispatched");
    assert_eq!(receipt["provider_outcome"]["dispatch"], "not_dispatched");
    assert_eq!(
        receipt["outcome"]["delivery"]["reason"],
        "service_input_policy"
    );
    assert_eq!(receipt["outcome"]["delivery"]["foreground_borrowed"], false);
    assert_eq!(receipt["interruption"], Value::Null);
}

struct CaptureWithoutFocus(MockBackend);
impl Backend for CaptureWithoutFocus {
    type Binding = ();
    fn stamp(&self) -> ProviderStamp {
        self.0.stamp()
    }
    fn supports(&self, capability: Capability) -> bool {
        capability != Capability::Focus
    }
    fn windows(&self, app: &AppInstance, call: &CallContext<'_>) -> Result<WindowInventory, Error> {
        self.0.windows(app, call)
    }
    fn observe(
        &self,
        target: &WindowTarget,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<()>, Error> {
        self.0.observe(target, call)
    }
    fn capture(
        &self,
        target: &WindowTarget,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<()>, Error> {
        self.0.capture(target, call)
    }
    fn press(
        &self,
        target: &WindowTarget,
        binding: &(),
        element: ElementId,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        self.0.press(target, binding, element, call)
    }
    fn input(&self, _: &WindowTarget, _: &WindowInput, _: &CallContext<'_>) -> ActionOutcome {
        panic!("Core must reject unsupported focus before entering the provider")
    }
}

#[test]
fn capture_permission_does_not_imply_focus_permission() {
    let backend = MockBackend::new();
    backend
        .0
        .model
        .lock()
        .unwrap()
        .capture_errors
        .push_back(occluded());
    let owner = Coordinator::new(
        CaptureWithoutFocus(backend.clone()),
        application(),
        "Capture only".into(),
    );
    let reply = capture_script(&owner);
    assert!(reply.is_error, "{}", reply.data);
    assert_eq!(backend.0.captures.load(Ordering::SeqCst), 1);
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 0);
    let step = &reply.data["script"]["steps"][1];
    assert_eq!(step["error"]["code"], "unsupported_capability");
    let receipt = &step["resolution"]["focus_preparation"]["receipt"];
    assert_eq!(receipt["outcome"]["dispatch"], "not_dispatched");
    assert_eq!(receipt["provider_outcome"], Value::Null);
}

#[test]
fn capture_focus_retires_target_semantics_and_preserves_other_windows() {
    for occlusion in [false, true] {
        let (owner, backend) = coordinator();
        let windows = windows(&owner, "windows");
        let (target_observation, target_element) = observe(&owner, "target", &windows[0]);
        let (other_observation, other_element) = observe(&owner, "other", &windows[1]);
        if occlusion {
            backend
                .0
                .model
                .lock()
                .unwrap()
                .capture_errors
                .push_back(occluded());
        }
        let reply = capture_script(&owner);
        assert!(!reply.is_error, "{}", reply.data);
        assert_eq!(
            backend.0.actions.load(Ordering::SeqCst),
            usize::from(occlusion)
        );
        let target = run(
            &owner,
            "target-old-ref",
            Some(&windows[0]),
            press(&target_observation, &target_element),
        );
        if occlusion {
            assert_code(&target, "stale_observation");
            assert_eq!(backend.0.actions.load(Ordering::SeqCst), 1);
        } else {
            assert!(!target.is_error, "{}", target.data);
        }
        let other = run(
            &owner,
            "other-old-ref",
            Some(&windows[1]),
            press(&other_observation, &other_element),
        );
        assert!(!other.is_error, "{}", other.data);
        assert_eq!(backend.0.actions.load(Ordering::SeqCst), 2);
        assert_eq!(
            backend.0.captures.load(Ordering::SeqCst),
            if occlusion { 2 } else { 1 }
        );
    }
}
