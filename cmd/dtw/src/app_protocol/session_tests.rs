use super::*;

fn session_exec(server: &Arc<Server<Catalog>>, source: &str) -> Reply {
    call(server, "exec", json!({"source":source}))
}

#[test]
fn repeated_default_receipt_reads_keep_the_original_target_and_mixed_work_stays_visible() {
    let (server, _) = server();
    let original = session_exec(&server, "return 11;");
    for _ in 0..3 {
        let query = session_exec(&server, "return (await session.result()).execution_id;");
        assert_eq!(success(&query), &original.data["execution_id"]);
    }
    let native = exec(&server, "native", "await app.windows();");
    for _ in 0..3 {
        let query = exec(
            &server,
            &uuid::Uuid::new_v4().to_string(),
            "return (await session.result()).execution_id;",
        );
        assert_eq!(success(&query), &native.data["execution_id"]);
    }
    let mixed = exec(
        &server,
        "mixed",
        "await session.result();await app.windows();",
    );
    success(&mixed);
    let next = session_exec(&server, "return (await session.result()).execution_id;");
    assert_eq!(success(&next), &mixed.data["execution_id"]);
    // Query executions themselves are still explicitly addressable.
    let explicit = session_exec(
        &server,
        &format!(
            "return (await session.result({})).execution_id;",
            next.data["execution_id"]
        ),
    );
    assert_eq!(success(&explicit), &next.data["execution_id"]);
    server.close().unwrap();
}

#[test]
fn print_and_session_json_persist_without_starting_any_native_owner() {
    let (server, catalog) = server();
    let first=session_exec(&server,"session.state.count=7;globalThis.temporary=3;print('ready', {count:state.count});return session.id;");
    let id = success(&first).clone();
    assert!(id.as_str().unwrap().starts_with("session_"));
    assert_eq!(
        first.data["value"]["console"],
        json!(["ready {\"count\":7}"])
    );
    let next=session_exec(&server,"print(session.state.count);return {count:state.count,temporary:typeof temporary,id:session.id,app:session.app};");
    assert_eq!(
        success(&next),
        &json!({"count":7,"temporary":"undefined","id":id,"app":null})
    );
    assert_eq!(next.data["app"], Value::Null);
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 0);
    assert_eq!(catalog.backend.0.windows.load(Ordering::SeqCst), 0);
    let rejected = session_exec(
        &server,
        "session.state.count=99;print('before failure');await app.windows();",
    );
    assert!(rejected.is_error);
    assert_eq!(rejected.data["script"]["failure"]["code"], "app_required");
    assert_eq!(rejected.data["value"]["console"], json!(["before failure"]));
    assert_eq!(success(&session_exec(&server, "return state.count;")), 7);
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 0);
    server.close().unwrap();
}

#[test]
fn session_receipts_are_paged_originals_and_survive_app_disappearance() {
    let (server, catalog) = server();
    let original = exec(
        &server,
        "original",
        "for(let i=0;i<25;i++) await app.windows();return 'kept';",
    );
    success(&original);
    let original_full = receipt(&server, &original);
    let source = format!(
        "return await session.result({{execution_id:{},limit:10}});",
        original.data["execution_id"]
    );
    catalog.entries.lock().unwrap().clear();
    let page = session_exec(&server, &source);
    let page = success(&page);
    assert_eq!(page["value"]["result"], "kept");
    assert_eq!(page["script"]["total_steps"], 25);
    assert_eq!(page["script"]["steps"].as_array().unwrap().len(), 10);
    assert_eq!(page["script"]["next_offset"], 10);
    let last = session_exec(
        &server,
        &format!(
            "return await session.result({{execution_id:{},offset:20,limit:10}});",
            original.data["execution_id"]
        ),
    );
    assert_eq!(
        success(&last)["script"]["steps"].as_array().unwrap().len(),
        5
    );
    assert!(success(&last)["script"]["next_offset"].is_null());
    assert_eq!(catalog.backend.0.windows.load(Ordering::SeqCst), 25);
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 1);
    assert_eq!(receipt(&server, &original).data, original_full.data);
    let (foreign, _) = super::server();
    let missing = session_exec(&foreign, &source);
    assert_eq!(
        missing.data["script"]["failure"]["code"],
        "execution_unavailable"
    );
    foreign.close().unwrap();
    server.close().unwrap();
}

#[test]
fn session_receipt_preserves_failed_effects_and_historical_images() {
    let (server, catalog) = server();
    let original=exec(&server,"failed","const w=(await app.windows()).windows[0].window_ref;await app.capture({window_ref:w});throw new Error('after image');");
    assert!(original.is_error);
    let read = session_exec(
        &server,
        &format!(
            "const r=await session.result({});print(r.error);return r.script.failure;",
            original.data["execution_id"]
        ),
    );
    assert!(!read.is_error, "{}", read.data);
    assert_eq!(success(&read)["kind"], "exception");
    assert_eq!(read.image, original.image);
    assert_eq!(read.data["value"]["last_capture"]["timing"], "historical");
    assert_eq!(
        read.data["value"]["last_capture"]["recovered_from"],
        original.data["execution_id"]
    );
    assert_eq!(catalog.backend.0.captures.load(Ordering::SeqCst), 1);
    server.close().unwrap();
}

