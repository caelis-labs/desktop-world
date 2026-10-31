//! Public MCP contract uses one stable app ID and ordinary tool calls.
use super::*;
use crate::app_protocol::{
    app_id,
    server::{Application, Factory, Server, Started},
};
const APP: &str = "org.example.Controlled";
#[derive(Clone)]
struct Catalog {
    input_policy: InputPolicy,
    entries: Arc<Mutex<Vec<Application>>>,
    backend: MockBackend,
    connects: Arc<AtomicUsize>,
    shutdowns_before_connect: Arc<Mutex<Vec<usize>>>,
}
impl Factory for Catalog {
    type Provider = MockBackend;
    fn input_policy(&self) -> InputPolicy {
        self.input_policy
    }
    fn catalog(&self) -> Result<Vec<Application>, Error> {
        Ok(self.entries.lock().unwrap().clone())
    }
    fn connect(&self) -> Result<MockBackend, Error> {
        self.shutdowns_before_connect
            .lock()
            .unwrap()
            .push(self.backend.0.shutdowns.load(Ordering::SeqCst));
        self.connects.fetch_add(1, Ordering::SeqCst);
        Ok(self.backend.clone())
    }
    fn close_catalog(&self) -> Result<(), Error> {
        Ok(())
    }
}
fn server() -> (Arc<Server<Catalog>>, Catalog) {
    let catalog = Catalog {
        input_policy: InputPolicy::Auto,
        entries: Arc::new(Mutex::new(vec![Application {
            app_id: APP.into(),
            app: application(),
            name: "Controlled".into(),
        }])),
        backend: MockBackend::new(),
        connects: Arc::new(AtomicUsize::new(0)),
        shutdowns_before_connect: Arc::new(Mutex::new(Vec::new())),
    };
    (Server::new(catalog.clone()), catalog)
}
fn call_with_rpc(server: &Arc<Server<Catalog>>, rpc_id: Value, name: &str, args: Value) -> Reply {
    match server.begin(rpc_id, name, args) {
        Started::Reply(reply) => reply,
        Started::Call(call) => server.run(&call),
        Started::InFlight(_) => panic!("unexpected duplicate frame"),
    }
}
fn call(server: &Arc<Server<Catalog>>, name: &str, args: Value) -> Reply {
    call_with_rpc(server, json!(uuid::Uuid::new_v4().to_string()), name, args)
}
fn input(source: &str) -> Value {
    json!({"app":APP,"source":source})
}
fn exec(server: &Arc<Server<Catalog>>, rpc_id: &str, source: &str) -> Reply {
    call_with_rpc(server, json!(rpc_id), "exec", input(source))
}
fn receipt(server: &Arc<Server<Catalog>>, original: &Reply) -> Reply {
    call(
        server,
        "result",
        json!({"app":original.data["app"],"execution_id":original.data["execution_id"]}),
    )
}
fn latest(server: &Arc<Server<Catalog>>, app: &str) -> Reply {
    call(server, "result", json!({"app":app}))
}
fn success(reply: &Reply) -> &Value {
    assert!(!reply.is_error, "{}", reply.data);
    assert_eq!(reply.data["status"], "completed");
    &reply.data["value"]["result"]
}
const PRESS:&str="const w=(await app.windows()).windows[0].window_ref;const o=await app.observe({window_ref:w});return await app.press({window_ref:w,observation_ref:o.observation_ref,element_ref:o.elements[0].element_ref});";

