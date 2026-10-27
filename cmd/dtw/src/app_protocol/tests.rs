use super::*;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc,
};
use std::time::{SystemTime, UNIX_EPOCH};
use wire::Target;

struct WorkerGate {
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}
impl WorkerGate {
    fn wait(self) {
        self.entered.send(()).unwrap();
        // A failing test drops the sender and releases the mock worker too.
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
    fn entered(&self) {
        self.entered
            .recv_timeout(Duration::from_secs(5))
            .expect("mock did not reach the operation barrier");
    }
    fn release(self) {
        let _ = self.release.send(());
    }
}

struct Model {
    read_gate: Option<WorkerGate>,
    action_gate: Option<WorkerGate>,
    outcome: ActionOutcome,
    panic_action: bool,
    poll_action: bool,
    shutdown_error: Option<Error>,
    windows_error: Option<Error>,
}
struct Shared {
    model: Mutex<Model>,
    windows: AtomicUsize,
    observes: AtomicUsize,
    captures: AtomicUsize,
    actions: AtomicUsize,
    shutdowns: AtomicUsize,
}
#[derive(Clone)]
struct MockBackend(Arc<Shared>);
impl MockBackend {
    fn new() -> Self {
        Self(Arc::new(Shared {
            model: Mutex::new(Model {
                read_gate: None,
                action_gate: None,
                outcome: ActionOutcome {
                    dispatch: DispatchState::Dispatched,
                    evidence: EffectEvidence::Unverified,
                    error: None,
                },
                panic_action: false,
                poll_action: false,
                shutdown_error: None,
                windows_error: None,
            }),
            windows: AtomicUsize::new(0),
            observes: AtomicUsize::new(0),
            captures: AtomicUsize::new(0),
            actions: AtomicUsize::new(0),
            shutdowns: AtomicUsize::new(0),
        }))
    }
    fn read_gate(&self) -> TestGate {
        let (worker, test) = TestGate::pair();
        self.0.model.lock().unwrap().read_gate = Some(worker);
        test
    }
    fn action_gate(&self) -> TestGate {
        let (worker, test) = TestGate::pair();
        self.0.model.lock().unwrap().action_gate = Some(worker);
        test
    }
    fn act(&self, call: &CallContext<'_>) -> ActionOutcome {
        self.0.actions.fetch_add(1, Ordering::SeqCst);
        let (gate, outcome, panic, poll) = {
            let mut model = self.0.model.lock().unwrap();
            (
                model.action_gate.take(),
                model.outcome.clone(),
                model.panic_action,
                model.poll_action,
            )
        };
        if let Some(gate) = gate {
            if poll {
                gate.entered.send(()).unwrap();
                loop {
                    let left = match call.remaining() {
                        Ok(left) => left,
                        Err(error) => return ActionOutcome::unknown(error),
                    };
                    match gate
                        .release
                        .recv_timeout(left.min(Duration::from_millis(10)))
                    {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                }
            } else {
                gate.wait();
            }
        }
        assert!(!panic, "controlled provider panic");
        outcome
    }
    fn read(&self, target: &WindowTarget, pixels: bool) -> BackendObservation<()> {
        let gate = self.0.model.lock().unwrap().read_gate.take();
        if let Some(gate) = gate {
            gate.wait();
        }
        BackendObservation {
            target: target.clone(),
            binding: (),
            elements: if pixels {
                vec![]
            } else {
                vec![Element {
                    id: ElementId(9007199254740993),
                    label: "Increment".into(),
                    can_press: true,
                }]
            },
            // Opaque controlled bytes exercise retention/encoding, not PNG or GUI acceptance.
            image: pixels.then(|| ImageFrame {
                width: 20,
                height: 10,
                png: vec![7; 4096],
            }),
        }
    }
}
impl Backend for MockBackend {
    type Binding = ();
    fn stamp(&self) -> ProviderStamp {
        ProviderStamp {
            id: "controlled".into(),
            generation: 1,
        }
    }
    fn supports(&self, _: Capability) -> bool {
        true
    }
    fn shutdown(&self) -> Result<(), Error> {
        self.0.shutdowns.fetch_add(1, Ordering::SeqCst);
        self.0
            .model
            .lock()
            .unwrap()
            .shutdown_error
            .clone()
            .map_or(Ok(()), Err)
    }
    fn windows(&self, app: &AppInstance, _: &CallContext<'_>) -> Result<Vec<WindowInfo>, Error> {
        self.0.windows.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = self.0.model.lock().unwrap().windows_error.clone() {
            return Err(error);
        }
        Ok([101, 202]
            .into_iter()
            .map(|native_id| WindowInfo {
                target: WindowTarget {
                    app: app.clone(),
                    native_id,
                    generation: 3,
                },
                title: format!("Window {native_id}"),
                visible: true,
            })
            .collect())
    }
    fn observe(
        &self,
        target: &WindowTarget,
        _: &CallContext<'_>,
    ) -> Result<BackendObservation<()>, Error> {
        self.0.observes.fetch_add(1, Ordering::SeqCst);
        Ok(self.read(target, false))
    }
    fn capture(
        &self,
        target: &WindowTarget,
        _: &CallContext<'_>,
    ) -> Result<BackendObservation<()>, Error> {
        self.0.captures.fetch_add(1, Ordering::SeqCst);
        Ok(self.read(target, true))
    }
    fn press(
        &self,
        _: &WindowTarget,
        _: &(),
        _: ElementId,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        self.act(call)
    }
    fn click(
        &self,
        _: &WindowTarget,
        _: &(),
        _: PixelPoint,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        self.act(call)
    }
}
fn authority() -> HostGrant {
    HostGrant {
        grant_id: "host-approved".into(),
        app: AppInstance {
            identity: AppIdentity("org.example.Controlled".into()),
            pid: 42,
            generation: u64::MAX,
        },
        capabilities: vec![
            Capability::Observe,
            Capability::Press,
            Capability::Capture,
            Capability::Click,
        ],
        expires_at_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            + 60_000,
        expires_at: Instant::now() + Duration::from_secs(60),
    }
}
fn coordinator() -> (Arc<Coordinator<MockBackend>>, MockBackend) {
    let backend = MockBackend::new();
    (
        Coordinator::new(backend.clone(), authority(), "Controlled".into()),
        backend,
    )
}
fn submit(
    owner: &Arc<Coordinator<MockBackend>>,
    id: &str,
    window: Option<&str>,
    command: Command,
) -> Submit {
    Submit {
        request_id: id.into(),
        target: Target {
            app_ref: owner.handle(Request::Describe).data["app"]["app_ref"]
                .as_str()
                .unwrap()
                .into(),
            window_ref: window.map(str::to_owned),
        },
        command,
    }
}
fn start(owner: &Arc<Coordinator<MockBackend>>, submitted: Submit) -> Reply {
    owner.handle(Request::Submit(submitted))
}
fn result(owner: &Arc<Coordinator<MockBackend>>, id: &str) -> Reply {
    owner.handle(Request::Result {
        request_id: id.into(),
    })
}
fn finish(owner: &Arc<Coordinator<MockBackend>>, id: &str) -> Reply {
    assert!(
        owner.wait_idle(Duration::from_secs(5)),
        "accepted job did not settle"
    );
    let reply = result(owner, id);
    assert_eq!(reply.data["status"], "completed", "{}", reply.data);
    reply
}
fn run(
    owner: &Arc<Coordinator<MockBackend>>,
    id: &str,
    window: Option<&str>,
    command: Command,
) -> Reply {
    let accepted = start(owner, submit(owner, id, window, command));
    assert_eq!(accepted.data["status"], "running", "{}", accepted.data);
    finish(owner, id)
}
fn windows(owner: &Arc<Coordinator<MockBackend>>, id: &str) -> Vec<String> {
    run(owner, id, None, Command::Windows).data["value"]["windows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["window_ref"].as_str().unwrap().into())
        .collect()
}
fn observe(owner: &Arc<Coordinator<MockBackend>>, id: &str, window: &str) -> (String, String) {
    let reply = run(owner, id, Some(window), Command::Observe);
    (
        reply.data["value"]["observation_ref"]
            .as_str()
            .unwrap()
            .into(),
        reply.data["value"]["elements"][0]["element_ref"]
            .as_str()
            .unwrap()
            .into(),
    )
}
fn press(observation: &str, element: &str) -> Command {
    Command::Press {
        observation_ref: observation.into(),
        element_ref: element.into(),
    }
}
fn assert_code(reply: &Reply, code: &str) {
    assert!(reply.is_error, "{}", reply.data);
    assert_eq!(reply.data["error"]["code"], code, "{}", reply.data);
}

#[test]
fn describe_reports_trusted_authority_separately_from_backend_support() {
    let backend = MockBackend::new();
    let mut grant = authority();
    grant.capabilities = vec![Capability::Observe];
    let owner = Coordinator::new(backend.clone(), grant, "Controlled".into());
    let description = owner.handle(Request::Describe).data;
    assert_eq!(description["version"], 1);
    assert_eq!(description["app"]["generation"], u64::MAX.to_string());
    assert_eq!(description["grant"]["capabilities"], json!(["observe"]));
    assert_eq!(
        description["supported_capabilities"],
        json!(["observe", "press", "capture", "click"])
    );
    let window = windows(&owner, "windows").remove(0);
    assert_code(
        &run(&owner, "capture", Some(&window), Command::Capture),
        "capability_denied",
    );
    assert_eq!(backend.0.captures.load(Ordering::SeqCst), 0);
}

#[test]
fn running_and_completed_ids_deduplicate_while_busy_rejections_are_reusable() {
    let (owner, backend) = coordinator();
    let window = windows(&owner, "windows").remove(0);
    let gate = backend.read_gate();
    let original = submit(&owner, "read", Some(&window), Command::Observe);
    assert_eq!(start(&owner, original.clone()).data["status"], "running");
    gate.entered();
    assert_eq!(start(&owner, original.clone()).data["status"], "running");
    assert_code(
        &start(
            &owner,
            submit(&owner, "read", Some(&window), Command::Capture),
        ),
        "request_conflict",
    );
    let later = submit(&owner, "later", Some(&window), Command::Observe);
    assert_code(&start(&owner, later.clone()), "busy");
    assert_code(&result(&owner, "later"), "unknown_request");
    assert_eq!(backend.0.observes.load(Ordering::SeqCst), 1);
    gate.release();
    let completed = finish(&owner, "read");
    assert_eq!(start(&owner, original).data, completed.data);
    assert_eq!(start(&owner, later).data["status"], "running");
    assert!(!finish(&owner, "later").is_error);
    assert_eq!(backend.0.observes.load(Ordering::SeqCst), 2);
}

#[test]
fn public_references_do_not_cross_contexts_or_accept_native_ids() {
    let (first, _) = coordinator();
    let (second, backend) = coordinator();
    let first_window = windows(&first, "windows").remove(0);
    let (foreign_observation, foreign_element) = observe(&first, "observe", &first_window);
    let foreign_app = submit(&first, "cross-app", None, Command::Windows);
    assert_code(&start(&second, foreign_app), "wrong_app");
    assert_code(&result(&second, "cross-app"), "unknown_request");
    assert_code(
        &run(
            &second,
            "cross-window",
            Some(&first_window),
            Command::Observe,
        ),
        "target_changed",
    );
    assert_code(
        &run(&second, "native-window", Some("101"), Command::Observe),
        "target_changed",
    );
    let window = windows(&second, "windows").remove(0);
    let (own_observation, own_element) = observe(&second, "observe", &window);
    assert_code(
        &run(
            &second,
            "cross-observation",
            Some(&window),
            press(&foreign_observation, &foreign_element),
        ),
        "invalid_observation",
    );
    assert_code(
        &run(
            &second,
            "native-element",
            Some(&window),
            press(&own_observation, "9007199254740993"),
        ),
        "invalid_observation",
    );
    assert!(
        !run(
            &second,
            "valid",
            Some(&window),
            press(&own_observation, &own_element)
        )
        .is_error
    );
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 1);
}

#[test]
fn wrong_window_rejection_preserves_binding_and_success_consumes_it_once() {
    let (owner, backend) = coordinator();
    let targets = windows(&owner, "windows");
    let (observation, element) = observe(&owner, "observe", &targets[0]);
    let rejected = run(
        &owner,
        "wrong-window",
        Some(&targets[1]),
        press(&observation, &element),
    );
    assert_code(&rejected, "target_changed");
    assert_eq!(
        rejected.data["receipt"]["outcome"]["dispatch"],
        "not_dispatched"
    );
    assert!(rejected.data["receipt"]["provider_outcome"].is_null());
    let original = submit(
        &owner,
        "press",
        Some(&targets[0]),
        press(&observation, &element),
    );
    start(&owner, original.clone());
    let completed = finish(&owner, "press");
    assert_eq!(
        completed.data["receipt"]["outcome"]["dispatch"],
        "dispatched"
    );
    assert_eq!(
        completed.data["receipt"]["outcome"]["evidence"]["kind"],
        "unverified"
    );
    assert_code(
        &run(
            &owner,
            "replay",
            Some(&targets[0]),
            press(&observation, &element),
        ),
        "stale_observation",
    );
    owner.close();
    assert_eq!(start(&owner, original.clone()).data, completed.data);
    let mut conflict = original;
    conflict.command = Command::Observe;
    assert_code(&start(&owner, conflict), "request_conflict");
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 1);
}

#[test]
fn new_observation_and_new_window_inventory_retire_old_public_refs() {
    let (owner, backend) = coordinator();
    let first_window = windows(&owner, "windows-1").remove(0);
    let (old_observation, old_element) = observe(&owner, "old", &first_window);
    let (current_observation, current_element) = observe(&owner, "current", &first_window);
    assert_code(
        &run(
            &owner,
            "old-ref",
            Some(&first_window),
            press(&old_observation, &old_element),
        ),
        "invalid_observation",
    );
    let new_window = windows(&owner, "windows-2").remove(0);
    assert_ne!(first_window, new_window);
    assert_code(
        &run(&owner, "old-window", Some(&first_window), Command::Observe),
        "target_changed",
    );
    assert_code(
        &run(
            &owner,
            "retired-ref",
            Some(&new_window),
            press(&current_observation, &current_element),
        ),
        "invalid_observation",
    );
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 0);
}

