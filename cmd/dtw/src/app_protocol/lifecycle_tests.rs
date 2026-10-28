use super::*;

fn expiring(ttl: Duration) -> (Arc<Coordinator<MockBackend>>, MockBackend, Instant) {
    let backend = MockBackend::new();
    let mut grant = authority();
    grant.expires_at = Instant::now() + ttl;
    grant.expires_at_unix_ms -= 60_000 - ttl.as_millis() as u64;
    let deadline = grant.expires_at;
    (
        Coordinator::new(backend.clone(), grant, "Controlled".into()),
        backend,
        deadline,
    )
}

fn reach(deadline: Instant) {
    while Instant::now() < deadline {
        std::thread::park_timeout(deadline.saturating_duration_since(Instant::now()));
    }
}

fn script(source: impl Into<String>) -> Command {
    Command::Script {
        source: source.into(),
        timeout_ms: 5000,
        max_calls: 32,
    }
}

fn assert_expired(owner: &Arc<Coordinator<MockBackend>>) {
    let context = owner.handle(Request::Describe).data["context"].clone();
    assert_eq!(context["state"], "closed", "{context}");
    assert_eq!(
        context["terminal_reason"]["code"], "grant_expired",
        "{context}"
    );
}

#[test]
fn expired_constructor_closes_without_dispatch_or_repeated_idle_cleanup() {
    let backend = MockBackend::new();
    let mut grant = authority();
    grant.expires_at = Instant::now() - Duration::from_secs(1);
    let owner = Coordinator::new(backend.clone(), grant, "Controlled".into());
    for _ in 0..3 {
        assert_expired(&owner);
        owner.expire_if_needed();
        assert_code(
            &start(&owner, submit(&owner, "never", None, Command::Windows)),
            "grant_expired",
        );
    }
    assert_eq!(backend.0.shutdowns.load(Ordering::SeqCst), 1);
    assert_eq!(backend.0.windows.load(Ordering::SeqCst), 0);
    assert_code(&result(&owner, "never"), "unknown_request");
    assert!(owner.state.lock().unwrap().jobs.is_empty());
}

#[test]
fn idle_expiry_preserves_receipts_dedup_and_request_history() {
    let (owner, backend, deadline) = expiring(Duration::from_secs(1));
    let window = &windows(&owner, "windows")[0];
    let (observation, element) = observe(&owner, "observe", window);
    let submitted = submit(&owner, "press", Some(window), press(&observation, &element));
    start(&owner, submitted.clone());
    let original = finish(&owner, "press");
    assert!(!original.is_error, "{}", original.data);
    reach(deadline);
    owner.expire_if_needed();
    assert_expired(&owner);
    assert_eq!(result(&owner, "press").data, original.data);
    assert_eq!(start(&owner, submitted).data, original.data);
    assert_code(
        &start(&owner, submit(&owner, "late", None, Command::Windows)),
        "grant_expired",
    );
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(backend.0.shutdowns.load(Ordering::SeqCst), 1);
    let state = owner.state.lock().unwrap();
    assert!(state.windows.is_empty());
    assert!(state.observation.is_none());
    assert_eq!(state.jobs.len(), 3);
}

#[test]
fn pure_javascript_expiry_uses_core_cause_and_does_not_commit_state() {
    let (owner, backend, _) = expiring(Duration::from_millis(100));
    let reply = run(
        &owner,
        "loop",
        None,
        script("state.saved = true; while (true) {}"),
    );
    assert_code(&reply, "grant_expired");
    assert_eq!(reply.data["script"]["failure"]["code"], "grant_expired");
    assert_eq!(reply.data["script"]["state_committed"], false);
    assert_eq!(reply.data["script"]["steps"], json!([]));
    assert_eq!(owner.state.lock().unwrap().script_state, json!({}));
    assert_expired(&owner);
    assert!(backend.0.shutdowns.load(Ordering::SeqCst) > 0);
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 0);
    let cancel = owner.handle(Request::Cancel {
        request_id: "loop".into(),
    });
    assert_eq!(cancel.data["cancel_requested"], false);
    assert_eq!(cancel.data["terminal_reason"]["code"], "grant_expired");
}

