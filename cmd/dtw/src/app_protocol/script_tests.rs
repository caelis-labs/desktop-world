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

#[test]
fn capture_cleanup_failure_cannot_be_caught_to_commit_state_or_change_prior_results() {
    let (owner, backend) = coordinator();
    let seed = "state.kept=7;return 'before cleanup';";
    let prior = run_script(&owner, "prior", seed);
    {
        let mut model = backend.0.model.lock().unwrap();
        model.cleanup_after_capture = Some(Error::NativeFailure {
            code: "scene_cleanup_failed".into(),
            message: "original scene cleanup failure".into(),
        });
        model.shutdown_error = Some(Error::BackendFailed("later close failure".into()));
    }
    let completed = run_script(
        &owner,
        "failed-capture",
        r#"
const window_ref=(await app.windows()).windows[0].window_ref;
try { await app.capture({window_ref}); } catch (_) {}
state.invalid=true;
await app.windows();
return 'pretend success';
"#,
    );
    assert!(completed.is_error, "{}", completed.data);
    assert_eq!(
        completed.data["shutdown_error"]["code"],
        "scene_cleanup_failed"
    );
    assert_eq!(
        completed.data["script"]["shutdown_error"],
        completed.data["shutdown_error"]
    );
    assert_eq!(completed.data["script"]["state_committed"], false);
    assert_eq!(
        *owner.state.lock().unwrap().script_state.lock().unwrap(),
        json!({"kept":7})
    );
    assert_eq!(operation_list(&completed), ["windows", "capture"]);
    assert_eq!(steps(&completed)[1]["error"], Value::Null);
    assert_eq!(backend.0.windows.load(Ordering::SeqCst), 1);
    assert_eq!(backend.0.captures.load(Ordering::SeqCst), 1);
    assert_code(
        &start(&owner, submit(&owner, "blocked", None, script("return 1;"))),
        "closed",
    );
    owner.close();
    assert_eq!(result(&owner, "failed-capture").data, completed.data);
    assert_eq!(result(&owner, "prior").data, prior.data);
    assert_eq!(
        start(&owner, submit(&owner, "prior", None, script(seed))).data,
        prior.data
    );
}

#[test]
fn click_cleanup_failure_keeps_the_original_native_outcome_and_step_receipt() {
    for provider in [
        ActionOutcome {
            delivery: None,
            dispatch: DispatchState::Dispatched,
            evidence: EffectEvidence::Unverified,
            error: None,
        },
        ActionOutcome::unknown(Error::NativeFailure {
            code: "native_click_failed".into(),
            message: "original native click failure".into(),
        }),
    ] {
        let (owner, backend) = coordinator();
        {
            let mut model = backend.0.model.lock().unwrap();
            model.outcome = provider.clone();
            model.cleanup_after_action = Some(Error::NativeFailure {
                code: "scene_cleanup_failed".into(),
                message: "original scene cleanup failure".into(),
            });
            model.shutdown_error = Some(Error::BackendFailed("later close failure".into()));
        }
        let completed = run_script(
            &owner,
            "click-cleanup",
            r#"
const window_ref=(await app.windows()).windows[0].window_ref;
const o=await app.capture({window_ref});
try { await app.click({window_ref,observation_ref:o.observation_ref,x:1,y:1}); } catch (_) {}
state.invalid=true;return 'pretend success';
"#,
        );
        assert!(completed.is_error, "{}", completed.data);
        assert_eq!(
            completed.data["shutdown_error"]["code"],
            "scene_cleanup_failed"
        );
        assert_eq!(completed.data["script"]["state_committed"], false);
        assert_eq!(operation_list(&completed), ["windows", "capture", "click"]);
        assert_eq!(
            steps(&completed)[2]["receipt"],
            wire::receipt(&ActionReceipt {
                outcome: provider.clone(),
                provider_outcome: Some(provider),
                interruption: None,
            })
        );
        assert_eq!(backend.0.actions.load(Ordering::SeqCst), 1);
        assert_eq!(result(&owner, "click-cleanup").data, completed.data);
    }
}