#[test]
fn app_identifier_is_a_plain_stable_platform_id() {
    assert_eq!(
        app_id::parse(&json!("com.google.Chrome")).unwrap(),
        "com.google.Chrome"
    );
    for bad in [
        Value::Null,
        json!(42),
        json!({"identity":APP,"pid":42,"generation":"1"}),
        json!(""),
        json!(" app "),
        json!("bad\napp"),
    ] {
        assert!(app_id::parse(&bad).is_err());
    }
    assert_eq!(app_id::schema()["type"], "string");
}
#[test]
fn ordinary_discovery_and_exec_need_no_custom_host_or_grant() {
    let (server, catalog) = server();
    let apps = call(&server, "apps", json!({}));
    assert_eq!(apps.data["apps"], json!([{"app":APP,"name":"Controlled"}]));
    assert!(apps.data["script"]["methods"]["observe"].is_string());
    let api = call(&server, "api", json!({"methods":["observe"]}));
    assert!(
        api.data["methods"]["observe"]["returns"]["properties"]["elements"]["items"]["properties"]
            ["role"]
            .is_object()
    );
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 0);
    let reply = exec(
        &server,
        "read",
        "return app.windows().then(value=>value.windows.length);",
    );
    assert_eq!(success(&reply), 2);
    assert_eq!(reply.data["app"], APP);
    assert!(reply.data.get("pid").is_none());
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 1);
    server.close().unwrap();
}
#[test]
fn no_missing_or_unlisted_app_can_dispatch_even_pure_javascript() {
    let (server, catalog) = server();
    let mut args = input("return 1");
    args.as_object_mut().unwrap().remove("app");
    assert!(call(&server, "exec", args).is_error);
    let mut args = input("return 1");
    args["app"] = json!("com.example.Unknown");
    let reply = call(&server, "exec", args);
    assert!(reply.is_error, "{}", reply.data);
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 0);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
    server.close().unwrap();
}
#[test]
fn multiple_instances_are_one_public_app_and_share_one_script_scope() {
    let (server, catalog) = server();
    let mut duplicate = catalog.entries.lock().unwrap()[0].clone();
    duplicate.app.pid += 1;
    catalog.entries.lock().unwrap().push(duplicate);
    let apps = call(&server, "apps", json!({}));
    assert_eq!(apps.data["apps"].as_array().unwrap().len(), 1);
    let reply = exec(
        &server,
        "multiple",
        "return (await app.windows()).windows.length",
    );
    assert_eq!(success(&reply), 4);
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 1);
    server.close().unwrap();
}
#[test]
fn original_result_and_duplicate_survive_app_exit_without_replaying() {
    let (server, catalog) = server();
    let original = exec(&server, "once", PRESS);
    success(&original);
    catalog.entries.lock().unwrap().clear();
    assert_eq!(receipt(&server, &original).data, original.data);
    assert_eq!(exec(&server, "once", PRESS).data, original.data);
    assert_code(
        &exec(&server, "once", "return 'different'"),
        "request_conflict",
    );
    assert!(
        call(
            &server,
            "result",
            json!({"app":"another.app","execution_id":original.data["execution_id"]})
        )
        .is_error
    );
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    server.close().unwrap();
}
#[test]
fn stable_id_survives_restart_but_state_and_refs_do_not() {
    let (server, catalog) = server();
    let first = exec(&server,"first","state.count=7;state.window=(await app.windows()).windows[0].window_ref;return state.window");
    success(&first);
    catalog.entries.lock().unwrap()[0].app.generation = 1;
    let reply = exec(&server, "restart", "return Object.keys(state)");
    assert_eq!(success(&reply), &json!([]));
    assert_eq!(reply.data["context_reset"], true);
    assert_eq!(reply.data["state_reset"], true);
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 2);
    assert!(!receipt(&server, &first).is_error);
    server.close().unwrap();
}
#[test]
fn cancellation_before_run_does_not_connect_or_dispatch() {
    let (server, catalog) = server();
    let Started::Call(active) = server.begin(json!(123), "exec", input(PRESS)) else {
        panic!("not admitted")
    };
    let preparing = latest(&server, APP);
    assert_eq!(preparing.data["status"], "running");
    assert_eq!(preparing.data["selection"], "latest_accepted");
    // Only the transport request ID cancels; the returned execution ID is for reads.
    server.cancel(&preparing.data["execution_id"]);
    assert!(!active.cancelled.load(Ordering::Acquire));
    server.cancel(&json!(123));
    let reply = server.run(&active);
    assert!(reply.is_error);
    assert_eq!(reply.data["status"], "completed");
    assert_eq!(receipt(&server, &reply).data["status"], "completed");
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 0);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
    assert!(receipt(&server, &reply).is_error);
    let recovered = latest(&server, APP);
    assert!(recovered.is_error);
    assert_eq!(
        recovered.data["execution_id"],
        preparing.data["execution_id"]
    );
    assert_eq!(recovered.data["status"], "completed");
    server.close().unwrap();
}
#[test]
fn standard_cancel_preserves_committed_state_and_other_app_contexts() {
    let (server, catalog) = server();
    catalog.backend.0.model.lock().unwrap().poll_action = true;
    let gate = catalog.backend.action_gate();
    add_app(&catalog, "org.example.Other", 43);
    success(&call(
        &server,
        "exec",
        json!({"app":"org.example.Other",
        "source":"state.peer=true;state.window=(await app.windows()).windows[0].window_ref;return 1;"}),
    ));
    success(&exec(&server, "state", "state.before=true;return 1"));
    let source = format!("state.uncommitted=true;{PRESS}");
    let Started::Call(active) = server.begin(json!(123), "exec", input(&source)) else {
        panic!("not admitted")
    };
    let running = server.clone();
    let worker = std::thread::spawn(move || running.run(&active));
    gate.entered();
    let running = latest(&server, APP);
    server.cancel(&running.data["execution_id"]);
    assert_code(&exec(&server, "busy", "return 1"), "busy");
    server.cancel(&json!(123));
    let reply = worker.join().unwrap();
    assert!(reply.is_error, "{}", reply.data);
    assert!(reply.data["execution_id"].is_string());
    assert!(reply.data.get("request_id").is_none());
    assert_eq!(reply.data["execution_id"], running.data["execution_id"]);
    let step = reply.data["script"]["steps"]
        .as_array()
        .unwrap()
        .last()
        .unwrap();
    assert_eq!(
        step["receipt"]["outcome"]["dispatch"], "unknown",
        "{}",
        reply.data
    );
    assert_eq!(receipt(&server, &reply).data, reply.data);
    let next = exec(&server, "fresh", "return Object.keys(state)");
    assert_eq!(success(&next), &json!(["before"]));
    assert_eq!(next.data["context_reset"], true);
    assert_eq!(next.data["state_reset"], false);
    let peer = call(
        &server,
        "exec",
        json!({"app":"org.example.Other",
        "source":"await app.capture({window_ref:state.window});return state.peer;"}),
    );
    assert_eq!(success(&peer), true);
    assert_eq!(peer.data["context_reset"], false);
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 3);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    server.close().unwrap();
}

