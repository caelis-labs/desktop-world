//! Logical app membership is independent of exact native window ownership.
use super::*;

type DropPlan = (Option<WorkerGate>, Option<Error>);
struct GroupBinding {
    backend: MockBackend,
    plan: Option<DropPlan>,
    panic_on_drop: bool,
}
impl Drop for GroupBinding {
    fn drop(&mut self) {
        assert!(
            !self.panic_on_drop,
            "controlled member binding cleanup panic"
        );
        if let Some((gate, error)) = self.plan.take() {
            if let Some(gate) = gate {
                gate.wait();
            }
            if let Some(error) = error {
                self.backend
                    .0
                    .model
                    .lock()
                    .unwrap()
                    .cleanup_error
                    .get_or_insert(error);
            }
        }
    }
}
#[derive(Clone)]
struct GroupBackend {
    inner: MockBackend,
    failures: Arc<Mutex<Vec<(AppInstance, Error)>>>,
    targets: Arc<Mutex<Vec<WindowTarget>>>,
    drop_plan: Arc<Mutex<Option<DropPlan>>>,
    panic_next_drop: Arc<Mutex<bool>>,
}
impl GroupBackend {
    fn new() -> Self {
        Self {
            inner: MockBackend::new(),
            failures: Arc::new(Mutex::new(vec![])),
            targets: Arc::new(Mutex::new(vec![])),
            drop_plan: Arc::new(Mutex::new(None)),
            panic_next_drop: Arc::new(Mutex::new(false)),
        }
    }
}
impl Backend for GroupBackend {
    type Binding = GroupBinding;
    fn supports(&self, capability: Capability) -> bool {
        self.inner.supports(capability)
    }
    fn stamp(&self) -> ProviderStamp {
        self.inner.stamp()
    }
    fn shutdown(&self) -> Result<(), Error> {
        self.inner.shutdown()
    }
    fn cleanup_error(&self) -> Option<Error> {
        self.inner.cleanup_error()
    }
    fn windows(&self, app: &AppInstance, call: &CallContext<'_>) -> Result<WindowInventory, Error> {
        if let Some((_, error)) = self
            .failures
            .lock()
            .unwrap()
            .iter()
            .find(|(member, _)| member == app)
        {
            return Err(error.clone());
        }
        self.inner.windows(app, call)
    }
    fn observe(
        &self,
        target: &WindowTarget,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<Self::Binding>, Error> {
        let observed = self.inner.observe(target, call)?;
        Ok(BackendObservation {
            target: observed.target,
            elements: observed.elements,
            image: observed.image,
            discovery: observed.discovery,
            binding: GroupBinding {
                backend: self.inner.clone(),
                plan: self.drop_plan.lock().unwrap().take(),
                panic_on_drop: std::mem::take(&mut *self.panic_next_drop.lock().unwrap()),
            },
        })
    }
    fn press(
        &self,
        target: &WindowTarget,
        _: &Self::Binding,
        element: ElementId,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        self.targets.lock().unwrap().push(target.clone());
        self.inner.press(target, &(), element, call)
    }
}
fn members() -> Vec<AppInstance> {
    let first = application();
    let mut second = first.clone();
    second.pid += 1;
    second.identity = AppIdentity("different-copy-private-identity".into());
    vec![first, second]
}
fn fixture() -> (
    Arc<Coordinator<GroupBackend>>,
    GroupBackend,
    Arc<Mutex<Value>>,
) {
    let backend = GroupBackend::new();
    let json_state = Arc::new(Mutex::new(json!({"count":42})));
    let owner = Coordinator::with_app_instances(
        backend.clone(),
        members(),
        "same public app".into(),
        json_state.clone(),
    )
    .unwrap();
    (owner, backend, json_state)
}

#[test]
fn membership_changes_preserve_surviving_window_observation_and_json_state() {
    let (owner, backend, state) = fixture();
    let refs = windows(&owner, "windows");
    assert_eq!(refs.len(), 4);
    let (observation, element) = observe(&owner, "observe-b", &refs[2]);
    let transplanted = run(
        &owner,
        "wrong-member",
        Some(&refs[0]),
        press(&observation, &element),
    );
    assert!(transplanted.is_error);
    assert!(backend.targets.lock().unwrap().is_empty());
    owner
        .reconcile_instances(vec![members()[1].clone()])
        .unwrap();
    assert_eq!(*state.lock().unwrap(), json!({"count":42}));
    assert_eq!(windows(&owner, "refresh"), refs[2..]);
    let removed = run(&owner, "removed", Some(&refs[0]), Command::Observe);
    assert_eq!(removed.data["error"]["code"], "target_changed");
    let pressed = run(
        &owner,
        "press-b",
        Some(&refs[2]),
        press(&observation, &element),
    );
    assert!(!pressed.is_error, "{}", pressed.data);
    assert_eq!(backend.targets.lock().unwrap()[0].app, members()[1]);
    assert_eq!(backend.inner.0.shutdowns.load(Ordering::SeqCst), 0);
}

#[test]
fn partial_inventory_is_explicit_and_does_not_retire_unproven_member_references() {
    let (owner, backend, _) = fixture();
    let refs = windows(&owner, "windows");
    let (observation, element) = observe(&owner, "observe-b", &refs[2]);
    backend.failures.lock().unwrap().push((
        members()[1].clone(),
        Error::NativeFailure {
            code: "member_unavailable".into(),
            message: "original native failure".into(),
        },
    ));
    let partial = run(&owner, "partial", None, Command::Windows);
    assert!(!partial.is_error, "{}", partial.data);
    let value = &partial.data["value"];
    assert_eq!(value["windows"].as_array().unwrap().len(), 2);
    assert_eq!(value["coverage"]["status"], "partial");
    assert_eq!(value["coverage"]["complete_within_scope"], false);
    assert_eq!(value["coverage"]["available_instances"], 1);
    assert_eq!(value["coverage"]["unavailable_instances"], 1);
    assert!(value["coverage"]["candidate_count"].is_null());
    assert_eq!(
        value["diagnostics"]["errors"][0]["code"],
        "member_unavailable"
    );
    assert_eq!(
        value["diagnostics"]["errors"][0]["message"],
        "original native failure"
    );
    assert!(!value.to_string().contains("private-identity"));
    let pressed = run(
        &owner,
        "press-b",
        Some(&refs[2]),
        press(&observation, &element),
    );
    assert!(!pressed.is_error, "{}", pressed.data);
    backend.failures.lock().unwrap().clear();
    assert_eq!(windows(&owner, "recovered"), refs);
}

#[test]
fn reconciliation_busy_does_not_change_members_or_late_action_receipt() {
    let (owner, backend, _) = fixture();
    let refs = windows(&owner, "windows");
    let (observation, element) = observe(&owner, "observe-b", &refs[2]);
    let gate = backend.inner.action_gate();
    start(
        &owner,
        submit(
            &owner,
            "press-b",
            Some(&refs[2]),
            press(&observation, &element),
        ),
    );
    gate.entered();
    assert_eq!(
        owner.reconcile_instances(vec![members()[0].clone()]),
        Err(Error::Busy)
    );
    assert_eq!(owner.instances(), members());
    owner.handle(Request::Cancel {
        request_id: "press-b".into(),
    });
    gate.release();
    let original = finish(&owner, "press-b");
    assert_eq!(original.data["receipt"]["outcome"]["dispatch"], "unknown");
    assert_eq!(
        original.data["receipt"]["provider_outcome"]["dispatch"],
        "dispatched"
    );
    assert_eq!(result(&owner, "press-b").data, original.data);
}

#[test]
fn reconciliation_cleanup_failure_seals_owner_and_keeps_original_completed_receipt() {
    let (owner, backend, state) = fixture();
    let refs = windows(&owner, "windows");
    *backend.drop_plan.lock().unwrap() = Some((
        None,
        Some(Error::NativeFailure {
            code: "release_failed".into(),
            message: "member binding still owned".into(),
        }),
    ));
    observe(&owner, "observe-a", &refs[0]);
    let original = result(&owner, "observe-a").data;
    assert!(owner
        .reconcile_instances(vec![members()[1].clone()])
        .is_err());
    assert_eq!(
        owner.handle(Request::Describe).data["context"]["shutdown_error"]["code"],
        "release_failed"
    );
    assert_eq!(
        owner.handle(Request::Describe).data["context"]["state"],
        "closed"
    );
    assert_eq!(result(&owner, "observe-a").data, original);
    assert_eq!(*state.lock().unwrap(), json!({"count":42}));
    assert_eq!(owner.reconcile_instances(members()), Err(Error::Closed));
}

#[test]
fn close_during_reconciliation_waits_for_retirement_and_retains_late_cleanup_error() {
    let (owner, backend, _) = fixture();
    let refs = windows(&owner, "windows");
    let (worker_gate, gate) = TestGate::pair();
    *backend.drop_plan.lock().unwrap() = Some((
        Some(worker_gate),
        Some(Error::NativeFailure {
            code: "late_release_failed".into(),
            message: "failed after concurrent close".into(),
        }),
    ));
    observe(&owner, "observe-a", &refs[0]);
    let active = owner.clone();
    let worker = std::thread::spawn(move || active.reconcile_instances(vec![members()[1].clone()]));
    gate.entered();
    assert!(!owner.wait_idle(Duration::from_millis(1)));
    let busy = start(
        &owner,
        submit(&owner, "during-reconcile", None, Command::Windows),
    );
    assert_eq!(busy.data["error"]["code"], "busy");
    owner.close();
    gate.release();
    assert!(worker.join().unwrap().is_err());
    assert!(owner.wait_idle(Duration::from_secs(1)));
    assert_eq!(
        owner.handle(Request::Describe).data["context"]["shutdown_error"]["code"],
        "late_release_failed"
    );
    assert_eq!(
        owner.handle(Request::Describe).data["context"]["state"],
        "closed"
    );
}

#[test]
fn reconciliation_cleanup_panic_is_a_sticky_shutdown_error_even_when_close_succeeds() {
    let (owner, backend, state) = fixture();
    let refs = windows(&owner, "windows");
    *backend.panic_next_drop.lock().unwrap() = true;
    observe(&owner, "observe-a", &refs[0]);
    assert!(owner
        .reconcile_instances(vec![members()[1].clone()])
        .is_err());
    let described = owner.handle(Request::Describe);
    assert_eq!(described.data["context"]["state"], "closed");
    assert_eq!(
        described.data["context"]["shutdown_error"]["code"],
        "shutdown_failed"
    );
    assert_eq!(*state.lock().unwrap(), json!({"count":42}));
    assert!(owner.close().is_error);
}

#[test]
fn all_failed_members_preserve_each_original_error_in_unproven_receipt() {
    let (owner, backend, _) = fixture();
    *backend.failures.lock().unwrap() = members()
        .into_iter()
        .enumerate()
        .map(|(index, app)| {
            (
                app,
                Error::NativeFailure {
                    code: format!("member_{index}_failed"),
                    message: format!("original failure {index}"),
                },
            )
        })
        .collect();
    let result = run(&owner, "all-unavailable", None, Command::Windows);
    assert!(result.is_error);
    assert_eq!(result.data["error"]["code"], "window_inventory_unproven");
    let details = &result.data["error"]["details"];
    assert_eq!(details["coverage"]["status"], "unproven");
    assert_eq!(details["coverage"]["unavailable_instances"], 2);
    assert_eq!(
        details["diagnostics"]["errors"][0]["code"],
        "member_0_failed"
    );
    assert_eq!(
        details["diagnostics"]["errors"][1]["message"],
        "original failure 1"
    );
}

#[test]
fn aggregate_inventory_keeps_counters_error_codes_and_provider_meaning() {
    let mut first = test_inventory(vec![]).report;
    first.diagnostics.provider = "one".into();
    first.diagnostics.counters = vec![("visited".into(), 2)];
    first.diagnostics.error_codes = vec![7, 8];
    let mut second = first.clone();
    second.diagnostics.counters[0].1 = 3;
    second.diagnostics.error_codes = vec![8, 9];
    let summary = aggregate_inventory_report(&[&first, &second], &[], 0);
    assert_eq!(summary["diagnostics"]["provider"], "one");
    assert_eq!(summary["diagnostics"]["counters"]["visited"], 5);
    assert_eq!(summary["diagnostics"]["error_codes"], json!([7, 8, 9]));
    second.diagnostics.provider = "two".into();
    let summary = aggregate_inventory_report(&[&first, &second], &[], 0);
    assert_eq!(summary["diagnostics"]["counters"]["one:visited"], 2);
    assert_eq!(summary["diagnostics"]["counters"]["two:visited"], 3);
}