#[test]
fn semantic_and_pixel_bindings_keep_their_kind_and_rejected_points_do_not_consume() {
    let (owner, backend) = coordinator();
    let window = windows(&owner, "windows").remove(0);
    let (semantic, element) = observe(&owner, "semantic", &window);
    assert_code(
        &run(
            &owner,
            "wrong-kind",
            Some(&window),
            Command::Click {
                observation_ref: semantic.clone(),
                x: 1.0,
                y: 1.0,
            },
        ),
        "invalid_observation",
    );
    assert!(!run(&owner, "press", Some(&window), press(&semantic, &element)).is_error);
    let captured = run(&owner, "capture", Some(&window), Command::Capture);
    let pixels = captured.data["value"]["observation_ref"].as_str().unwrap();
    assert_eq!(
        captured.data["value"]["image"]["coordinate_space"],
        "image_pixels"
    );
    assert_eq!(captured.mcp()["content"][1]["type"], "image");
    assert_code(
        &run(
            &owner,
            "invalid-point",
            Some(&window),
            Command::Click {
                observation_ref: pixels.into(),
                x: -1.0,
                y: 1.0,
            },
        ),
        "invalid_point",
    );
    assert!(
        !run(
            &owner,
            "click",
            Some(&window),
            Command::Click {
                observation_ref: pixels.into(),
                x: 19.0,
                y: 9.0,
            }
        )
        .is_error
    );
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 2);
}

