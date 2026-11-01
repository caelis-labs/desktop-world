//! Condition changes are controlled page sequences, never GUI or input retries.
use super::*;
use wire::ElementWaitState;

fn waited(state: ElementWaitState, timeout_ms: u64) -> Command {
    Command::Wait {
        selector: wire::ElementSelector {
            scope: wire::SelectorScope::Window,
            role: Some("button".into()),
            text: Some("Save".into()),
        },
        state,
        timeout_ms,
    }
}

#[test]
fn wait_batches_action_delayed_condition_and_readback_without_replaying_effect() {
    let (owner, backend) = setup(vec![
        Ok(page(vec![element(1, true)], false, true, 1)),
        Ok(page(vec![], false, true, 0)),
        Ok(page(vec![element(2, true)], true, false, 1)),
        Ok(page(vec![], false, true, 2)),
    ]);
    let reply = script(
        &owner,
        "batch",
        "
        const receipt = await app.press({window_ref,selector:{text:'Save'}});
        const ready = await app.wait({window_ref,selector:{text:'Save'},state:'present'});
        return {receipt, ready};
    ",
    );
    assert!(!reply.is_error, "{}", reply.data);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(backend.focus.0.actions.load(Ordering::SeqCst), 0);
    assert_eq!(backend.queries.lock().unwrap().len(), 3);
    assert_eq!(backend.advances.load(Ordering::SeqCst), 1);
    let result = &reply.data["value"]["result"];
    assert_eq!(result["ready"]["state"], "present");
    assert_eq!(result["ready"]["element"]["value"], "summary");
    assert_eq!(
        result["receipt"]["outcome"]["evidence"]["kind"],
        "unverified"
    );
    let steps = reply.data["script"]["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 3);
    assert_eq!(steps[2]["operation"], "wait");
    assert!(steps[2]["receipt"].is_null());
    assert_eq!(steps[2]["resolution"]["attempts"], 2);
    assert_eq!(steps[2]["resolution"]["state"], "present");
}

#[test]
fn wait_enabled_requires_explicit_state_then_returns_usable_fresh_refs() {
    let mut unknown = element(1, true);
    unknown.enabled = None;
    let mut disabled = element(2, true);
    disabled.enabled = Some(false);
    let (owner, backend) = setup(vec![
        Ok(page(vec![unknown], false, true, 1)),
        Ok(page(vec![disabled], false, true, 1)),
        Ok(page(vec![element(3, true)], false, true, 1)),
    ]);
    let reply = script(&owner, "enabled", "
        const ready = await app.wait({window_ref,selector:{text:'Save'},state:'enabled'});
        await app.press({window_ref,observation_ref:ready.observation_ref,element_ref:ready.element_ref});
        return ready;
    ");
    assert!(!reply.is_error, "{}", reply.data);
    assert_eq!(backend.queries.lock().unwrap().len(), 3);
    assert_eq!(*backend.selected_elements.lock().unwrap(), [ElementId(3)]);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(reply.data["value"]["result"]["state"], "enabled");
    assert_eq!(reply.data["value"]["result"]["element"]["enabled"], true);
}

#[test]
fn wait_disabled_does_not_require_action_support_or_focus() {
    let mut disabled = element(2, false);
    disabled.enabled = Some(false);
    let (owner, backend) = setup(vec![Ok(page(vec![disabled], false, true, 1))]);
    let reply = script(
        &owner,
        "disabled",
        "return await app.wait({window_ref,selector:{text:'Save'},state:'disabled',timeout_ms:0});",
    );
    assert!(!reply.is_error, "{}", reply.data);
    assert_eq!(reply.data["value"]["result"]["element"]["enabled"], false);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
    assert_eq!(backend.focus.0.actions.load(Ordering::SeqCst), 0);
}

#[test]
fn wait_absent_retries_matches_and_partial_empty_but_needs_complete_zero() {
    let (owner, backend) = setup(vec![
        Ok(page(
            vec![element(1, true), element(2, true)],
            true,
            false,
            2,
        )),
        Ok(page(vec![], false, false, 3)),
        Ok(page(vec![], true, false, 4)),
        Ok(page(vec![], false, true, 5)),
    ]);
    let reply = script(
        &owner,
        "absent",
        "return await app.wait({window_ref,selector:{text:'Save'},state:'absent'});",
    );
    assert!(!reply.is_error, "{}", reply.data);
    let result = &reply.data["value"]["result"];
    assert_eq!(result["state"], "absent");
    assert_eq!(result.as_object().unwrap().len(), 2);
    assert_eq!(backend.queries.lock().unwrap().len(), 3);
    assert_eq!(backend.advances.load(Ordering::SeqCst), 1);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
    assert_eq!(backend.focus.0.actions.load(Ordering::SeqCst), 0);
}

#[test]
fn wait_never_filters_duplicates_by_requested_enabled_state() {
    for state in ["present", "enabled", "disabled"] {
        let mut disabled = element(2, false);
        disabled.enabled = Some(false);
        let (owner, backend) = setup(vec![Ok(page(
            vec![element(1, true), disabled],
            false,
            true,
            2,
        ))]);
        let reply = script(
            &owner,
            "ambiguous",
            &format!("await app.wait({{window_ref,selector:{{text:'Save'}},state:'{state}'}});"),
        );
        assert_eq!(step(&reply)["error"]["code"], "selector_ambiguous");
        assert!(step(&reply)["receipt"].is_null());
        assert_eq!(backend.queries.lock().unwrap().len(), 1);
        assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn wait_zero_is_one_traversal_and_does_not_borrow_focus_for_content_gaps() {
    let mut unknown = element(1, false);
    unknown.enabled = None;
    for (state, sample) in [
        ("present", page(vec![], false, true, 0)),
        ("absent", content_unproven(vec![])),
        ("enabled", page(vec![element(1, true)], false, false, 1)),
        ("disabled", page(vec![unknown], false, true, 1)),
    ] {
        let (owner, backend) = setup(vec![Ok(sample)]);
        let reply = script(&owner, "gap", &format!("await app.wait({{window_ref,selector:{{text:'Save'}},state:'{state}',timeout_ms:0}});"));
        assert_eq!(step(&reply)["error"]["code"], "wait_timeout");
        assert_eq!(step(&reply)["resolution"]["status"], "timeout");
        assert_eq!(backend.queries.lock().unwrap().len(), 1);
        assert_eq!(backend.focus.0.actions.load(Ordering::SeqCst), 0);
        assert_eq!(backend.base.0.shutdowns.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn wait_spent_poll_window_does_not_start_another_traversal() {
    let (owner, backend) = setup(vec![
        Ok(page(vec![], false, true, 0)),
        Ok(page(vec![element(1, true)], false, true, 1)),
    ]);
    let work = select_after_poll_window(&owner, waited(ElementWaitState::Present, 2000), 2000);
    assert_eq!(work.completed.error.unwrap().code, "wait_timeout");
    assert_eq!(backend.queries.lock().unwrap().len(), 1);
    assert_eq!(backend.pages.lock().unwrap().len(), 1);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
}

#[test]
fn wait_condition_timeout_can_be_caught_without_replaying_prior_action() {
    let (owner, backend) = setup(vec![
        Ok(page(vec![element(1, true)], false, true, 1)),
        Ok(page(vec![], false, true, 0)),
    ]);
    let reply = script(
        &owner,
        "timeout",
        "
        await app.press({window_ref,selector:{text:'Save'}});
        try { await app.wait({window_ref,selector:{text:'Saved'},state:'present',timeout_ms:0}); }
        catch (error) { state.condition = error.code; }
        return state.condition;
    ",
    );
    assert!(!reply.is_error, "{}", reply.data);
    assert_eq!(reply.data["value"]["result"], "wait_timeout");
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(backend.base.0.shutdowns.load(Ordering::SeqCst), 0);
    assert!(!script(&owner, "still-live", "return 1;").is_error);
}

#[test]
fn wait_cancellation_during_read_keeps_prior_receipt_and_stops_without_more_reads() {
    let (owner, backend) = setup(vec![
        Ok(page(vec![element(1, true)], false, true, 1)),
        Ok(page(vec![], false, true, 0)),
    ]);
    let action_gate = backend.base.action_gate();
    let started = start(&owner, submit(&owner, "cancel-wait", None, Command::Script {
        source: "const window_ref=(await app.windows()).windows[0].window_ref; await app.press({window_ref,selector:{text:'Save'}}); try { await app.wait({window_ref,selector:{text:'Saved'},state:'present'}); } catch (_) {} return 1;".into(),
        timeout_ms: 5000, max_calls: 16,
    }));
    assert!(!started.is_error);
    action_gate.entered();
    let read_gate = backend.base.read_gate();
    action_gate.release();
    read_gate.entered();
    owner.handle(Request::Cancel {
        request_id: "cancel-wait".into(),
    });
    read_gate.release();
    let reply = finish(&owner, "cancel-wait");
    assert!(reply.is_error, "{}", reply.data);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(backend.queries.lock().unwrap().len(), 2);
    assert_eq!(
        reply.data["script"]["steps"][1]["receipt"]["outcome"]["dispatch"],
        "dispatched"
    );
    assert_eq!(
        reply.data["script"]["steps"][1]["receipt"]["outcome"]["evidence"]["kind"],
        "unverified"
    );
    assert_eq!(
        reply.data["script"]["steps"][2]["resolution"]["status"],
        "read_failed"
    );
    assert!(reply.data["script"]["steps"][2]["receipt"].is_null());
    assert_eq!(reply.data["script"]["state_committed"], false);
}

#[test]
fn wait_expired_execution_deadline_prevents_first_native_read() {
    let (owner, backend) = setup(vec![]);
    let window = windows(&owner, "inventory").remove(0);
    let command = waited(ElementWaitState::Present, 30_000);
    let submitted = submit(&owner, "expired", Some(&window), command);
    let work = owner.select_element_until(
        &submitted,
        &wire::ElementSelector {
            scope: wire::SelectorScope::Window,
            role: None,
            text: Some("Save".into()),
        },
        30_000,
        Instant::now() - Duration::from_millis(1),
        Instant::now() + Duration::from_secs(30),
    );
    assert_eq!(work.completed.error.unwrap().code, "timed_out");
    assert!(backend.queries.lock().unwrap().is_empty());
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
}

#[test]
fn wait_replaces_only_selected_windows_semantic_refs_and_keeps_capture_and_sibling() {
    let (owner, backend) = setup(vec![
        Ok(page(vec![element(1, true)], false, true, 1)),
        Ok(page(vec![element(2, true)], false, true, 1)),
        Ok(page(vec![element(3, true)], false, true, 1)),
    ]);
    let reply = script(
        &owner,
        "refs",
        "
        const sibling = (await app.windows()).windows[1].window_ref;
        const old = await app.find({window_ref,selector:{text:'Save'}});
        const other = await app.find({window_ref:sibling,selector:{text:'Save'}});
        const pixels = await app.capture({window_ref});
        const ready = await app.wait({window_ref,selector:{text:'Save'},state:'present'});
        return {old,other,pixels,ready};
    ",
    );
    assert!(!reply.is_error, "{}", reply.data);
    let result = &reply.data["value"]["result"];
    let bound = |key: &str| {
        owner.bound_element(
            result[key]["observation_ref"].as_str().unwrap(),
            result[key]["element_ref"].as_str().unwrap(),
        )
    };
    // A replacement query removes the previous public mapping, just as an
    // explicit observe does. It cannot be revived through the saved strings.
    assert!(matches!(bound("old"), Err(Error::InvalidObservation)));
    assert!(bound("other").is_ok());
    assert!(bound("ready").is_ok());
    assert_ne!(
        result["old"]["observation_ref"],
        result["ready"]["observation_ref"]
    );
    let pixels = result["pixels"]["observation_ref"].as_str().unwrap();
    let core = owner
        .state
        .lock()
        .unwrap()
        .observations
        .iter()
        .find(|entry| entry.public == pixels)
        .unwrap()
        .core;
    assert!(owner.runtime.retains_observation(core));
    let source = format!("const refs={result};
        await app.press({{window_ref:refs.other.window_ref,observation_ref:refs.other.observation_ref,element_ref:refs.other.element_ref}});
        await app.click({{window_ref:refs.pixels.window_ref,observation_ref:refs.pixels.observation_ref,x:0,y:0}});");
    let used = script(&owner, "surviving-refs", &source);
    assert!(!used.is_error, "{}", used.data);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 2);
}

#[test]
fn wait_retains_original_provider_error_without_retry_or_input() {
    let (owner, backend) = setup(vec![Err(Error::NativeFailure {
        code: "provider_read_failed".into(),
        message: "original diagnostic".into(),
    })]);
    let reply = script(
        &owner,
        "provider-error",
        "await app.wait({window_ref,selector:{text:'Save'},state:'absent'});",
    );
    assert_eq!(step(&reply)["error"]["code"], "provider_read_failed");
    assert_eq!(step(&reply)["error"]["message"], "original diagnostic");
    assert_eq!(backend.queries.lock().unwrap().len(), 1);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
}

#[test]
fn wait_read_crossing_execution_deadline_cannot_be_caught_into_more_calls_or_commit() {
    let (owner, backend) = setup(vec![Ok(page(vec![element(1, true)], false, true, 1))]);
    let gate = backend.base.read_gate();
    let started = start(&owner, submit(&owner, "deadline-read", None, Command::Script {
        source: "const window_ref=(await app.windows()).windows[0].window_ref; try { await app.wait({window_ref,selector:{text:'Save'},state:'present',timeout_ms:30000}); } catch (_) { state.recovered=true; } await app.capture({window_ref}); state.committed=true; return 1;".into(),
        timeout_ms: 2000, max_calls: 16,
    }));
    assert!(!started.is_error);
    gate.entered();
    let deadline = owner
        .state
        .lock()
        .unwrap()
        .jobs
        .iter()
        .find(|job| job.submitted.request_id == "deadline-read")
        .unwrap()
        .script
        .as_ref()
        .unwrap()
        .deadline;
    // Hold a known in-flight read through its actual accepted deadline. This
    // waits on that deadline, not an assumed scheduler/GUI settling delay.
    let (_keep_open, signal) = mpsc::channel::<()>();
    assert!(matches!(
        signal.recv_timeout(deadline.saturating_duration_since(Instant::now())),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    assert!(Instant::now() >= deadline);
    gate.release();
    let reply = finish(&owner, "deadline-read");
    assert!(reply.is_error, "{}", reply.data);
    assert_eq!(reply.data["error"]["code"], "script_timeout");
    assert_eq!(reply.data["script"]["state_committed"], false);
    assert_eq!(backend.queries.lock().unwrap().len(), 1);
    assert_eq!(backend.base.0.captures.load(Ordering::SeqCst), 0);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
    assert_eq!(step(&reply)["resolution"]["status"], "read_failed");
    assert_eq!(step(&reply)["error"]["code"], "timed_out");
}

#[test]
fn wait_wire_requires_one_bounded_condition_and_no_sleep_or_reference_arguments() {
    for state in ["present", "absent", "enabled", "disabled"] {
        let parsed = wire::script_submit(
            "a",
            "r",
            "wait",
            &json!({"window_ref":"w","selector":{"text":"Save"},"state":state}),
        )
        .unwrap();
        assert!(matches!(
            parsed.command,
            Command::Wait {
                timeout_ms: 10_000,
                ..
            }
        ));
    }
    for args in [
        json!({"window_ref":"w","selector":{"text":"Save"}}),
        json!({"window_ref":"w","state":"present"}),
        json!({"duration_ms":100}),
        json!({"window_ref":"w","selector":{"text":"Save"},"state":"visible"}),
        json!({"window_ref":"w","selector":{"text":"Save"},"state":"present","timeout_ms":30001}),
        json!({"window_ref":"w","selector":{"text":"Save"},"state":"present","timeout_ms":-1}),
        json!({"window_ref":"w","selector":{"text":"Save"},"state":"present","observation_ref":"old"}),
    ] {
        assert!(
            wire::script_submit("a", "r", "wait", &args).is_err(),
            "{args}"
        );
    }
}