#[test]
fn native_cleanup_failure_closes_dispatch_for_every_app_and_keeps_original_receipts() {
    let (server, catalog) = server();
    add_app(&catalog, "org.example.Other", 43);
    let other = json!({"app":"org.example.Other","source":"state.kept=true;return 'peer';"});
    let peer = call_with_rpc(&server, json!("peer-rpc"), "exec", other.clone());
    success(&peer);
    catalog
        .backend
        .0
        .model
        .lock()
        .unwrap()
        .cleanup_after_capture = Some(Error::NativeFailure {
        code: "scene_cleanup_failed".into(),
        message: "original scene cleanup failure".into(),
    });
    let source = "const w=(await app.windows()).windows[0].window_ref;try { await app.capture({window_ref:w}); } catch (_) {} state.invalid=true;return 'pretend success';";
    let original = exec(&server, "capture-cleanup", source);
    assert!(original.is_error, "{}", original.data);
    assert_eq!(
        original.data["shutdown_errors"][0]["code"],
        "scene_cleanup_failed"
    );
    assert_eq!(original.data["script"]["state_committed"], false);
    let blocked = call(
        &server,
        "exec",
        json!({"app":"org.example.Other","source":"return app.windows();"}),
    );
    assert_code(&blocked, "shutdown_failed");
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 2);
    assert_eq!(catalog.backend.0.windows.load(Ordering::SeqCst), 1);
    assert_eq!(catalog.backend.0.captures.load(Ordering::SeqCst), 1);
    assert_eq!(receipt(&server, &original).data, original.data);
    assert_eq!(exec(&server, "capture-cleanup", source).data, original.data);
    // Earlier app results retain their payload and per-job outcome; session
    // shutdown diagnostics are appended independently by the public Server.
    let prior = call_with_rpc(&server, json!("peer-rpc"), "exec", other);
    assert_eq!(prior.data["value"], peer.data["value"]);
    assert_eq!(prior.data["script"], peer.data["script"]);
    assert!(prior.data.get("shutdown_error").is_none());
    assert!(server.close().is_err());
}
#[test]
fn generated_types_are_the_distributed_contract() {
    assert_eq!(
        crate::app_protocol::script_api_types(),
        include_str!("../../../../docs/app-script-api.d.ts")
    );
}