#[test]
fn late_cancel_of_completed_or_unknown_id_cannot_stop_the_current_job() {
    let (owner, backend) = coordinator();
    let window = windows(&owner, "completed").remove(0);
    let gate = backend.read_gate();
    start(
        &owner,
        submit(&owner, "active", Some(&window), Command::Observe),
    );
    gate.entered();
    for id in ["completed", "never-accepted"] {
        let cancelled = owner.handle(Request::Cancel {
            request_id: id.into(),
        });
        assert_eq!(cancelled.data["cancel_requested"], false);
        assert_eq!(cancelled.data["context_state"], "open");
    }
    assert_eq!(backend.0.shutdowns.load(Ordering::SeqCst), 0);
    assert_eq!(result(&owner, "active").data["status"], "running");
    gate.release();
    assert!(!finish(&owner, "active").is_error);
}

#[test]
fn cancelling_an_entered_action_retains_original_evidence_and_never_replays() {
    let (owner, backend) = coordinator();
    let window = windows(&owner, "windows").remove(0);
    let (observation, element) = observe(&owner, "observe", &window);
    let provider = ActionOutcome {
        dispatch: DispatchState::Dispatched,
        evidence: EffectEvidence::Mismatch {
            expected: "counter=1".into(),
            observed: "counter=2".into(),
        },
        error: Some(Error::BackendFailed("late provider diagnostic".into())),
    };
    backend.0.model.lock().unwrap().outcome = provider.clone();
    let gate = backend.action_gate();
    let original = submit(
        &owner,
        "action",
        Some(&window),
        press(&observation, &element),
    );
    start(&owner, original.clone());
    gate.entered();
    let cancelled = owner.handle(Request::Cancel {
        request_id: "action".into(),
    });
    assert_eq!(cancelled.data["cancel_requested"], true);
    assert_eq!(cancelled.data["context_state"], "cancelled");
    assert_eq!(result(&owner, "action").data["status"], "running");
    assert_code(
        &start(&owner, submit(&owner, "new", None, Command::Windows)),
        "cancelled",
    );
    gate.release();
    let completed = finish(&owner, "action");
    let expected = ActionReceipt {
        outcome: ActionOutcome::unknown(Error::Cancelled),
        provider_outcome: Some(provider),
        interruption: Some(Error::Cancelled),
    };
    assert_eq!(completed.data["receipt"], wire::receipt(&expected));
    assert_eq!(start(&owner, original).data, completed.data);
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 1);
}

