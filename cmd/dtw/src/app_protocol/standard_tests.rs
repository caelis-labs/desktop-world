//! Public MCP contract uses one stable app ID and ordinary tool calls.
use super::*;
use crate::app_protocol::{
    app_id,
    mcp::{Event, Session},
    server::{Application, Factory, Server, Started},
};
const APP: &str = "org.example.Controlled";
#[derive(Clone)]
struct Catalog {
    entries: Arc<Mutex<Vec<Application>>>,
    backend: MockBackend,
    connects: Arc<AtomicUsize>,
    shutdowns_before_connect: Arc<Mutex<Vec<usize>>>,
}
impl Factory for Catalog {
    type Provider = MockBackend;
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
        Started::InFlight => panic!("unexpected duplicate frame"),
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
    assert!(
        apps.data["script"]["methods"]["observe"]["returns"]["properties"]["elements"]["items"]
            ["properties"]["role"]
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
fn ambiguous_bundle_id_is_listed_once_and_never_selects_a_pid() {
    let (server, catalog) = server();
    let mut duplicate = catalog.entries.lock().unwrap()[0].clone();
    duplicate.app.pid += 1;
    catalog.entries.lock().unwrap().push(duplicate);
    let apps = call(&server, "apps", json!({}));
    assert_eq!(apps.data["apps"].as_array().unwrap().len(), 1);
    assert_code(&exec(&server, "ambiguous", "return 1"), "ambiguous_app");
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 0);
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
fn message(id: Value, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}
fn response(event: Event) -> Value {
    match event {
        Event::Reply(value) => value,
        _ => panic!("expected response"),
    }
}
fn ready(version: &str) -> Session {
    let mut session = Session::default();
    let reply=response(session.handle(message(json!(1),"initialize",json!({"protocolVersion":version,"capabilities":{},"clientInfo":{"name":"standard-test","version":"1"}}))));
    assert_eq!(reply["result"]["protocolVersion"], version);
    assert!(matches!(
        session.handle(json!({"jsonrpc":"2.0","method":"notifications/initialized"})),
        Event::None
    ));
    session
}

fn framed_call(session: &mut Session, mut encoded: Vec<u8>) -> (Value, String, Value) {
    encoded.push(b'\n');
    let frame = mcp::read_frame(&mut std::io::Cursor::new(encoded))
        .unwrap()
        .unwrap();
    let Event::Call { id, name, args, .. } =
        session.handle(serde_json::from_slice(&frame).unwrap())
    else {
        panic!("expected a tool call")
    };
    (id, name, args)
}

#[test]
fn source_json_round_trip_preserves_javascript_and_result_characters() {
    let source = r#"const text = '中文🙂 "quoted" \'single\' \\';
const path = String.raw`C:\temp\new`;
const lines = "first\nsecond\tend";
const template = `中文🙂
${path}`;
return {text, path, lines, template, boundary: /\bword\b/.test("word"), literal: String.raw`\n`, nul: "\0"};"#;
    let expected = json!({
        "text": "中文🙂 \"quoted\" 'single' \\",
        "path": "C:\\temp\\new",
        "lines": "first\nsecond\tend",
        "template": "中文🙂\nC:\\temp\\new",
        "boundary": true,
        "literal": "\\n",
        "nul": "\0"
    });
    let (server, catalog) = server();
    let mut session = ready("2025-06-18");
    for (index, script) in [source.to_owned(), source.replace('\n', "\r\n")]
        .into_iter()
        .enumerate()
    {
        let request = message(
            json!(index + 2),
            "tools/call",
            json!({"name":"exec","arguments":input(&script)}),
        );
        // Exercise both UTF-8 and JSON Unicode escapes, including a surrogate pair.
        let encoded = serde_json::to_string(&request).unwrap();
        let encoded = if index == 0 {
            encoded
        } else {
            encoded
                .replace('中', "\\u4e2d")
                .replace('🙂', "\\ud83d\\ude42")
        };
        let (id, name, args) = framed_call(&mut session, encoded.into_bytes());
        assert_eq!(
            args["source"].as_str().unwrap().as_bytes(),
            script.as_bytes()
        );
        let reply = call_with_rpc(&server, id.clone(), &name, args);
        assert_eq!(success(&reply), &expected);
        let encoded = serde_json::to_vec(&mcp::response(id, reply, true)).unwrap();
        let returned: Value = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(
            returned["result"]["structuredContent"]["value"]["result"],
            expected
        );
    }
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
    server.close().unwrap();
}

#[test]
fn source_json_underescaping_is_not_silently_repaired() {
    // This is valid JSON, but JSON's backspace escape changes the JS regex.
    // A receiver cannot infer the author's intended word-boundary expression.
    let encoded = br#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"exec","arguments":{"app":"org.example.Controlled","source":"return /\bword\b/.test(\"word\");"}}}"#;
    let mut session = ready("2025-06-18");
    let (id, name, args) = framed_call(&mut session, encoded.to_vec());
    assert_eq!(args["source"], "return /\u{8}word\u{8}/.test(\"word\");");
    let (server, catalog) = server();
    let reply = call_with_rpc(&server, id, &name, args);
    assert_eq!(success(&reply), false);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
    server.close().unwrap();
}

#[test]
fn standard_mcp_lists_three_tools_with_simple_app_parameter() {
    let mut session = ready("2025-06-18");
    let listed = response(session.handle(message(json!(2), "tools/list", json!({}))));
    let tools = listed["result"]["tools"].as_array().unwrap();
    assert_eq!(
        tools
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["apps", "exec", "result"]
    );
    assert_eq!(
        tools[1]["inputSchema"]["properties"]["app"]["type"],
        "string"
    );
    assert_eq!(
        tools[1]["inputSchema"]["required"],
        json!(["app", "source"])
    );
    assert!(tools[1]["inputSchema"]["properties"]
        .get("request_id")
        .is_none());
    assert!(tools[1]["inputSchema"]["properties"]
        .get("execution_id")
        .is_none());
    assert_eq!(tools[2]["inputSchema"]["required"], json!(["app"]));
    assert!(tools[2]["inputSchema"]["properties"]["execution_id"].is_object());
    assert_eq!(tools[1]["annotations"]["readOnlyHint"], false);
    let event = session.handle(message(
        json!(3),
        "tools/call",
        json!({"name":"exec","arguments":input("return 1")}),
    ));
    assert!(matches!(event,Event::Call{id,..} if id==3));
    assert!(
        matches!(session.handle(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":3}})),Event::Cancel(id) if id==3)
    );
}
#[test]
fn legacy_mcp_clients_receive_text_without_new_structured_field() {
    let mut session = ready("2024-11-05");
    assert!(matches!(
        session.handle(message(json!(3), "tools/call", json!({"name":"apps"}))),
        Event::Call {
            structured: false,
            ..
        }
    ));
    let reply =
        crate::app_protocol::mcp::response(json!(3), Reply::plain(json!({"ok":true})), false);
    assert!(reply["result"].get("structuredContent").is_none());
    assert_eq!(reply["result"]["content"][0]["type"], "text");
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
fn ordinary_requests_do_not_rotate_the_native_owner_at_sixty_four_jobs() {
    let (server, catalog) = server();
    let capacity = call(&server, "apps", json!({})).data["limits"]["retained_executions"]
        .as_u64()
        .unwrap() as usize;
    let seed = exec(&server, "seed", "state.count=0;state.window=(await app.windows()).windows[0].window_ref;return state.window;");
    let original_ref = success(&seed).clone();
    assert!(capacity > MAX_REQUESTS + 2);
    for index in 1..capacity - 1 {
        let reply = exec(&server, &format!("count-{index}"), "return ++state.count;");
        assert_eq!(success(&reply), index);
        assert_eq!(reply.data["context_reset"], false);
        assert_eq!(reply.data["state_reset"], false);
    }
    let next = exec(&server, "retained-window", "const shot=await app.capture({window_ref:state.window});return {count:state.count,window:shot.window_ref};");
    assert_eq!(success(&next)["count"], capacity - 2);
    assert_eq!(success(&next)["window"], original_ref);
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 1);
    assert_eq!(catalog.backend.0.shutdowns.load(Ordering::SeqCst), 0);
    let exhausted = exec(
        &server,
        "past-session-capacity",
        "state.count=0;return await app.windows();",
    );
    assert_code(&exhausted, "request_limit");
    assert_eq!(receipt(&server, &next).data, next.data);
    assert_eq!(receipt(&server, &seed).data, seed.data);
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 1);
    assert_eq!(catalog.backend.0.windows.load(Ordering::SeqCst), 1);
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
    assert_code(&latest(&server, APP), "no_execution");
    let mut obsolete = input("return 1");
    obsolete["request_id"] = json!("model-authored-id");
    assert_code(&call(&server, "exec", obsolete), "invalid_arguments");
    assert_code(&latest(&server, APP), "no_execution");

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
        "unknown_execution",
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
        Started::InFlight
    ));
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 0);
    let running = server.clone();
    let worker = std::thread::spawn(move || running.run(&active));
    gate.entered();
    assert!(matches!(
        server.begin(json!(19), "exec", args.clone()),
        Started::InFlight
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
    assert_code(&latest(&right, APP), "no_execution");
    assert_code(&receipt(&right, &original), "unknown_execution");
    let independent = exec(&right, "same-rpc", PRESS);
    success(&independent);
    assert_ne!(
        independent.data["execution_id"],
        original.data["execution_id"]
    );
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 2);
    assert_code(&receipt(&right, &original), "unknown_execution");
    assert_eq!(
        latest(&right, APP).data["execution_id"],
        independent.data["execution_id"]
    );
    left.close().unwrap();
    right.close().unwrap();
}

#[test]
fn numeric_rpc_ids_are_canonical_for_retransmission_and_cancellation() {
    let mut session = ready("2025-06-18");
    let params = json!({"name":"exec","arguments":input("return 1")});
    let Event::Call { id: integer, .. } =
        session.handle(message(json!(2), "tools/call", params.clone()))
    else {
        panic!("missing integer call")
    };
    let Event::Call { id: floating, .. } =
        session.handle(message(json!(2.0), "tools/call", params.clone()))
    else {
        panic!("missing floating call")
    };
    let Event::Call { id: string, .. } =
        session.handle(message(json!("2"), "tools/call", params.clone()))
    else {
        panic!("missing string call")
    };
    assert_eq!(integer, floating);
    assert_ne!(integer, string);
    let Event::Cancel(cancel) = session.handle(
        json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":2.0}}),
    ) else {
        panic!("missing cancellation")
    };
    assert_eq!(cancel, integer);
    for id in [json!(null), json!(2.5), json!(9_007_199_254_740_992.0)] {
        assert_eq!(
            response(session.handle(message(id, "tools/call", params.clone())))["error"]["code"],
            -32600
        );
    }
}