fn add_app(catalog: &Catalog, app_id: &str, pid: u32) {
    let mut app = application();
    app.identity = AppIdentity(app_id.into());
    app.pid = pid;
    catalog.entries.lock().unwrap().push(Application {
        app_id: app_id.into(),
        app,
        name: app_id.into(),
    });
}

#[test]
fn switching_apps_preserves_each_apps_state_refs_and_original_receipts() {
    let (server, catalog) = server();
    let original_action = exec(&server, "original-action", PRESS);
    success(&original_action);
    let original_pixels = exec(&server, "original-pixels",
        "state.old='A'; state.window=(await app.windows()).windows[0].window_ref; return await app.capture({window_ref:state.window});");
    let old = success(&original_pixels).clone();
    assert!(original_pixels.image.is_some());

    add_app(&catalog, "org.example.Other", 43);
    let switched = call(
        &server,
        "exec",
        json!({"app":"org.example.Other",
        "source":"state.old='B'; const window_ref=(await app.windows()).windows[0].window_ref; return await app.capture({window_ref});"}),
    );
    success(&switched);
    assert!(switched.image.is_some());
    assert_eq!(catalog.backend.0.shutdowns.load(Ordering::SeqCst), 0);
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 2);

    let returned = exec(
        &server,
        "return",
        &format!(
            r#"
const old = {old};
await app.click({{window_ref:old.window_ref,observation_ref:old.observation_ref,x:1,y:1}});
const fresh = await app.capture({{window_ref:state.window}});
return {{old:state.old, fresh}};
"#
        ),
    );
    let value = success(&returned);
    assert_eq!(returned.data["context_reset"], false);
    assert_eq!(returned.data["state_reset"], false);
    assert_eq!(value["old"], "A");
    assert_eq!(value["fresh"]["window_ref"], old["window_ref"]);
    assert_ne!(value["fresh"]["observation_ref"], old["observation_ref"]);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 2);
    assert_eq!(catalog.backend.0.captures.load(Ordering::SeqCst), 3);
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 2);
    assert_eq!(catalog.backend.0.shutdowns.load(Ordering::SeqCst), 0);
    assert_eq!(
        *catalog.shutdowns_before_connect.lock().unwrap(),
        vec![0, 0]
    );
    assert_eq!(
        receipt(&server, &original_action).data,
        original_action.data
    );
    let retained_pixels = receipt(&server, &original_pixels);
    assert_eq!(retained_pixels.data, original_pixels.data);
    assert_eq!(retained_pixels.image, original_pixels.image);
    assert_eq!(
        call(
            &server,
            "result",
            json!({"app":"org.example.Other","execution_id":switched.data["execution_id"]})
        )
        .data,
        switched.data
    );
    server.close().unwrap();
}