#[test]
fn cancellation_does_not_replace_a_known_not_dispatched_provider_result() {
    let (owner, backend) = coordinator();
    let window = windows(&owner, "windows").remove(0);
    let (observation, element) = observe(&owner, "observe", &window);
    let provider = ActionOutcome::rejected(Error::BackendFailed(
        "native refused before dispatch".into(),
    ));
    backend.0.model.lock().unwrap().outcome = provider.clone();
    let gate = backend.action_gate();
    start(
        &owner,
        submit(
            &owner,
            "action",
            Some(&window),
            press(&observation, &element),
        ),
    );
    gate.entered();
    owner.handle(Request::Cancel {
        request_id: "action".into(),
    });
    gate.release();
    let expected = ActionReceipt {
        outcome: provider.clone(),
        provider_outcome: Some(provider),
        interruption: Some(Error::Cancelled),
    };
    assert_eq!(
        finish(&owner, "action").data["receipt"],
        wire::receipt(&expected)
    );
}

#[test]
fn close_then_cancel_keeps_the_closed_state_and_reports_shutdown_failures() {
    let (owner, backend) = coordinator();
    let window = windows(&owner, "windows").remove(0);
    let (observation, element) = observe(&owner, "observe", &window);
    backend.0.model.lock().unwrap().shutdown_error =
        Some(Error::BackendFailed("reap failed".into()));
    let gate = backend.action_gate();
    start(
        &owner,
        submit(
            &owner,
            "action",
            Some(&window),
            press(&observation, &element),
        ),
    );
    gate.entered();
    let closed = owner.close();
    assert!(closed.is_error);
    assert_eq!(closed.data["context_state"], "closed");
    assert_eq!(closed.data["shutdown_error"]["message"], "reap failed");
    let cancelled = owner.handle(Request::Cancel {
        request_id: "action".into(),
    });
    assert_eq!(cancelled.data["cancel_requested"], false);
    assert_eq!(cancelled.data["context_state"], "closed");
    assert_eq!(backend.0.shutdowns.load(Ordering::SeqCst), 1);
    gate.release();
    assert_eq!(
        finish(&owner, "action").data["receipt"]["interruption"]["code"],
        "closed"
    );
    assert_code(
        &start(&owner, submit(&owner, "late", None, Command::Windows)),
        "closed",
    );
}