#[test]
fn inline_focus_cleanup_failure_preserves_action_and_stops_before_another_input() {
    for mut provider in [
        ActionOutcome {
            delivery: None,
            dispatch: DispatchState::Dispatched,
            evidence: EffectEvidence::Unverified,
            error: None,
        },
        ActionOutcome::unknown(Error::NativeFailure {
            code: "native_keypress_failed".into(),
            message: "original uncertain keypress".into(),
        }),
    ] {
        let (owner, backend) = coordinator();
        let prior = run_script(&owner, "prior", "state.kept=7;return 'prior result';");
        provider.delivery = Some(InputDelivery {
            route: Some(InputRoute::Foreground),
            reason: Some("menu_key_equivalent".into()),
            foreground_borrowed: true,
            focus_return: Some(FocusReturn::Unconfirmed),
        });
        {
            let mut model = backend.0.model.lock().unwrap();
            model.outcome = provider.clone();
            let cleanup = Error::NativeFailure {
                code: "deadline_exceeded".into(),
                message: "original focus return deadline".into(),
            };
            model.cleanup_after_action = Some(cleanup.clone());
            model.finish_error = Some(cleanup);
            model.shutdown_error = Some(Error::BackendFailed("later close failure".into()));
        }
        let completed = run_script(
            &owner,
            "inline-focus-cleanup",
            r#"
const window_ref=(await app.windows()).windows[0].window_ref;
state.kept=9;
try { await app.keypress({window_ref,keys:['META','SHIFT','S']}); } catch (_) {}
await app.keypress({window_ref,keys:['ENTER']});
return 'pretend success';
"#,
        );
        assert!(completed.is_error, "{}", completed.data);
        assert_eq!(
            completed.data["shutdown_error"]["code"],
            "deadline_exceeded"
        );
        assert_eq!(
            completed.data["shutdown_error"]["message"],
            "original focus return deadline"
        );
        assert_eq!(completed.data["script"]["focus_return"], "unconfirmed");
        assert_eq!(completed.data["script"]["state_committed"], false);
        assert_eq!(
            *owner.state.lock().unwrap().script_state.lock().unwrap(),
            json!({"kept":7})
        );
        assert_eq!(operation_list(&completed), ["windows", "keypress"]);
        assert_eq!(
            steps(&completed)[1]["receipt"],
            wire::receipt(&ActionReceipt {
                outcome: provider.clone(),
                provider_outcome: Some(provider),
                interruption: None,
            })
        );
        assert_eq!(backend.0.actions.load(Ordering::SeqCst), 1);
        assert_eq!(backend.0.windows.load(Ordering::SeqCst), 1);
        assert_code(
            &start(&owner, submit(&owner, "blocked", None, script("return 1;"))),
            "closed",
        );
        assert_eq!(result(&owner, "inline-focus-cleanup").data, completed.data);
        assert_eq!(result(&owner, "prior").data, prior.data);
    }
}

#[test]
fn atomic_cleanup_failure_is_separate_from_a_successful_native_read() {
    let (owner, backend) = coordinator();
    let window = windows(&owner, "windows").remove(0);
    backend.0.model.lock().unwrap().cleanup_after_capture = Some(Error::NativeFailure {
        code: "scene_cleanup_failed".into(),
        message: "original scene cleanup failure".into(),
    });
    let completed = run(&owner, "atomic-capture", Some(&window), Command::Capture);
    assert!(completed.is_error, "{}", completed.data);
    assert_eq!(
        completed.data["shutdown_error"]["code"],
        "scene_cleanup_failed"
    );
    assert_eq!(completed.data["error"], Value::Null);
    assert!(completed.image.is_some());
    assert_eq!(completed.data["value"]["kind"], "pixels");
    assert_eq!(result(&owner, "atomic-capture").data, completed.data);
}

#[test]
fn receipt_byte_budget_stops_before_another_dispatch_without_losing_prior_steps() {
    let (owner, backend) = coordinator();
    let completed = run(&owner, "receipt-resource", None, Command::Script {
        source: "for(let i=0;i<10000;i++) { try { await app.windows(); } catch(e) { break; } } state.invalid=true; return 'pretend success';".into(),
        timeout_ms: 30_000,
        max_calls: scripts::UNLIMITED_CALLS,
    });
    assert!(completed.is_error, "{}", completed.data);
    assert_eq!(completed.data["script"]["failure"]["code"], "receipt_limit");
    assert_eq!(completed.data["script"]["state_committed"], false);
    let recorded = steps(&completed);
    assert!(recorded.len() > 32);
    assert_eq!(recorded.len(), backend.0.windows.load(Ordering::SeqCst));
    assert!(recorded.iter().all(|step| step["status"] == "completed"));
    let bytes: usize = recorded.iter().map(|step| step.to_string().len()).sum();
    assert!(bytes >= scripts::MAX_STEP_RECEIPT_BYTES);
    assert!(bytes - recorded.last().unwrap().to_string().len() < scripts::MAX_STEP_RECEIPT_BYTES);
    assert_eq!(
        *owner.state.lock().unwrap().script_state.lock().unwrap(),
        json!({})
    );
}