#[test]
fn sliding_history_preserves_the_app_state_and_window_after_more_than_512_calls() {
    let (server, catalog) = server();
    let capacity = call(&server, "apps", json!({})).data["limits"]["retained_executions"]
        .as_u64()
        .unwrap() as usize;
    let seed = exec(&server, "seed", "state.count=0;state.window=(await app.windows()).windows[0].window_ref;return state.window;");
    let original_ref = success(&seed).clone();
    let total = capacity * 2 + 3;
    for index in 1..=total {
        let reply = exec(&server, &format!("count-{index}"), "return ++state.count;");
        assert_eq!(success(&reply), index);
        assert_eq!(reply.data["context_reset"], false);
        assert_eq!(reply.data["state_reset"], false);
    }
    let next = exec(&server, "retained-window", "const shot=await app.capture({window_ref:state.window});return {count:state.count,window:shot.window_ref};");
    assert_eq!(success(&next)["count"], total);
    assert_eq!(success(&next)["window"], original_ref);
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 1);
    assert_eq!(catalog.backend.0.shutdowns.load(Ordering::SeqCst), 0);
    assert_eq!(receipt(&server, &next).data, next.data);
    assert_eq!(latest(&server, APP).data["value"], next.data["value"]);
    assert_code(&receipt(&server, &seed), "execution_unavailable");
    // Read-only history lookup neither repeats the original windows call nor
    // rebuilds the native owner holding the still-valid window reference.
    assert_eq!(catalog.backend.0.windows.load(Ordering::SeqCst), 1);
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 1);
    server.close().unwrap();
}

#[test]
fn independent_sessions_never_share_json_state_or_native_refs() {
    let (first, _) = server();
    let (second, _) = server();
    let original = exec(&first, "seed", "state.owner='first';state.window=(await app.windows()).windows[0].window_ref;return state.window;");
    let foreign_ref = success(&original).clone();
    assert_eq!(
        success(&exec(&second, "seed", "return Object.keys(state);")),
        &json!([])
    );
    let foreign = exec(
        &second,
        "foreign",
        &format!("return await app.capture({{window_ref:{foreign_ref}}});"),
    );
    assert!(foreign.is_error);
    assert_eq!(
        foreign.data["script"]["steps"][0]["error"]["code"],
        "target_changed"
    );
    first.close().unwrap();
    let live = exec(&second, "live", "state.owner='second';const w=(await app.windows()).windows[0].window_ref;await app.capture({window_ref:w});return state.owner;");
    assert_eq!(success(&live), "second");
    assert_eq!(live.data["context_reset"], false);
    second.close().unwrap();
}

#[test]
fn app_switches_do_not_evict_a_small_working_set() {
    let (server, catalog) = server();
    for index in 0..8 {
        let app_id = format!("org.example.Peer{index}");
        add_app(&catalog, &app_id, 100 + index);
        success(&call(
            &server,
            "exec",
            json!({"app":app_id,
            "source":format!("state.index={index};state.window=(await app.windows()).windows[0].window_ref;return state.index;")}),
        ));
    }
    for index in 0..8 {
        let value = call(
            &server,
            "exec",
            json!({"app":format!("org.example.Peer{index}"),
            "source":"await app.capture({window_ref:state.window});return state.index;"}),
        );
        assert_eq!(success(&value), index);
        assert_eq!(value.data["context_reset"], false);
    }
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 8);
    assert_eq!(catalog.backend.0.shutdowns.load(Ordering::SeqCst), 0);
    server.close().unwrap();
}
#[test]
fn one_method_registry_covers_facade_schemas_and_types() {
    let spec = crate::app_protocol::scripts::description();
    let methods = spec["methods"].as_object().unwrap();
    assert_eq!(
        methods.len(),
        crate::app_protocol::methods::Method::ALL.len()
    );
    let mcp_tools = mcp::tools();
    let names: Vec<_> = crate::app_protocol::methods::Method::ALL
        .into_iter()
        .map(|method| method.name())
        .collect();
    assert_eq!(
        mcp_tools[1]["inputSchema"]["properties"]["methods"]["items"]["enum"],
        json!(names)
    );
    assert_eq!(
        mcp_tools[1]["inputSchema"]["properties"]["methods"]["maxItems"],
        names.len()
    );
    for method in crate::app_protocol::methods::Method::ALL {
        let spec = &methods[method.name()];
        assert!(spec.get("parameters").is_some());
        assert!(spec["returns"].is_object());
        assert_eq!(spec["requires"], json!([method.capability().as_str()]));
    }
}