#[test]
fn payload_eviction_preserves_receipts_and_accepted_ids_are_never_recycled() {
    let (mut owner, backend) = coordinator();
    let config = Arc::get_mut(&mut owner).unwrap();
    config.max_requests = 4;
    config.max_payload_bytes = 1024;
    let window = windows(&owner, "windows").remove(0);
    let (observation, element) = observe(&owner, "observe", &window);
    let action = submit(
        &owner,
        "action",
        Some(&window),
        press(&observation, &element),
    );
    start(&owner, action.clone());
    let completed = finish(&owner, "action");
    let capture = submit(&owner, "capture", Some(&window), Command::Capture);
    start(&owner, capture.clone());
    let expired = finish(&owner, "capture");
    assert!(expired.data["value"].is_null());
    assert_eq!(expired.data["payload_expired"], true);
    assert!(expired.image.is_none());
    assert_eq!(result(&owner, "windows").data["payload_expired"], true);
    assert_eq!(result(&owner, "observe").data["payload_expired"], true);
    assert_eq!(result(&owner, "action").data, completed.data);
    assert_code(
        &start(&owner, submit(&owner, "fifth", None, Command::Windows)),
        "request_limit",
    );
    assert_code(&result(&owner, "fifth"), "unknown_request");
    assert_eq!(start(&owner, capture).data, expired.data);
    assert_eq!(start(&owner, action).data, completed.data);
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(backend.0.captures.load(Ordering::SeqCst), 1);
}