#[test]
fn final_action_over_receipt_threshold_keeps_unknown_provider_evidence() {
    let (owner, backend) = coordinator();
    let native_error = Error::BackendFailed("controlled unknown effect; ".repeat(24 * 1024));
    backend.0.model.lock().unwrap().outcome = ActionOutcome::unknown(native_error.clone());
    let source = format!("{SETUP} try {{ {PRESS} }} catch(e) {{}} try {{ await app.windows(); }} catch(e) {{}} state.invalid=true;return 'pretend success';");
    let completed = run(
        &owner,
        "action-receipt-resource",
        None,
        Command::Script {
            source,
            timeout_ms: 30_000,
            max_calls: scripts::UNLIMITED_CALLS,
        },
    );
    assert!(completed.is_error);
    assert_eq!(completed.data["script"]["failure"]["code"], "receipt_limit");
    assert_eq!(completed.data["script"]["state_committed"], false);
    assert_eq!(operation_list(&completed), ["windows", "observe", "press"]);
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(backend.0.windows.load(Ordering::SeqCst), 1);
    let receipt = &steps(&completed)[2]["receipt"];
    assert_eq!(receipt["outcome"]["dispatch"], "unknown");
    assert_eq!(receipt["provider_outcome"], receipt["outcome"]);
    assert_eq!(
        receipt["outcome"]["error"],
        error_json(&wire::core_error(&native_error))
    );
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
            delivery: None,
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
        assert_eq!(
            *owner.state.lock().unwrap().script_state.lock().unwrap(),
            json!({})
        );
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
        assert_eq!(*state.script_state.lock().unwrap(), json!({}));
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
        assert_eq!(
            *owner.state.lock().unwrap().script_state.lock().unwrap(),
            json!({})
        );
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
    Arc::get_mut(&mut owner).unwrap().retention.payload_bytes = 512;
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

#[test]
fn execution_finish_runs_on_success_and_script_failure_without_retiring_the_owner() {
    let (owner, backend) = coordinator();
    let first = run_script(&owner, "success", "state.kept=7;return state.kept;");
    assert!(!first.is_error, "{}", first.data);
    assert_eq!(backend.0.finishes.load(Ordering::SeqCst), 1);
    let failed = run_script(&owner, "failure", "state.kept=9;throw new Error('test');");
    assert!(failed.is_error, "{}", failed.data);
    assert_eq!(backend.0.finishes.load(Ordering::SeqCst), 2);
    let next = run_script(&owner, "next", "return state.kept;");
    assert_eq!(next.data["value"]["result"], 7);
    assert_eq!(backend.0.finishes.load(Ordering::SeqCst), 3);
    assert_eq!(backend.0.shutdowns.load(Ordering::SeqCst), 0);
    assert_eq!(result(&owner, "success").data, first.data);
    assert_eq!(backend.0.finishes.load(Ordering::SeqCst), 3);
    owner.close();
}

#[test]
fn execution_finish_failure_preserves_action_receipt_and_prevents_state_commit() {
    let (owner, backend) = coordinator();
    run_script(&owner, "seed", "state.kept=7;");
    backend.0.model.lock().unwrap().finish_error = Some(Error::NativeFailure {
        code: "finish_failed".into(),
        message: "original execution cleanup failure".into(),
    });
    let completed = run_script(
        &owner,
        "finish-error",
        format!("{SETUP}{PRESS}state.kept=9;return true;"),
    );
    assert!(completed.is_error, "{}", completed.data);
    assert_eq!(completed.data["shutdown_error"]["code"], "finish_failed");
    assert_eq!(completed.data["script"]["focus_return"], "unconfirmed");
    assert_eq!(completed.data["script"]["state_committed"], false);
    assert_eq!(
        *owner.state.lock().unwrap().script_state.lock().unwrap(),
        json!({"kept":7})
    );
    assert_eq!(operation_list(&completed), ["windows", "observe", "press"]);
    assert_eq!(
        steps(&completed)[2]["receipt"]["outcome"]["dispatch"],
        "dispatched"
    );
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 1);
    assert_code(
        &start(&owner, submit(&owner, "later", None, script("return 1;"))),
        "closed",
    );
    assert_eq!(result(&owner, "finish-error").data, completed.data);
    owner.close();
}