#[test]
fn execution_ids_are_automatic_and_new_rpc_ids_repeat_the_requested_effects() {
    let (server, catalog) = server();
    assert_code(&latest(&server, APP), "execution_unavailable");
    let mut obsolete = input("return 1");
    obsolete["request_id"] = json!("model-authored-id");
    assert_code(&call(&server, "exec", obsolete), "invalid_arguments");
    assert_code(&latest(&server, APP), "execution_unavailable");

    let source = format!(
        "state.count=(state.count||0)+1;{}return state.count;",
        PRESS.replace("return await", "await")
    );
    let first = exec(&server, "rpc-first", &source);
    assert_eq!(success(&first), 1);
    let id = first.data["execution_id"].as_str().unwrap();
    assert_ne!(id, "rpc-first");
    assert!(first.data.get("request_id").is_none());
    assert_eq!(exec(&server, "rpc-first", &source).data, first.data);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);

    let second = exec(&server, "rpc-second", &source);
    assert_eq!(success(&second), 2);
    assert_ne!(second.data["execution_id"], first.data["execution_id"]);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 2);
    let recent = latest(&server, APP);
    assert_eq!(success(&recent), 2);
    assert_eq!(recent.data["execution_id"], second.data["execution_id"]);
    assert_eq!(recent.data["selection"], "latest_accepted");
    assert_eq!(receipt(&server, &first).data, first.data);
    assert_eq!(
        latest(&server, APP).data["execution_id"],
        second.data["execution_id"]
    );

    for bad in [json!(null), json!(""), json!(42), json!("x".repeat(129))] {
        assert_code(
            &call(&server, "result", json!({"app":APP,"execution_id":bad})),
            "invalid_arguments",
        );
    }
    assert_code(
        &call(
            &server,
            "result",
            json!({"app":APP,"request_id":"rpc-first"}),
        ),
        "invalid_arguments",
    );
    assert_code(
        &call(
            &server,
            "result",
            json!({"app":APP,"execution_id":"not-in-this-session"}),
        ),
        "execution_unavailable",
    );
    server.close().unwrap();
}

#[test]
fn duplicate_active_rpc_frames_share_one_execution_and_original_response() {
    let (server, catalog) = server();
    let gate = catalog.backend.action_gate();
    let args = input(PRESS);
    let Started::Call(active) = server.begin(json!(19), "exec", args.clone()) else {
        panic!("execution was not accepted")
    };
    let preparing = latest(&server, APP);
    assert!(matches!(
        server.begin(json!(19), "exec", args.clone()),
        Started::InFlight(_)
    ));
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 0);
    let running = server.clone();
    let worker = std::thread::spawn(move || running.run(&active));
    gate.entered();
    assert!(matches!(
        server.begin(json!(19), "exec", args.clone()),
        Started::InFlight(_)
    ));
    assert_eq!(
        latest(&server, APP).data["execution_id"],
        preparing.data["execution_id"]
    );

    for (field, value) in [
        ("app", json!("another.app")),
        ("source", json!("return 2")),
        ("timeout_ms", json!(30_000)),
        ("max_calls", json!(20)),
    ] {
        let mut changed = args.clone();
        changed[field] = value;
        assert_code(
            &call_with_rpc(&server, json!(19), "exec", changed),
            "request_conflict",
        );
    }
    assert_code(
        &call_with_rpc(&server, json!(19), "result", json!({"app":APP})),
        "request_conflict",
    );
    assert_code(
        &call_with_rpc(&server, json!(19), "apps", json!({})),
        "request_conflict",
    );
    let busy = call_with_rpc(&server, json!(20), "exec", args.clone());
    assert_code(&busy, "busy");
    assert!(busy.data.get("execution_id").is_none());
    assert_eq!(
        latest(&server, APP).data["execution_id"],
        preparing.data["execution_id"]
    );
    gate.release();
    let original = worker.join().unwrap();
    success(&original);
    assert_eq!(
        original.data["execution_id"],
        preparing.data["execution_id"]
    );
    assert_eq!(
        call_with_rpc(&server, json!(19), "exec", args).data,
        original.data
    );
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    server.close().unwrap();
}

