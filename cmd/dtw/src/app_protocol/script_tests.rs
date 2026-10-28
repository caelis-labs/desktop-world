//! Coordinator integration: a controlled Backend is not native GUI evidence.
use super::*;

const SETUP: &str = r#"
const inventory = await app.windows();
const window_ref = inventory.windows[0].window_ref;
const observation = await app.observe({window_ref});
"#;
const PRESS: &str = r#"
await app.press({window_ref, observation_ref:observation.observation_ref,
    element_ref:observation.elements[0].element_ref});
"#;
fn script(source: impl Into<String>) -> Command {
    Command::Script {
        source: source.into(),
        timeout_ms: 5000,
        max_calls: 32,
    }
}
fn run_script(owner: &Arc<Coordinator<MockBackend>>, id: &str, source: impl Into<String>) -> Reply {
    run(owner, id, None, script(source))
}
fn steps(reply: &Reply) -> &Vec<Value> {
    reply.data["script"]["steps"]
        .as_array()
        .expect("script ledger is absent")
}
fn operation_list(reply: &Reply) -> Vec<&str> {
    steps(reply)
        .iter()
        .map(|s| s["operation"].as_str().unwrap())
        .collect()
}
fn last_step_error(reply: &Reply, code: &str) {
    assert!(reply.is_error, "{}", reply.data);
    assert_eq!(
        steps(reply).last().unwrap()["error"]["code"],
        code,
        "{}",
        reply.data
    );
    assert_eq!(reply.data["script"]["state_committed"], false);
}

#[test]
fn script_composes_semantic_and_pixel_calls_under_one_app() {
    let (owner, backend) = coordinator();
    let completed = run_script(
        &owner,
        "sequence",
        format!(
            r#"{SETUP}{PRESS}
const reobserved = await app.observe({{window_ref}});
const pixels = await app.capture({{window_ref}});
await app.click({{window_ref,observation_ref:pixels.observation_ref,x:1,y:2}});
const verified = await app.observe({{window_ref}});
state.lastWindow = window_ref;
return {{label:verified.elements[0].label, refreshed:reobserved.observation_ref !== observation.observation_ref}};
"#
        ),
    );
    assert!(!completed.is_error, "{}", completed.data);
    assert_eq!(
        operation_list(&completed),
        ["windows", "observe", "press", "observe", "capture", "click", "observe"]
    );
    assert_eq!(
        completed.data["value"]["result"],
        json!({"label":"Increment","refreshed":true})
    );
    assert_eq!(completed.data["value"]["calls"], 7);
    assert_eq!(completed.data["script"]["state_committed"], true);
    assert!(completed.data["script"]["failure"].is_null());
    assert!(completed.image.is_some());
    assert_eq!(completed.data["value"]["last_capture"]["step"], 5);
    for (index, step) in steps(&completed).iter().enumerate() {
        assert_eq!(step["number"], index + 1);
        assert!(step["line"].as_u64().unwrap_or(0) > 0, "{step}");
        assert_eq!(step["status"], "completed");
        assert!(step["error"].is_null());
    }
    for index in [2, 5] {
        assert_eq!(
            steps(&completed)[index]["receipt"]["outcome"]["dispatch"],
            "dispatched"
        );
        assert_eq!(
            steps(&completed)[index]["receipt"]["provider_outcome"]["dispatch"],
            "dispatched"
        );
    }
    assert_eq!(backend.0.windows.load(Ordering::SeqCst), 1);
    assert_eq!(backend.0.observes.load(Ordering::SeqCst), 3);
    assert_eq!(backend.0.captures.load(Ordering::SeqCst), 1);
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 2);
}

