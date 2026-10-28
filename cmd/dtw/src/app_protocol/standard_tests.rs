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
fn call(server: &Arc<Server<Catalog>>, name: &str, args: Value) -> Reply {
    match server.begin(json!("rpc"), name, args) {
        Started::Reply(reply) => reply,
        Started::Call(call) => server.run(&call),
    }
}
fn input(id: &str, source: &str) -> Value {
    json!({"app":APP,"request_id":id,"source":source})
}
fn exec(server: &Arc<Server<Catalog>>, id: &str, source: &str) -> Reply {
    call(server, "exec", input(id, source))
}
fn receipt(server: &Arc<Server<Catalog>>, id: &str) -> Reply {
    call(server, "result", json!({"app":APP,"request_id":id}))
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
    let mut args = input("missing", "return 1");
    args.as_object_mut().unwrap().remove("app");
    assert!(call(&server, "exec", args).is_error);
    let mut args = input("unknown", "return 1");
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
    assert_eq!(receipt(&server, "once").data, original.data);
    assert_eq!(exec(&server, "once", PRESS).data, original.data);
    assert_code(
        &exec(&server, "once", "return 'different'"),
        "request_conflict",
    );
    assert!(
        call(
            &server,
            "result",
            json!({"app":"another.app","request_id":"once"})
        )
        .is_error
    );
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    server.close().unwrap();
}
#[test]
fn stable_id_survives_restart_but_state_and_refs_do_not() {
    let (server, catalog) = server();
    success(&exec(&server,"first","state.count=7;state.window=(await app.windows()).windows[0].window_ref;return state.window"));
    catalog.entries.lock().unwrap()[0].app.generation = 1;
    let reply = exec(&server, "restart", "return Object.keys(state)");
    assert_eq!(success(&reply), &json!([]));
    assert_eq!(reply.data["context_reset"], true);
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 2);
    assert!(!receipt(&server, "first").is_error);
    server.close().unwrap();
}
#[test]
fn cancellation_before_run_does_not_connect_or_dispatch() {
    let (server, catalog) = server();
    let Started::Call(active) = server.begin(json!(123), "exec", input("never", PRESS)) else {
        panic!("not admitted")
    };
    server.cancel(&json!(123));
    let reply = server.run(&active);
    assert!(reply.is_error);
    assert_eq!(reply.data["status"], "completed");
    assert_eq!(receipt(&server, "never").data["status"], "completed");
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 0);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
    assert!(receipt(&server, "never").is_error);
    server.close().unwrap();
}
#[test]
fn standard_cancel_targets_rpc_id_and_new_exec_uses_a_clean_context() {
    let (server, catalog) = server();
    catalog.backend.0.model.lock().unwrap().poll_action = true;
    let gate = catalog.backend.action_gate();
    success(&exec(&server, "state", "state.before=true;return 1"));
    let Started::Call(active) = server.begin(json!(123), "exec", input("original-action", PRESS))
    else {
        panic!("not admitted")
    };
    let running = server.clone();
    let worker = std::thread::spawn(move || running.run(&active));
    gate.entered();
    server.cancel(&json!("original-action")); // operation ID is not the JSON-RPC ID
    assert_code(&exec(&server, "busy", "return 1"), "busy");
    server.cancel(&json!(123));
    let reply = worker.join().unwrap();
    assert!(reply.is_error, "{}", reply.data);
    assert_eq!(reply.data["request_id"], "original-action");
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
    assert_eq!(receipt(&server, "original-action").data, reply.data);
    let next = exec(&server, "fresh", "return Object.keys(state)");
    assert_eq!(success(&next), &json!([]));
    assert_eq!(next.data["context_reset"], true);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    server.close().unwrap();
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
    assert!(tools[1]["inputSchema"]["required"]
        .as_array()
        .unwrap()
        .contains(&json!("app")));
    assert_eq!(tools[1]["annotations"]["readOnlyHint"], false);
    let event = session.handle(message(
        json!(3),
        "tools/call",
        json!({"name":"exec","arguments":input("id","return 1")}),
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

#[test]
fn switching_apps_retires_each_owner_before_connecting_and_preserves_original_receipts() {
    let (server, catalog) = server();
    let apps = call(&server, "apps", json!({}));
    assert_eq!(apps.data["limits"]["resident_app_contexts"], 1);
    let original_action = exec(&server, "original-action", PRESS);
    success(&original_action);
    let original_pixels = exec(&server, "original-pixels",
        "state.old='A'; const window_ref=(await app.windows()).windows[0].window_ref; return await app.capture({window_ref});");
    let old = success(&original_pixels).clone();
    assert!(original_pixels.image.is_some());

    let mut other = application();
    other.identity = AppIdentity("org.example.Other".into());
    other.pid += 1;
    catalog.entries.lock().unwrap().push(Application {
        app_id: "org.example.Other".into(),
        app: other,
        name: "Other".into(),
    });
    let switched = call(
        &server,
        "exec",
        json!({"app":"org.example.Other","request_id":"other-pixels",
        "source":"state.old='B'; const window_ref=(await app.windows()).windows[0].window_ref; return await app.capture({window_ref});"}),
    );
    success(&switched);
    assert!(switched.image.is_some());
    assert_eq!(catalog.backend.0.shutdowns.load(Ordering::SeqCst), 1);
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 2);

    let returned = exec(
        &server,
        "return",
        &format!(
            r#"
const old = {old};
let staleCode = null;
try {{ await app.click({{window_ref:old.window_ref,observation_ref:old.observation_ref,x:1,y:1}}); }}
catch(error) {{ staleCode=error.code; }}
const keys = Object.keys(state);
const window_ref = (await app.windows()).windows[0].window_ref;
const fresh = await app.capture({{window_ref}});
return {{keys,staleCode,fresh}};
"#
        ),
    );
    let value = success(&returned);
    assert_eq!(returned.data["context_reset"], true);
    assert_eq!(value["keys"], json!([]));
    assert_eq!(value["staleCode"], "target_changed");
    assert_ne!(value["fresh"]["window_ref"], old["window_ref"]);
    assert_ne!(value["fresh"]["observation_ref"], old["observation_ref"]);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(catalog.backend.0.captures.load(Ordering::SeqCst), 3);
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 3);
    assert_eq!(catalog.backend.0.shutdowns.load(Ordering::SeqCst), 2);
    assert_eq!(
        *catalog.shutdowns_before_connect.lock().unwrap(),
        vec![0, 1, 2]
    );
    assert_eq!(
        receipt(&server, "original-action").data,
        original_action.data
    );
    let retained_pixels = receipt(&server, "original-pixels");
    assert_eq!(retained_pixels.data, original_pixels.data);
    assert_eq!(retained_pixels.image, original_pixels.image);
    assert_eq!(
        call(
            &server,
            "result",
            json!({"app":"org.example.Other","request_id":"other-pixels"})
        )
        .data,
        switched.data
    );
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
