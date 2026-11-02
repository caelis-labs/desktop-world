//! The production app facade; the surrounding suite tests the private atomic DSL.
use super::*;
fn agent() -> (Arc<Server<Catalog>>, Catalog) {
    let (_, catalog) = server();
    (Server::new(catalog.clone()), catalog)
}
#[test]
fn public_tools_are_apps_and_exec_and_missing_app_never_connects() {
    let (server, catalog) = agent();
    let tools = super::super::mcp::tools();
    assert_eq!(
        tools
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["apps", "exec"]
    );
    assert_eq!(
        tools[1]["inputSchema"]["required"],
        json!(["app", "source"])
    );
    for (name, args) in [
        ("exec", json!({"source":"return 1"})),
        ("api", json!({})),
        ("result", json!({})),
    ] {
        let Started::Reply(reply) =
            server.begin(json!(uuid::Uuid::new_v4().to_string()), name, args)
        else {
            panic!("invalid public request admitted")
        };
        assert!(reply.is_error);
    }
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 0);
}
#[test]
fn agent_sees_only_app_operations_and_has_no_session_or_state() {
    let (server, _) = agent();
    let r=exec(&server,"surface","return {methods:Object.keys(app),session:typeof session,state:typeof state,host:typeof __host};");
    assert_eq!(
        *success(&r),
        json!({"methods":["look","capture","click","typeText","keypress","scroll","quit"],"session":"undefined","state":"undefined","host":"undefined"})
    );
}
#[test]
fn look_returns_baseline_then_small_delta_and_refreshes_after_click() {
    let (server, catalog) = agent();
    let first = exec(&server, "first", "await app.look();");
    assert!(!first.is_error, "{:?}", first.data);
    assert_eq!(first.data["value"]["view"]["reset"], true);
    assert_eq!(first.data["value"]["view"]["elements"][0]["id"], "w1.e1");
    let next = exec(&server, "next", "await app.look();");
    assert_eq!(next.data["value"]["view"], json!({"changed":false}));
    let action = exec(&server, "click", "await app.click({target:'w1.e1'});");
    assert!(!action.is_error, "{:?}", action.data);
    assert!(action.data["value"]["view"].is_object());
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
}
#[test]
fn intermediate_reads_do_not_hide_the_first_published_baseline() {
    let (server, catalog) = agent();
    let first = exec(
        &server,
        "read_and_click",
        "await app.look(); await app.click({target:'w1.e1'});",
    );
    assert!(!first.is_error, "{:?}", first.data);
    assert_eq!(first.data["value"]["view"]["reset"], true);
    assert!(first.data["value"]["view"]["windows"].is_array());
    assert!(first.data["value"]["view"]["elements"].is_array());
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    let next = exec(
        &server,
        "published_delta",
        "await app.look(); await app.look();",
    );
    assert_eq!(next.data["value"]["view"], json!({"changed":false}));
}
#[test]
fn invalid_and_unbound_coordinate_input_never_dispatch_and_keeps_rejection() {
    let (server, catalog) = agent();
    exec(&server, "look", "await app.look();");
    let r = exec(&server, "no_capture", "await app.click({x:1,y:2});");
    assert!(r.is_error);
    assert_eq!(r.data["script"]["failure"]["code"], "capture_required");
    assert!(r.data["value"]["view"].is_object());
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
    let r = exec(
        &server,
        "mixed",
        "await app.click({target:'w1.e1',x:1,y:2});",
    );
    assert!(r.is_error);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
}
#[test]
fn compact_scene_output_has_no_native_refs_or_session_ledger() {
    let (server, _) = agent();
    let r = exec(&server, "compact", "await app.look(); print('ready');");
    let r = super::super::projection::execution(r);
    assert!(r.data["view"].is_object());
    assert!(r.data.get("script").is_none());
    for word in ["window_ref", "observation_ref", "element_ref", "session_id"] {
        assert!(!r.data.to_string().contains(word));
    }
    assert!(r.data["output"].is_array());
}
#[test]
fn application_search_is_bounded_and_does_not_connect() {
    let (server, catalog) = agent();
    let r = call(
        &server,
        "apps",
        json!({"query":"controlled","state":"running","limit":1}),
    );
    assert_eq!(r.data["apps"].as_array().unwrap().len(), 1);
    assert_eq!(r.data["apps"][0]["running"], true);
    let r = call(&server, "apps", json!({"query":"not-present"}));
    assert_eq!(r.data["apps"], json!([]));
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 0);
    for args in [
        json!({"state":"unknown"}),
        json!({"limit":0}),
        json!({"query":123}),
        json!({"offset":-1}),
    ] {
        assert!(call(&server, "apps", args).is_error);
    }
}