#[test]
fn scripts_cannot_supply_another_app_or_forge_context_window_references() {
    let (first, _) = coordinator();
    let (second, backend) = coordinator();
    let foreign = submit(
        &first,
        "cross-app",
        None,
        script("return await app.windows();"),
    );
    assert_code(&start(&second, foreign), "wrong_app");
    assert_code(&result(&second, "cross-app"), "unknown_request");
    let foreign_window = windows(&first, "windows").remove(0);
    for (id, window) in [("foreign", foreign_window.as_str()), ("native", "101")] {
        let source = format!(
            "return await app.observe({{window_ref:{}}});",
            json!(window)
        );
        last_step_error(&run_script(&second, id, source), "target_changed");
    }
    let source = format!(
        "return await app.observe({{window_ref:\"101\",app_ref:{}}});",
        json!(first.app_ref)
    );
    let rejected = run_script(&second, "model-app", source);
    assert!(rejected.is_error);
    assert_eq!(rejected.data["script"]["failure"]["code"], "invalid_args");
    assert!(steps(&rejected).is_empty());
    assert_eq!(rejected.data["script"]["state_committed"], false);
    assert_eq!(backend.0.windows.load(Ordering::SeqCst), 0);
    assert_eq!(backend.0.observes.load(Ordering::SeqCst), 0);
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 0);
}

#[test]
fn exception_keeps_prior_effect_receipt_but_only_success_commits_state() {
    let (owner, backend) = coordinator();
    let seeded = run_script(&owner, "seed", "state.total=7; return state.total;");
    assert_eq!(seeded.data["value"]["result"], 7);
    assert_eq!(seeded.data["script"]["state_committed"], true);
    let original = submit(
        &owner,
        "throws",
        None,
        script(format!(
            "{SETUP}{PRESS}state.total=99; throw new Error('after effect');"
        )),
    );
    start(&owner, original.clone());
    let failed = finish(&owner, "throws");
    assert!(failed.is_error);
    assert_eq!(failed.data["script"]["state_committed"], false);
    assert_eq!(
        failed.data["script"]["failure"]["message"],
        "Error: after effect"
    );
    assert_eq!(
        steps(&failed)[2]["receipt"]["outcome"]["dispatch"],
        "dispatched"
    );
    assert_eq!(start(&owner, original.clone()).data, failed.data);
    let mut conflict = original;
    conflict.command = script("return 8;");
    assert_code(&start(&owner, conflict), "request_conflict");
    let state = run_script(&owner, "state-read", "return state.total;");
    assert_eq!(state.data["value"]["result"], 7);
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 1);
}

#[test]
fn active_script_reserves_one_slot_and_running_ids_never_replay() {
    let (owner, backend) = coordinator();
    let gate = backend.read_gate();
    let original = submit(
        &owner,
        "active-script",
        None,
        script(format!(
            "{SETUP}{PRESS}return await app.observe({{window_ref}});"
        )),
    );
    assert_eq!(start(&owner, original.clone()).data["status"], "running");
    gate.entered();
    let current = result(&owner, "active-script");
    assert_eq!(operation_list(&current), ["windows", "observe"]);
    assert_eq!(steps(&current)[1]["status"], "running");
    assert_eq!(start(&owner, original.clone()).data["status"], "running");
    for (id, command) in [
        ("atomic", Command::Windows),
        ("another-script", script("return 2;")),
    ] {
        assert_code(&start(&owner, submit(&owner, id, None, command)), "busy");
        assert_code(&result(&owner, id), "unknown_request");
    }
    assert_eq!(backend.0.windows.load(Ordering::SeqCst), 1);
    assert_eq!(backend.0.observes.load(Ordering::SeqCst), 1);
    gate.release();
    let done = finish(&owner, "active-script");
    assert!(!done.is_error, "{}", done.data);
    assert_eq!(start(&owner, original).data, done.data);
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(backend.0.observes.load(Ordering::SeqCst), 2);
}