#[test]
fn provider_panic_finishes_the_job_as_unknown_and_seals_the_context() {
    let (owner, backend) = coordinator();
    let window = windows(&owner, "windows").remove(0);
    let (observation, element) = observe(&owner, "observe", &window);
    backend.0.model.lock().unwrap().panic_action = true;
    let reply = run(
        &owner,
        "panic",
        Some(&window),
        press(&observation, &element),
    );
    assert_eq!(reply.data["receipt"]["outcome"]["dispatch"], "unknown");
    assert_eq!(reply.data["receipt"]["interruption"]["code"], "closed");
    assert!(reply.data["receipt"]["provider_outcome"].is_null());
    assert_eq!(
        owner.handle(Request::Describe).data["context"]["state"],
        "closed"
    );
    assert_code(
        &start(&owner, submit(&owner, "late", None, Command::Windows)),
        "closed",
    );
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 1);
}

fn rpc(method: &str, id: u64, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}
fn ready(owner: &Arc<Coordinator<MockBackend>>, version: &str) -> mcp::Session {
    let mut session = mcp::Session::default();
    let init = session
        .handle(
            owner,
            rpc("initialize", 1, json!({"protocolVersion":version})),
        )
        .unwrap();
    assert!(init["error"].is_null(), "{init}");
    assert_eq!(
        init["result"]["protocolVersion"],
        if version == "2024-11-05" {
            "2024-11-05"
        } else {
            "2025-06-18"
        }
    );
    assert!(session
        .handle(
            owner,
            json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        )
        .is_none());
    session
}
#[test]
fn mcp_requires_the_handshake_and_exposes_only_one_exec_tool() {
    let (owner, _) = coordinator();
    let mut session = mcp::Session::default();
    let request = rpc("tools/list", 7, json!({}));
    assert_eq!(
        session.handle(&owner, request.clone()).unwrap()["error"]["code"],
        -32002
    );
    assert_eq!(
        session
            .handle(&owner, rpc("initialize", 1, json!({})))
            .unwrap()["error"]["code"],
        -32602
    );
    let initialized = session
        .handle(
            &owner,
            rpc("initialize", 2, json!({"protocolVersion":"2025-06-18"})),
        )
        .unwrap();
    assert_eq!(initialized["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(
        session.handle(&owner, request.clone()).unwrap()["error"]["code"],
        -32002
    );
    session.handle(
        &owner,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    );
    let listed = session.handle(&owner, request).unwrap();
    let tools = listed["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "exec");
    assert_eq!(tools[0]["inputSchema"], wire::input_schema());
    assert_eq!(
        session
            .handle(
                &owner,
                rpc("initialize", 3, json!({"protocolVersion":"2025-06-18"}))
            )
            .unwrap()["error"]["code"],
        -32600
    );
    assert_eq!(
        session
            .handle(
                &owner,
                rpc("tools/call", 4, json!({"name":"raw_native","arguments":{}}))
            )
            .unwrap()["error"]["code"],
        -32602
    );
}
#[test]
fn mcp_negotiates_only_implemented_versions_and_preserves_their_result_format() {
    let (owner, _) = coordinator();
    for version in ["2024-11-05", "2025-03-26", "2025-06-18", "future"] {
        let mut session = ready(&owner, version);
        let response = session
            .handle(
                &owner,
                rpc(
                    "tools/call",
                    9,
                    json!({"name":"exec","arguments":{"version":1,"operation":"describe"}}),
                ),
            )
            .unwrap();
        let content = &response["result"]["content"];
        assert_eq!(content[0]["type"], "text");
        let text: Value = serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text["grant"]["grant_id"], "host-approved");
        if version != "2024-11-05" {
            assert_eq!(response["result"]["structuredContent"], text);
        } else {
            assert!(response["result"].get("structuredContent").is_none());
        }
    }
}
#[test]
fn mcp_arguments_cannot_supply_or_expand_host_authority() {
    let (owner, backend) = coordinator();
    let mut session = ready(&owner, "2025-06-18");
    for authority_field in ["grant", "approved", "capabilities", "pid"] {
        let mut arguments = json!({"version":1,"operation":"describe"});
        arguments[authority_field] = json!({"capabilities":["click"],"approved":true});
        let response = session
            .handle(
                &owner,
                rpc(
                    "tools/call",
                    9,
                    json!({"name":"exec","arguments":arguments}),
                ),
            )
            .unwrap();
        assert_eq!(response["result"]["isError"], true);
        assert_eq!(
            response["result"]["structuredContent"]["error"]["code"],
            "invalid_request"
        );
    }
    assert_eq!(
        owner.handle(Request::Describe).data["grant"]["grant_id"],
        "host-approved"
    );
    assert_eq!(backend.0.windows.load(Ordering::SeqCst), 0);
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 0);
}
#[test]
fn mcp_cancellation_notification_does_not_cancel_an_accepted_native_job() {
    let (owner, backend) = coordinator();
    let window = windows(&owner, "windows").remove(0);
    let mut session = ready(&owner, "2025-06-18");
    let gate = backend.read_gate();
    let app = owner.handle(Request::Describe).data["app"]["app_ref"].clone();
    let accepted = session
        .handle(
            &owner,
            rpc(
                "tools/call",
                41,
                json!({"name":"exec","arguments":{
                    "version":1,"operation":"submit","request_id":"native-job",
                    "target":{"app_ref":app,"window_ref":window},"command":{"kind":"observe"}
                }}),
            ),
        )
        .unwrap();
    assert_eq!(accepted["result"]["structuredContent"]["status"], "running");
    gate.entered();
    assert!(session
        .handle(
            &owner,
            json!({"jsonrpc":"2.0","method":"notifications/cancelled",
        "params":{"requestId":41,"reason":"Stop waiting","request_id":"native-job"}})
        )
        .is_none());
    assert_eq!(
        owner.handle(Request::Describe).data["context"]["state"],
        "open"
    );
    assert_eq!(backend.0.shutdowns.load(Ordering::SeqCst), 0);
    assert_eq!(result(&owner, "native-job").data["status"], "running");
    gate.release();
    assert!(!finish(&owner, "native-job").is_error);
    assert_eq!(backend.0.observes.load(Ordering::SeqCst), 1);
}
#[test]
fn mcp_frames_require_complete_lines_and_never_accumulate_past_64_kib() {
    use std::io::{BufReader, Cursor, Read};
    let mut exact = vec![b' '; mcp::MAX_INPUT_BYTES - 1];
    exact.push(b'\n');
    assert_eq!(
        mcp::read_frame(&mut Cursor::new(exact.clone()))
            .unwrap()
            .unwrap(),
        exact
    );
    let mut oversized = Cursor::new(vec![b'x'; mcp::MAX_INPUT_BYTES * 3]);
    assert_eq!(
        mcp::read_frame(&mut oversized).unwrap_err().code,
        "invalid_frame"
    );
    assert_eq!(oversized.position(), (mcp::MAX_INPUT_BYTES + 1) as u64);
    assert_eq!(
        mcp::read_frame(&mut Cursor::new(b"{}".to_vec()))
            .unwrap_err()
            .code,
        "invalid_frame"
    );
    let mut two = Cursor::new(b"{}\n[]\n".to_vec());
    assert_eq!(mcp::read_frame(&mut two).unwrap(), Some(b"{}\n".to_vec()));
    assert_eq!(mcp::read_frame(&mut two).unwrap(), Some(b"[]\n".to_vec()));
    assert_eq!(mcp::read_frame(&mut two).unwrap(), None);
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("controlled input failure"))
        }
    }
    assert_eq!(
        mcp::read_frame(&mut BufReader::new(Broken))
            .unwrap_err()
            .code,
        "transport_error"
    );
}