#[test]
fn latest_selects_accepted_failures_and_remains_readable_after_app_exit() {
    let (server, catalog) = server();
    let first = exec(&server, "first", "state.kept=true;return 1");
    success(&first);
    let failed = exec(
        &server,
        "failed",
        "state.uncommitted=true;throw Error('failed');",
    );
    assert!(failed.is_error);
    assert_eq!(failed.data["script"]["state_committed"], false);
    assert_eq!(
        latest(&server, APP).data["execution_id"],
        failed.data["execution_id"]
    );
    assert!(latest(&server, APP).is_error);
    assert_code(
        &call(&server, "exec", json!({"app":APP,"source":""})),
        "invalid_arguments",
    );
    assert_eq!(
        latest(&server, APP).data["execution_id"],
        failed.data["execution_id"]
    );
    catalog.entries.lock().unwrap().clear();
    assert_eq!(
        latest(&server, APP).data["execution_id"],
        failed.data["execution_id"]
    );
    let unavailable = exec(&server, "unavailable", "return 3");
    assert!(unavailable.is_error);
    assert!(unavailable.data["execution_id"].is_string());
    assert_eq!(
        latest(&server, APP).data["execution_id"],
        unavailable.data["execution_id"]
    );
    assert_eq!(receipt(&server, &first).data, first.data);
    server.close().unwrap();
    assert_eq!(
        latest(&server, APP).data["execution_id"],
        unavailable.data["execution_id"]
    );
}

#[test]
fn execution_lookup_and_transport_deduplication_are_session_and_app_scoped() {
    let (left, catalog) = server();
    add_app(&catalog, "org.example.Other", 43);
    let right = Server::new(catalog.clone());
    let original = exec(&left, "same-rpc", PRESS);
    let other = call(
        &left,
        "exec",
        json!({"app":"org.example.Other","source":"return 'other'"}),
    );
    assert_eq!(
        latest(&left, APP).data["execution_id"],
        original.data["execution_id"]
    );
    assert_eq!(
        latest(&left, "org.example.Other").data["execution_id"],
        other.data["execution_id"]
    );
    assert_code(
        &call(
            &left,
            "result",
            json!({"app":"org.example.Other","execution_id":original.data["execution_id"]}),
        ),
        "wrong_app",
    );
    assert_code(&latest(&right, APP), "execution_unavailable");
    assert_code(&receipt(&right, &original), "execution_unavailable");
    let independent = exec(&right, "same-rpc", PRESS);
    success(&independent);
    assert_ne!(
        independent.data["execution_id"],
        original.data["execution_id"]
    );
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 2);
    assert_code(&receipt(&right, &original), "execution_unavailable");
    assert_eq!(
        latest(&right, APP).data["execution_id"],
        independent.data["execution_id"]
    );
    left.close().unwrap();
    right.close().unwrap();
}

#[path = "mcp_tests.rs"]
mod mcp_tests;