#[test]
fn script_cancel_and_close_keep_late_provider_evidence_without_replay() {
    for close in [false, true] {
        let (owner, backend) = coordinator();
        let provider = ActionOutcome {
            dispatch: DispatchState::Dispatched,
            evidence: EffectEvidence::Mismatch {
                expected: "counter=1".into(),
                observed: "counter=2".into(),
            },
            error: Some(Error::BackendFailed("late diagnostic".into())),
        };
        backend.0.model.lock().unwrap().outcome = provider.clone();
        let gate = backend.action_gate();
        let original = submit(
            &owner,
            "stopping",
            None,
            script(format!(
                "state.pending=true; {SETUP}{PRESS}state.committed=true; return true;"
            )),
        );
        start(&owner, original.clone());
        gate.entered();
        assert_eq!(steps(&result(&owner, "stopping"))[2]["status"], "running");
        let reason = if close {
            owner.close();
            Error::Closed
        } else {
            let cancelled = owner.handle(Request::Cancel {
                request_id: "stopping".into(),
            });
            assert_eq!(cancelled.data["cancel_requested"], true);
            owner.close(); // Preserve the first terminal cause even when cleanup repeats.
            Error::Cancelled
        };
        assert_eq!(result(&owner, "stopping").data["status"], "running");
        gate.release();
        let done = finish(&owner, "stopping");
        assert!(done.is_error);
        assert_eq!(done.data["script"]["state_committed"], false);
        assert_eq!(owner.state.lock().unwrap().script_state, json!({}));
        assert_eq!(
            steps(&done)[2]["receipt"],
            wire::receipt(&ActionReceipt {
                outcome: ActionOutcome::unknown(reason.clone()),
                provider_outcome: Some(provider),
                interruption: Some(reason)
            })
        );
        assert_eq!(start(&owner, original).data, done.data);
        assert_eq!(backend.0.actions.load(Ordering::SeqCst), 1);
        assert!(backend.0.shutdowns.load(Ordering::SeqCst) > 0);
    }
}