#[test]
fn close_after_cancel_keeps_the_original_cancellation_cause_in_late_receipts() {
    let (owner, backend) = coordinator();
    let window = windows(&owner, "windows").remove(0);
    let (observation, element) = observe(&owner, "observe", &window);
    let provider = ActionOutcome {
        dispatch: DispatchState::Dispatched,
        evidence: EffectEvidence::Confirmed("controlled postcondition".into()),
        error: None,
    };
    backend.0.model.lock().unwrap().outcome = provider.clone();
    let gate = backend.action_gate();
    start(
        &owner,
        submit(
            &owner,
            "action",
            Some(&window),
            press(&observation, &element),
        ),
    );
    gate.entered();
    let cancelled = owner.handle(Request::Cancel {
        request_id: "action".into(),
    });
    assert_eq!(cancelled.data["cancel_requested"], true);
    assert_eq!(cancelled.data["context_state"], "cancelled");
    let closed = owner.close();
    assert!(!closed.is_error);
    assert_eq!(closed.data["context_state"], "cancelled");
    assert_eq!(result(&owner, "action").data["status"], "running");
    gate.release();
    let expected = ActionReceipt {
        outcome: ActionOutcome::unknown(Error::Cancelled),
        provider_outcome: Some(provider),
        interruption: Some(Error::Cancelled),
    };
    assert_eq!(
        finish(&owner, "action").data["receipt"],
        wire::receipt(&expected)
    );
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 1);
    assert_code(
        &start(&owner, submit(&owner, "late", None, Command::Windows)),
        "cancelled",
    );
}