#[test]
fn expiry_during_script_action_preserves_late_provider_evidence_after_close() {
    let (owner, backend, deadline) = expiring(Duration::from_secs(1));
    let window = &windows(&owner, "windows")[0];
    let (observation, element) = observe(&owner, "observe", window);
    backend.0.model.lock().unwrap().outcome.evidence =
        EffectEvidence::Confirmed("provider proof".into());
    let gate = backend.action_gate();
    let source = format!(
        "state.changed=true; return await app.press({});",
        json!({
            "window_ref":window,"observation_ref":observation,"element_ref":element
        })
    );
    start(&owner, submit(&owner, "late-action", None, script(source)));
    gate.entered();
    reach(deadline);
    owner.expire_if_needed();
    let closed = owner.close();
    assert_eq!(closed.data["terminal_reason"]["code"], "grant_expired");
    assert_eq!(
        owner
            .handle(Request::Cancel {
                request_id: "late-action".into()
            })
            .data["cancel_requested"],
        false
    );
    gate.release();
    let reply = finish(&owner, "late-action");
    assert_code(&reply, "grant_expired");
    let receipt = &reply.data["script"]["steps"][0]["receipt"];
    assert_eq!(receipt["outcome"]["dispatch"], "unknown", "{receipt}");
    assert_eq!(receipt["interruption"]["code"], "grant_expired");
    assert_eq!(receipt["provider_outcome"]["dispatch"], "dispatched");
    assert_eq!(
        receipt["provider_outcome"]["evidence"],
        json!({"kind":"confirmed","postcondition":"provider proof"})
    );
    assert_eq!(reply.data["script"]["state_committed"], false);
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 1);
    assert_expired(&owner);
}

#[test]
fn atomic_expiry_reports_grant_expired_with_its_original_action_receipt() {
    let (owner, backend, deadline) = expiring(Duration::from_secs(1));
    let window = &windows(&owner, "windows")[0];
    let (observation, element) = observe(&owner, "observe", window);
    let gate = backend.action_gate();
    start(
        &owner,
        submit(&owner, "late", Some(window), press(&observation, &element)),
    );
    gate.entered();
    reach(deadline);
    owner.expire_if_needed();
    gate.release();
    let reply = finish(&owner, "late");
    assert_code(&reply, "grant_expired");
    assert_eq!(
        reply.data["receipt"]["interruption"]["code"],
        "grant_expired"
    );
    assert_eq!(
        reply.data["receipt"]["provider_outcome"]["dispatch"],
        "dispatched"
    );
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 1);
    assert_expired(&owner);
}

#[test]
fn idle_cleanup_failure_is_visible_and_does_not_reopen_the_context() {
    let backend = MockBackend::new();
    backend.0.model.lock().unwrap().shutdown_error =
        Some(Error::BackendFailed("controlled cleanup failure".into()));
    let mut grant = authority();
    grant.expires_at = Instant::now();
    let owner = Coordinator::new(backend.clone(), grant, "Controlled".into());
    let description = owner.handle(Request::Describe);
    assert_eq!(
        description.data["context"]["shutdown_error"]["message"],
        "controlled cleanup failure"
    );
    assert_expired(&owner);
    assert!(owner.close().is_error);
    assert_code(
        &start(&owner, submit(&owner, "late", None, Command::Windows)),
        "grant_expired",
    );
    assert_eq!(backend.0.windows.load(Ordering::SeqCst), 0);
}

#[test]
fn explicit_close_remains_the_first_cause_after_grant_expiry() {
    let (owner, backend, deadline) = expiring(Duration::from_millis(100));
    assert_eq!(owner.close().data["terminal_reason"]["code"], "closed");
    reach(deadline);
    owner.expire_if_needed();
    assert_eq!(
        owner.handle(Request::Describe).data["context"]["terminal_reason"]["code"],
        "closed"
    );
    assert_eq!(backend.0.shutdowns.load(Ordering::SeqCst), 1);
}

#[test]
fn cancel_admission_rechecks_expiry_after_an_earlier_entry_check() {
    let (owner, backend, deadline) = expiring(Duration::from_secs(1));
    let window = &windows(&owner, "windows")[0];
    let gate = backend.read_gate();
    start(
        &owner,
        submit(&owner, "read", Some(window), Command::Observe),
    );
    gate.entered();
    reach(deadline);
    // Invoke the admission boundary directly, modelling a handle precheck that
    // passed before the deadline and then waited for the state lock.
    let cancelled = owner.cancel("read");
    assert_eq!(cancelled.data["cancel_requested"], false);
    assert_eq!(cancelled.data["terminal_reason"]["code"], "grant_expired");
    gate.release();
    assert_code(&finish(&owner, "read"), "grant_expired");
}