#[test]
fn cancelling_a_script_already_stopping_keeps_the_first_terminal_cause() {
    for (reason, code) in [(Error::TimedOut, "timed_out")] {
        let (owner, backend) = coordinator();
        let gate = backend.read_gate();
        start(
            &owner,
            submit(
                &owner,
                "already-stopping",
                None,
                script(format!("{SETUP}state.after=true; return true;")),
            ),
        );
        gate.entered();
        {
            let mut state = owner.state.lock().unwrap();
            assert_eq!(state.active.as_deref(), Some("already-stopping"));
            // Freeze the exact cleanup window: a terminal cause exists while
            // the accepted script still owns its active slot and native read.
            state.terminal = Some(reason.clone());
        }
        let reply = owner.handle(Request::Cancel {
            request_id: "already-stopping".into(),
        });
        assert!(!reply.is_error);
        assert_eq!(reply.data["cancel_requested"], false);
        assert_eq!(reply.data["context_state"], "closed");
        assert_eq!(owner.state.lock().unwrap().terminal, Some(reason.clone()));
        assert_eq!(backend.0.shutdowns.load(Ordering::SeqCst), 0);
        assert_eq!(result(&owner, "already-stopping").data["status"], "running");

        gate.release();
        let done = finish(&owner, "already-stopping");
        assert_code(&done, code);
        assert_eq!(done.data["script"]["state_committed"], false);
        let state = owner.state.lock().unwrap();
        assert_eq!(state.terminal, Some(reason));
        assert_eq!(state.script_state, json!({}));
        assert_eq!(backend.0.actions.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn script_deadline_stops_a_polling_native_call_and_reports_shutdown_failure() {
    for failed_shutdown in [false, true] {
        let (owner, backend) = coordinator();
        {
            let mut model = backend.0.model.lock().unwrap();
            model.poll_action = true;
            if failed_shutdown {
                model.shutdown_error =
                    Some(Error::BackendFailed("controlled cleanup failure".into()));
            }
        }
        let gate = backend.action_gate();
        let command = Command::Script {
            source: format!("state.pending=true; {SETUP}{PRESS}state.after=true; return true;"),
            timeout_ms: 300,
            max_calls: 32,
        };
        start(&owner, submit(&owner, "timeout", None, command));
        gate.entered();
        // Keep the simulated native reply withheld. Only CallContext's budget
        // can release this operation; no later script may reuse this context.
        let done = finish(&owner, "timeout");
        gate.release();
        assert!(done.is_error, "{}", done.data);
        assert_eq!(done.data["script"]["state_committed"], false);
        assert_eq!(owner.state.lock().unwrap().script_state, json!({}));
        assert_eq!(steps(&done)[2]["receipt"]["outcome"]["dispatch"], "unknown");
        assert_eq!(
            steps(&done)[2]["receipt"]["provider_outcome"]["error"]["code"],
            "timed_out"
        );
        assert_eq!(
            steps(&done)[2]["receipt"]["interruption"]["code"],
            "timed_out"
        );
        assert!(backend.0.shutdowns.load(Ordering::SeqCst) > 0);
        assert_eq!(
            owner.handle(Request::Describe).data["context"]["state"],
            "closed"
        );
        assert_eq!(backend.0.actions.load(Ordering::SeqCst), 1);
        if failed_shutdown {
            assert_eq!(
                done.data["script"]["shutdown_error"]["message"],
                "controlled cleanup failure"
            );
        } else {
            assert!(done.data["script"]["shutdown_error"].is_null());
        }
    }
}

#[test]
fn script_payload_eviction_keeps_step_receipts_and_deduplication() {
    let (mut owner, backend) = coordinator();
    Arc::get_mut(&mut owner).unwrap().max_payload_bytes = 512;
    let original = submit(
        &owner,
        "evicted-script",
        None,
        script(format!(
            "{SETUP}{PRESS}await app.capture({{window_ref}}); return 'payload';"
        )),
    );
    start(&owner, original.clone());
    let done = finish(&owner, "evicted-script");
    assert!(!done.is_error, "{}", done.data);
    assert_eq!(done.data["payload_expired"], true);
    assert!(done.data["value"].is_null());
    assert!(done.image.is_none());
    assert_eq!(
        operation_list(&done),
        ["windows", "observe", "press", "capture"]
    );
    assert_eq!(
        steps(&done)[2]["receipt"]["outcome"]["dispatch"],
        "dispatched"
    );
    assert_eq!(done.data["script"]["state_committed"], true);
    assert_eq!(start(&owner, original).data, done.data);
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 1);
}

#[test]
fn provider_panic_keeps_prior_and_pending_script_effect_records() {
    let (owner, backend) = coordinator();
    let first = backend.action_gate();
    let source = format!(
        r#"{SETUP}{PRESS}
const next=await app.observe({{window_ref}});
await app.press({{window_ref,observation_ref:next.observation_ref,element_ref:next.elements[0].element_ref}});
state.after=true;
"#
    );
    let original = submit(&owner, "panic", None, script(source));
    start(&owner, original.clone());
    first.entered();
    // The first call already captured panic=false. Only the following call sees
    // this change; a second barrier lets us inspect the pending ledger entry.
    {
        let mut model = backend.0.model.lock().unwrap();
        model.panic_action = true;
        model.shutdown_error = Some(Error::BackendFailed("panic cleanup failed".into()));
    }
    let second = backend.action_gate();
    first.release();
    second.entered();
    let pending = result(&owner, "panic");
    assert_eq!(
        steps(&pending)[2]["receipt"]["outcome"]["dispatch"],
        "dispatched"
    );
    assert_eq!(steps(&pending)[4]["status"], "running");
    second.release();
    let done = finish(&owner, "panic");
    assert!(done.is_error);
    assert_eq!(done.data["script"]["state_committed"], false);
    assert_eq!(
        operation_list(&done),
        ["windows", "observe", "press", "observe", "press"]
    );
    assert_eq!(
        steps(&done)[2]["receipt"]["outcome"]["dispatch"],
        "dispatched"
    );
    assert_eq!(steps(&done)[4]["status"], "completed");
    assert_eq!(steps(&done)[4]["receipt"]["outcome"]["dispatch"], "unknown");
    assert!(steps(&done)[4]["receipt"]["provider_outcome"].is_null());
    assert_eq!(steps(&done)[4]["receipt"]["interruption"]["code"], "closed");
    assert_eq!(start(&owner, original).data, done.data);
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 2);
    assert!(backend.0.shutdowns.load(Ordering::SeqCst) > 0);
    assert!(!done.data["script"]["failure"].is_null());
    assert_eq!(
        done.data["script"]["shutdown_error"]["message"],
        "panic cleanup failed"
    );
}

#[test]
fn script_call_limit_preserves_prior_effect_and_rejects_further_calls() {
    let (owner, backend) = coordinator();
    let command = Command::Script {
        source: format!("{SETUP}{PRESS}await app.observe({{window_ref}}); state.after=true;"),
        timeout_ms: 5000,
        max_calls: 3,
    };
    start(&owner, submit(&owner, "limit", None, command));
    let done = finish(&owner, "limit");
    assert!(done.is_error);
    assert_eq!(done.data["script"]["state_committed"], false);
    assert_eq!(operation_list(&done), ["windows", "observe", "press"]);
    assert_eq!(
        steps(&done)[2]["receipt"]["outcome"]["dispatch"],
        "dispatched"
    );
    assert_eq!(backend.0.observes.load(Ordering::SeqCst), 1);
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 1);
}