#[test]
fn mcp_request_ids_accept_integer_valued_numbers_and_preserve_the_received_value() {
    let (owner, _) = coordinator();
    for literal in ["1", "-2", "1.0", "1e0", "-0.0", "\"rpc-1\""] {
        // Each independent session uses its ID once. Parse actual JSON so the
        // float and exponent spellings exercise serde_json's numeric variant.
        let request: Value = serde_json::from_str(&format!(
            r#"{{"jsonrpc":"2.0","method":"ping","id":{literal}}}"#
        ))
        .unwrap();
        let received_id = request["id"].clone();
        let response = mcp::Session::default().handle(&owner, request).unwrap();
        assert!(response["error"].is_null(), "{literal}: {response}");
        assert_eq!(response["id"], received_id, "{literal}");
        assert_eq!(response["result"], json!({}));
    }
    for literal in ["1.5", "null", "true", "false", "[]", "{}"] {
        let request: Value = serde_json::from_str(&format!(
            r#"{{"jsonrpc":"2.0","method":"ping","id":{literal}}}"#
        ))
        .unwrap();
        let response = mcp::Session::default().handle(&owner, request).unwrap();
        assert_eq!(response["error"]["code"], -32600, "{literal}: {response}");
        assert!(response["id"].is_null());
        assert!(response.get("result").is_none());
    }
}

#[path = "script_tests.rs"]
mod script_tests;