#[test]
fn default_discovery_is_bounded_and_method_details_are_opt_in_without_native_access() {
    let (server, catalog) = server();
    let mut apps = call(&server, "apps", json!({}));
    apps.data["apps"] = json!([]);
    assert!(
        apps.data.to_string().len() <= 4096,
        "{}",
        apps.data.to_string().len()
    );
    let details = call(&server, "api", json!({"methods":["typeText","keypress"]}));
    assert!(!details.is_error);
    assert_eq!(details.data["methods"].as_object().unwrap().len(), 2);
    assert_eq!(
        details.data["methods"]["typeText"]["parameters"]["required"],
        json!(["window_ref", "text"])
    );
    assert!(details.data["methods"].get("observe").is_none());
    assert!(call(&server, "api", json!({"methods":["shell"]})).is_error);
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 0);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
}

#[test]
fn window_input_scripts_use_the_same_action_ledger_without_observe_round_trips() {
    let (server, catalog) = server();
    let reply = exec(
        &server,
        "input",
        r#"
        const window_ref=(await app.windows()).windows[0].window_ref;
        await app.focus({window_ref});
        await app.typeText({window_ref,text:'中文 🐦\nSecond line'});
        await app.keypress({window_ref,keys:['META','A']});
        await app.scroll({window_ref,delta_y:3});
        return {done:true};
    "#,
    );
    assert_eq!(success(&reply), &json!({"done":true}));
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 4);
    assert_eq!(catalog.backend.0.observes.load(Ordering::SeqCst), 0);
    assert_eq!(reply.data["script"]["steps"].as_array().unwrap().len(), 5);
    let original = receipt(&server, &reply);
    assert_eq!(original.data["script"], reply.data["script"]);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 4);
}

#[test]
fn invalid_input_arguments_reject_before_native_dispatch_and_leave_read_bindings_live() {
    let (server, catalog) = server();
    let reply = exec(
        &server,
        "invalid-input",
        r#"
        const window_ref=(await app.windows()).windows[0].window_ref;
        const observed=await app.observe({window_ref});
        const failures=[];
        for (const bad of [()=>app.keypress({window_ref,keys:[]}),()=>app.scroll({window_ref,delta_y:0}),()=>app.typeText({window_ref,text:'\0'})]) {
            try { await bad(); } catch(e) { failures.push(e.code); }
        }
        await app.press({window_ref,observation_ref:observed.observation_ref,element_ref:observed.elements[0].element_ref});
        return failures;
    "#,
    );
    assert_eq!(
        success(&reply),
        &json!(["invalid_args", "invalid_args", "invalid_args"])
    );
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
}

#[test]
fn unicode_and_json_expansion_are_rejected_before_input_admission() {
    let (server, catalog) = server();
    let reply = exec(
        &server,
        "text-bounds",
        r#"
        const window_ref=(await app.windows()).windows[0].window_ref;
        const errors=[];
        for(const text of ['🐦'.repeat(4097),'\x01'.repeat(8192)]) {
            try { await app.typeText({window_ref,text}); } catch(e) { errors.push(e.code); }
        }
        return errors;
    "#,
    );
    assert_eq!(success(&reply), &json!(["invalid_args", "invalid_args"]));
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
}

#[test]
fn service_policy_is_discoverable_in_apps_and_filtered_api_without_connecting() {
    let (server_auto, mut catalog) = server();
    let automatic = call(&server_auto, "api", json!({}));
    assert_eq!(automatic.data["service"]["input_policy"], "auto");
    catalog.input_policy = InputPolicy::BackgroundOnly;
    let server = Server::new(catalog.clone());
    for args in [json!({}), json!({"methods":["click"]})] {
        let reply = call(&server, "api", args);
        assert!(!reply.is_error);
        assert_eq!(reply.data["service"]["input_policy"], "background_only");
    }
    let apps = call(&server, "apps", json!({}));
    assert_eq!(apps.data["service"]["input_policy"], "background_only");
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 0);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
}