#[test]
fn unproven_inventory_preserves_diagnostics_without_state_or_retry() {
    let (owner, backend) = coordinator();
    let message = "window_inventory_unproven: ax_windows=0, cg_eligible=6";
    backend.0.model.lock().unwrap().windows_error = Some(Error::BackendFailed(message.into()));
    let atomic = run(&owner, "atomic-inventory", None, Command::Windows);
    assert!(atomic.is_error);
    assert!(atomic.data["value"].is_null());
    let source = "state.changed=true; return await app.windows();";
    let scripted = run_script(&owner, "script-inventory", source);
    last_step_error(&scripted, "backend_failed");
    assert_eq!(steps(&scripted)[0]["error"], atomic.data["error"]);
    assert!(steps(&scripted)[0]["error"]["message"]
        .as_str()
        .unwrap()
        .contains(message));
    assert_eq!(operation_list(&scripted), ["windows"]);
    assert_eq!(backend.0.windows.load(Ordering::SeqCst), 2);
    let duplicate = start(
        &owner,
        submit(&owner, "script-inventory", None, script(source)),
    );
    assert_eq!(duplicate.data, scripted.data);
    assert_eq!(backend.0.windows.load(Ordering::SeqCst), 2);
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 0);
    let state = run_script(&owner, "read-state", "return state;");
    assert_eq!(state.data["value"]["result"], json!({}));
}

#[test]
fn omitted_await_does_not_hide_failed_action_or_commit_state() {
    let (owner, backend) = coordinator();
    backend.0.model.lock().unwrap().outcome = ActionOutcome::rejected(Error::NativeFailure {
        code: "permission_denied".into(),
        message: "controlled native condition".into(),
    });
    let source=format!("{SETUP} app.press({{window_ref,observation_ref:observation.observation_ref,element_ref:observation.elements[0].element_ref}}); state.falseSuccess=true; return 123;");
    let failed = run_script(&owner, "unawaited", source);
    assert!(failed.is_error, "{}", failed.data);
    assert_eq!(
        failed.data["script"]["failure"]["code"],
        "permission_denied"
    );
    assert_eq!(failed.data["script"]["failure"]["operation"], "press");
    assert_eq!(failed.data["script"]["state_committed"], false);
    assert_eq!(
        steps(&failed).last().unwrap()["receipt"]["outcome"]["dispatch"],
        "not_dispatched"
    );
    assert_eq!(result(&owner, "unawaited").data, failed.data);
    let state = run_script(&owner, "state-read", "return Object.keys(state)");
    assert_eq!(state.data["value"]["result"], json!([]));
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 1);
}