#[test]
fn final_focus_return_is_separate_from_the_original_focus_receipt() {
    let (owner, backend) = coordinator();
    {
        let mut model = backend.0.model.lock().unwrap();
        model.outcome.delivery = Some(InputDelivery {
            route: Some(InputRoute::Foreground),
            reason: None,
            foreground_borrowed: true,
            focus_return: Some(FocusReturn::Pending),
        });
        model.finish_focus = Some(FocusReturn::Restored);
    }
    let completed = run_script(&owner, "focus-return", "const w=(await app.windows()).windows[0].window_ref;return await app.focus({window_ref:w});");
    assert!(!completed.is_error, "{}", completed.data);
    assert_eq!(completed.data["script"]["focus_return"], "restored");
    assert_eq!(
        steps(&completed)[1]["receipt"]["outcome"]["delivery"]["focus_return"],
        "pending"
    );
    assert_eq!(result(&owner, "focus-return").data, completed.data);
    backend.0.model.lock().unwrap().finish_focus = None;
    let next = run_script(&owner, "no-focus-return", "return 1;");
    assert!(next.data["script"].get("focus_return").is_none());
    owner.close();
}

#[test]
fn retired_unconfirmed_focus_keeps_receipts_and_allows_fresh_reads() {
    for uncertain in [false, true] {
        let (owner, backend) = coordinator();
        run_script(&owner, "seed", "state.kept=7;");
        {
            let mut model = backend.0.model.lock().unwrap();
            if uncertain {
                model.outcome = ActionOutcome::unknown(Error::NativeFailure {
                    code: "ax_unavailable".into(),
                    message: "original focus preparation failed".into(),
                });
            }
            model.finish_focus = Some(FocusReturn::Unconfirmed);
        }
        let original = run_script(
            &owner,
            "retired-focus",
            format!("{SETUP}{PRESS}state.kept=9;return true;"),
        );
        assert_eq!(original.is_error, uncertain, "{}", original.data);
        assert_eq!(original.data["script"]["focus_return"], "unconfirmed");
        assert!(original.data["shutdown_error"].is_null());
        assert_eq!(
            steps(&original)[2]["receipt"]["outcome"]["dispatch"],
            if uncertain { "unknown" } else { "dispatched" }
        );
        backend.0.model.lock().unwrap().finish_focus = None;
        let fresh = run_script(
            &owner,
            "fresh-read",
            "return {windows:await app.windows(),kept:state.kept};",
        );
        assert!(!fresh.is_error, "{}", fresh.data);
        assert_eq!(
            fresh.data["value"]["result"]["kept"],
            if uncertain { 7 } else { 9 }
        );
        assert_eq!(backend.0.actions.load(Ordering::SeqCst), 1);
        assert_eq!(backend.0.shutdowns.load(Ordering::SeqCst), 0);
        assert_eq!(result(&owner, "retired-focus").data, original.data);
        owner.close();
    }
}

#[test]
fn script_scroll_resolves_bound_refs_once_and_keeps_other_windows() {
    let (owner, backend) = coordinator();
    let reply = run_script(
        &owner,
        "scroll-anchors",
        r#"
const [a,b]=(await app.windows()).windows;
const first=await app.observe({window_ref:a.window_ref});
const neighbor=await app.observe({window_ref:b.window_ref});
const semantic={window_ref:a.window_ref,observation_ref:first.observation_ref,element_ref:first.elements[0].element_ref,delta_y:8};
await app.scroll(semantic);
let stale;
try { await app.scroll(semantic); } catch(e) { stale=e.code; }
await app.scroll({window_ref:b.window_ref,observation_ref:neighbor.observation_ref,element_ref:neighbor.elements[0].element_ref,delta_x:-3});
const image=await app.capture({window_ref:a.window_ref});
let malformed;
try { await app.scroll({window_ref:a.window_ref,observation_ref:image.observation_ref,x:2.5,delta_y:8}); } catch(e) { malformed=e.code; }
await app.scroll({window_ref:a.window_ref,observation_ref:image.observation_ref,x:2.5,y:3,delta_y:8});
return {stale,malformed};
"#,
    );
    assert!(!reply.is_error, "{}", reply.data);
    assert_eq!(reply.data["value"]["result"]["stale"], "stale_observation");
    assert_eq!(reply.data["value"]["result"]["malformed"], "invalid_args");
    let calls = backend.0.anchored_scrolls.lock().unwrap();
    assert_eq!(calls.len(), 3);
    assert_eq!(
        calls
            .iter()
            .map(|(target, _)| target.native_id)
            .collect::<Vec<_>>(),
        vec![101, 202, 101]
    );
    assert_eq!(
        calls[0].1,
        ScrollRequest {
            anchor: ScrollAnchor::Element(ElementId(9007199254740993)),
            delta_x: 0,
            delta_y: 8
        }
    );
    assert_eq!(calls[1].1.delta_x, -3);
    assert_eq!(
        calls[2].1,
        ScrollRequest {
            anchor: ScrollAnchor::Pixels(PixelPoint { x: 2.5, y: 3.0 }),
            delta_x: 0,
            delta_y: 8
        }
    );
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 3);
}
