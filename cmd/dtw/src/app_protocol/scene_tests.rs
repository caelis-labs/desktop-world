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
        assert_eq!(reply.data["error"]["code"], "invalid_arguments");
    }
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 0);
}
#[test]
fn agent_sees_only_app_operations_and_has_no_session_or_state() {
    let (server, _) = agent();
    let r=exec(&server,"surface","return {methods:Object.keys(app),session:typeof session,state:typeof state,host:typeof __host};");
    assert_eq!(
        *success(&r),
        json!({"methods":["look","capture","click","move","drag","typeText","keypress","scroll","quit"],"session":"undefined","state":"undefined","host":"undefined"})
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
    let r = crate::app_protocol::projection::execution(r);
    assert!(r.data["view"].is_object());
    assert!(r.data.get("script").is_none());
    for word in ["window_ref", "observation_ref", "element_ref", "session_id"] {
        assert!(!r.data.to_string().contains(word));
    }
    assert!(r.data["output"].is_array());
}
#[test]
fn app_actions_hide_delivery_routing_but_retain_dispatch_and_private_evidence() {
    let (server, catalog) = agent();
    catalog.backend.0.model.lock().unwrap().outcome.delivery = Some(InputDelivery {
        route: Some(InputRoute::Foreground),
        reason: Some("classified_internal_capability".into()),
        foreground_borrowed: true,
        focus_return: Some(FocusReturn::Retained),
    });
    let response = exec(
        &server,
        "routing_boundary",
        "await app.look(); return await app.click({target:'w1.e1'});",
    );
    assert!(!response.is_error, "{}", response.data);
    let returned = &response.data["value"]["result"];
    assert_eq!(returned["outcome"]["dispatch"], "dispatched");
    assert!(returned["outcome"].get("delivery").is_none());
    assert!(returned["provider_outcome"].get("delivery").is_none());
    let steps = response.data["script"]["steps"].as_array().unwrap();
    assert!(steps
        .iter()
        .any(|step| step["receipt"]["outcome"]["delivery"]["reason"]
            == "classified_internal_capability"));
    let compact = crate::app_protocol::projection::execution(response);
    assert!(!compact
        .data
        .to_string()
        .contains("classified_internal_capability"));
    assert_eq!(compact.data["effects"]["dispatch"]["dispatched"], 1);
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

#[test]
fn direct_return_of_view_is_not_duplicated_and_unchanged_output_is_bounded() {
    let (server, catalog) = agent();
    catalog.backend.0.model.lock().unwrap().inventory = Some(test_inventory(vec![WindowInfo {
        target: WindowTarget {
            app: application(),
            native_id: 101,
            generation: 3,
        },
        title: "Window with physical bounds".into(),
        visible: true,
        scene: WindowLayout::new([16.0, 16.0, 416.0, 469.0], 0),
    }]));
    let first = crate::app_protocol::projection::execution(exec(
        &server,
        "returned_view",
        "return await app.look();",
    ));
    assert!(first.data["view"]["reset"] == true);
    assert_eq!(
        first.data["view"]["windows"][0]["bounds"],
        json!([16.0, 16.0, 416.0, 469.0])
    );
    assert!(first.data.get("result").is_none(), "{:?}", first.data);
    let unchanged = crate::app_protocol::projection::execution(exec(
        &server,
        "small_delta",
        "return await app.look();",
    ));
    assert_eq!(unchanged.data["view"], json!({"changed":false}));
    assert!(
        unchanged.data.to_string().len() <= 512,
        "{:?}",
        unchanged.data
    );
    let custom = crate::app_protocol::projection::execution(exec(
        &server,
        "task_output",
        "print('checked');return {count:3};",
    ));
    assert_eq!(custom.data["result"], json!({"count":3}));
    assert_eq!(custom.data["output"], json!(["checked"]));
}

#[test]
fn public_print_budget_keeps_its_truncation_signal() {
    let (server, _) = agent();
    let output = crate::app_protocol::projection::execution(exec(
        &server,
        "bounded_print",
        "for(let i=0;i<300;i++)print('row',i);",
    ));
    assert!(!output.is_error, "{:?}", output.data);
    assert!(output.data["output"].as_array().unwrap().len() <= 200);
    assert_eq!(output.data["output_truncated"], true);
}

#[test]
fn script_observations_are_snapshots_while_tool_messages_stay_incremental() {
    let (server, _) = agent();
    let first = exec(&server, "baseline", "await app.look();");
    assert_eq!(first.data["value"]["view"]["reset"], true);
    let second=exec(&server,"snapshot","const a=await app.look(); const b=await app.look(); return {windows:a.windows.length, elements:b.elements.length, hasBounds:'bounds' in a.windows[0]};");
    assert_eq!(second.data["value"]["view"], json!({"changed":false}));
    assert_eq!(
        *success(&second),
        json!({"windows":2,"elements":1,"hasBounds":true})
    );
    let returned = exec(&server, "coalesced", "return await app.look();");
    let projected = crate::app_protocol::projection::execution(returned).mcp();
    let text = projected["content"][0]["text"].as_str().unwrap();
    let body: Value = serde_json::from_str(text).unwrap();
    assert_eq!(body["view"], json!({"changed":false}));
    assert!(body.get("result").is_none(), "{body}");
}
#[test]
fn capture_snapshot_exposes_window_metadata_even_when_png_cannot_be_decoded() {
    let (server, _) = agent();
    let r=exec(&server,"metadata","const view=await app.capture(); return {windows:view.windows.length, hasBounds:'bounds' in view.windows[0], imageError:typeof view.image.error.code};");
    assert!(!r.is_error, "{:?}", r.data);
    assert_eq!(
        *success(&r),
        json!({"windows":2,"hasBounds":true,"imageError":"string"})
    );
}

#[test]
fn default_scroll_uses_an_internally_captured_pixel_anchor() {
    let (server, catalog) = agent();
    exec(&server, "scroll_baseline", "await app.look();");
    let reply = exec(&server, "center_scroll", "await app.scroll({delta_y:50});");
    assert!(!reply.is_error, "{}", reply.data);
    let anchors = catalog.backend.0.anchored_scrolls.lock().unwrap();
    assert_eq!(anchors.len(), 1);
    assert_eq!(anchors[0].1.delta_y, 50);
    assert_eq!(
        anchors[0].1.anchor,
        ScrollAnchor::Pixels(PixelPoint { x: 10.0, y: 5.0 })
    );
}

#[test]
fn native_identity_survives_insertion_but_removed_targets_never_retarget() {
    let (server, catalog) = agent();
    let first = exec(&server, "first_identity", "await app.look();");
    let old = first.data["value"]["view"]["elements"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let original = Element {
        id: ElementId(9007199254740993),
        identity: Some("mock:9007199254740993".into()),
        label: "Increment".into(),
        name: Some("Increment".into()),
        role: "button".into(),
        value: None,
        enabled: Some(true),
        actions: vec!["press".into()],
        can_press: true,
    };
    let mut added = original.clone();
    added.id = ElementId(17);
    added.identity = Some("native-added".into());
    added.name = Some("Before".into());
    catalog.backend.0.model.lock().unwrap().semantic_elements =
        Some(vec![added.clone(), original.clone()]);
    let next = exec(&server, "insert", "await app.look({full:true});");
    let rows = next.data["value"]["view"]["elements"].as_array().unwrap();
    assert_eq!(
        rows.iter().find(|e| e["name"] == "Increment").unwrap()["id"],
        old
    );
    assert_ne!(rows[0]["id"], old);
    catalog.backend.0.model.lock().unwrap().semantic_elements = Some(vec![added.clone()]);
    exec(&server, "remove", "await app.look();");
    let stale = exec(
        &server,
        "old_target",
        &format!("await app.click({{target:{}}});", json!(old)),
    );
    assert!(stale.is_error);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
    catalog.backend.0.model.lock().unwrap().semantic_elements = Some(vec![original]);
    let reappeared = exec(&server, "reappeared", "await app.look({full:true});");
    assert_ne!(reappeared.data["value"]["view"]["elements"][0]["id"], old);
    added.identity = None;
    catalog.backend.0.model.lock().unwrap().semantic_elements = Some(vec![added]);
    let a = exec(&server, "unidentified_a", "await app.look({full:true});");
    let b = exec(&server, "unidentified_b", "await app.look({full:true});");
    assert_ne!(
        a.data["value"]["view"]["elements"][0]["id"],
        b.data["value"]["view"]["elements"][0]["id"]
    );
}