#[test]
fn late_cleanup_error_stays_with_the_job_that_owned_the_cleanup() {
    let (owner, backend, deadline) = expiring(Duration::from_secs(1));
    let window = &windows(&owner, "windows")[0];
    let read = backend.read_gate();
    let (worker, cleanup) = TestGate::pair();
    backend.0.model.lock().unwrap().shutdown_gate =
        Some((worker, Error::BackendFailed("late cleanup failure".into())));
    start(
        &owner,
        submit(
            &owner,
            "read",
            None,
            script(format!(
                "return await app.observe({});",
                json!({"window_ref":window})
            )),
        ),
    );
    read.entered();
    reach(deadline);
    let watcher = owner.clone();
    let shutdown = std::thread::spawn(move || watcher.expire_if_needed());
    cleanup.entered();
    read.release();
    let completed = finish(&owner, "read");
    assert_code(&completed, "grant_expired");
    assert!(completed.data["script"]["shutdown_error"].is_null());
    cleanup.release();
    shutdown.join().unwrap();
    let retained = result(&owner, "read");
    assert_eq!(
        retained.data["script"]["shutdown_error"]["message"],
        "late cleanup failure"
    );
    assert_eq!(retained.data["error"], completed.data["error"]);
    assert_eq!(
        retained.data["script"]["steps"],
        completed.data["script"]["steps"]
    );
}

// Validate the declared data shape against actual values produced by all five
// methods. This is deliberately a small test matcher, not a protocol validator.
fn matches_contract(schema: &Value, value: &Value) -> bool {
    for alternatives in ["oneOf", "anyOf"] {
        if let Some(choices) = schema[alternatives].as_array() {
            let matches = choices
                .iter()
                .filter(|s| matches_contract(s, value))
                .count();
            return if alternatives == "oneOf" {
                matches == 1
            } else {
                matches > 0
            };
        }
    }
    if let Some(expected) = schema.get("const") {
        return value == expected;
    }
    if let Some(choices) = schema["enum"].as_array() {
        return choices.contains(value);
    }
    match schema["type"].as_str() {
        Some("object") => value.as_object().is_some_and(|object| {
            let required = schema["required"].as_array();
            required.is_none_or(|keys| {
                keys.iter()
                    .all(|k| object.contains_key(k.as_str().unwrap()))
            }) && object.iter().all(|(key, value)| {
                if let Some(child) = schema["properties"].get(key) {
                    matches_contract(child, value)
                } else if schema["additionalProperties"].is_object() {
                    matches_contract(&schema["additionalProperties"], value)
                } else {
                    schema["additionalProperties"] != false
                }
            })
        }),
        Some("array") => value
            .as_array()
            .is_some_and(|array| array.iter().all(|v| matches_contract(&schema["items"], v))),
        Some("string") => value.is_string(),
        Some("boolean") => value.is_boolean(),
        Some("integer") => value.is_i64() || value.is_u64(),
        Some("number") => value.is_number(),
        Some("null") => value.is_null(),
        None => true,
        other => panic!("unsupported contract test shape: {other:?}"),
    }
}

#[test]
fn advertised_method_returns_match_all_five_actual_script_values() {
    for evidence in [
        EffectEvidence::Unverified,
        EffectEvidence::Confirmed("proof".into()),
        EffectEvidence::Mismatch {
            expected: "before".into(),
            observed: "after".into(),
        },
    ] {
        let (owner, backend) = coordinator();
        backend.0.model.lock().unwrap().outcome.evidence = evidence;
        let contract = owner.handle(Request::Describe).data["script"].clone();
        let reply = run(
            &owner,
            "all",
            None,
            script(
                r#"
            const windows = await app.windows();
            const window_ref = windows.windows[0].window_ref;
            const observe = await app.observe({window_ref});
            const press = await app.press({window_ref, observation_ref:observe.observation_ref, element_ref:observe.elements[0].element_ref});
            const capture = await app.capture({window_ref});
            const click = await app.click({window_ref, observation_ref:capture.observation_ref, x:1, y:2});
            return {windows, observe, press, capture, click};
        "#,
            ),
        );
        assert!(!reply.is_error, "{}", reply.data);
        for method in ["windows", "observe", "press", "capture", "click"] {
            let value = &reply.data["value"]["result"][method];
            let schema = &contract["methods"][method]["returns"];
            assert!(schema.is_object(), "missing {method} return contract");
            assert!(
                matches_contract(schema, value),
                "{method}: {value}\ncontract: {schema}"
            );
        }
    }
}

#[test]
fn advertised_composition_example_runs_after_explicit_target_selection() {
    let (owner, backend) = coordinator();
    let contract = owner.handle(Request::Describe).data["script"].clone();
    let source = contract["example"]["source"]
        .as_str()
        .unwrap()
        .replace("Target window", "Window 101")
        .replace("Target control", "Increment");
    let reply = run(&owner, "example", None, script(source));
    assert!(!reply.is_error, "{}", reply.data);
    assert_eq!(reply.data["script"]["state_committed"], true);
    assert_eq!(reply.data["value"]["calls"], 5);
    assert_eq!(
        owner.state.lock().unwrap().script_state,
        json!({"completed":1})
    );
    assert!(reply.image.is_some());
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 1);
    assert!(matches_contract(
        &contract["methods"]["observe"]["returns"],
        &reply.data["value"]["result"]["after"]
    ));
}