#[test]
fn receipt_vm_remains_available_after_native_cleanup_failure() {
    let (server, catalog) = server();
    catalog
        .backend
        .0
        .model
        .lock()
        .unwrap()
        .cleanup_after_capture = Some(Error::NativeFailure {
        code: "scene_cleanup_failed".into(),
        message: "controlled native cleanup failure".into(),
    });
    let original = exec(
        &server,
        "cleanup",
        "const w=(await app.windows()).windows[0].window_ref;await app.capture({window_ref:w});",
    );
    assert!(original.is_error);
    let before = catalog.connects.load(Ordering::SeqCst);
    let recovered=session_exec(&server,&format!("const r=await session.result({});return {{id:r.execution_id,cleanup:r.shutdown_errors}};",original.data["execution_id"]));
    assert!(!recovered.is_error, "{}", recovered.data);
    assert_eq!(
        recovered.data["value"]["result"]["id"],
        original.data["execution_id"]
    );
    assert_eq!(
        recovered.data["value"]["result"]["cleanup"][0]["code"],
        "scene_cleanup_failed"
    );
    assert_eq!(catalog.connects.load(Ordering::SeqCst), before);
    assert_eq!(catalog.backend.0.captures.load(Ordering::SeqCst), 1);
    let denied = session_exec(&server, "await app.launch();");
    assert_eq!(denied.data["script"]["failure"]["code"], "app_required");
    assert_eq!(catalog.connects.load(Ordering::SeqCst), before);
    assert!(server.close().is_err());
}

#[test]
fn ambiguous_bound_window_candidates_can_be_explicitly_selected_without_another_inventory_read() {
    let (server, catalog) = server();
    let selected = exec(&server, "ambiguous", "let ref;try{await app.window({timeout_ms:0});}catch(e){if(e.code!=='ambiguous_window')throw e;ref=e.details.candidates[0].window_ref;}const w=await app.window({window_ref:ref});await w.capture();return w.window_ref;");
    assert!(success(&selected).is_string());
    assert_eq!(catalog.backend.0.windows.load(Ordering::SeqCst), 1);
    assert_eq!(catalog.backend.0.captures.load(Ordering::SeqCst), 1);
    server.close().unwrap();
}

#[test]
fn receipts_can_be_read_while_the_native_owner_is_still_running() {
    let (server, catalog) = server();
    let gate = catalog.backend.action_gate();
    let call = match server.begin(json!("slow"), "exec", input(PRESS)) {
        Started::Call(call) => call,
        _ => panic!("not admitted"),
    };
    let id = latest(&server, APP).data["execution_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let running_server = server.clone();
    let running_call = call.clone();
    let thread = std::thread::spawn(move || running_server.run(&running_call));
    gate.entered();
    let observed = session_exec(
        &server,
        &format!("return (await session.result({})).status;", json!(id)),
    );
    assert_eq!(success(&observed), "running");
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    gate.release();
    let original = thread.join().unwrap();
    success(&original);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    server.close().unwrap();
}

#[test]
fn bound_window_uses_exact_refs_and_keeps_actions_in_one_script() {
    let (server, catalog) = server();
    let run=exec(&server,"bound","const w=await app.window({title:'Window 101'});state.window=w.window_ref;const o=await w.observe();await w.press({observation_ref:o.observation_ref,element_ref:o.elements[0].element_ref});await w.capture();return {frozen:Object.isFrozen(w),ref:w.window_ref};");
    assert_eq!(success(&run)["frozen"], true);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(catalog.backend.0.windows.load(Ordering::SeqCst), 1);
    let rebound=exec(&server,"rebound","const w=await app.window({window_ref:state.window});await w.capture();return w.window_ref;");
    assert_eq!(success(&rebound), &success(&run)["ref"]);
    assert_eq!(catalog.backend.0.windows.load(Ordering::SeqCst), 1);
    let ambiguous = exec(&server, "ambiguous", "await app.window();");
    assert_eq!(
        ambiguous.data["script"]["failure"]["code"],
        "ambiguous_window"
    );
    let override_target=exec(&server,"override","const w=await app.window({window_ref:state.window});await w.capture({window_ref:'other'});");
    assert_eq!(
        override_target.data["script"]["failure"]["code"],
        "invalid_args"
    );
    assert_eq!(catalog.backend.0.captures.load(Ordering::SeqCst), 2);
    server.close().unwrap();
}

#[test]
fn cancelled_session_reservation_keeps_other_desktop_work_and_state() {
    let (server, catalog) = server();
    success(&session_exec(&server, "state.saved=1;"));
    let args = json!({"source":"state.saved=2;print('unreachable');"});
    let call = match server.begin(json!("cancel-session"), "exec", args.clone()) {
        Started::Call(call) => call,
        _ => panic!("not admitted"),
    };
    assert!(matches!(
        server.begin(json!("cancel-session"), "exec", args),
        Started::InFlight(_)
    ));
    server.cancel(&json!("cancel-session"));
    let cancelled = server.run(&call);
    assert!(cancelled.is_error);
    assert_eq!(success(&session_exec(&server, "return state.saved;")), 1);
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 0);
    server.close().unwrap();
}
