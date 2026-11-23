//! Real MCP transport and QuickJS, controlled native backend; no desktop evidence.
use super::*;

fn script_server() -> (Arc<Server<Catalog>>, Catalog) {
    let (_, catalog) = server();
    (Server::new(catalog.clone()), catalog)
}

#[tokio::test]
async fn sdk_public_tools_are_enforced_before_provider_connection() {
    let (server, catalog) = script_server();
    let mut client = Client::new(server).await;
    let listing = client.rpc("tools/list", json!({})).await;
    let tools = listing["result"]["tools"].as_array().unwrap();
    assert_eq!(
        tools
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["apps", "exec"]
    );
    for name in [
        "look",
        "capture",
        "readText",
        "wait",
        "waitForNewWindow",
        "click",
        "setValue",
        "move",
        "drag",
        "typeText",
        "keypress",
        "scroll",
        "focus",
        "quit",
        "read_text",
    ] {
        let rejected = decoded(&client.tool(name, json!({"app":APP})).await);
        assert_eq!(
            rejected["error"]["code"], "invalid_arguments",
            "{name}: {rejected}"
        );
    }
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 0);
    client.close().await;
}

#[tokio::test]
async fn sdk_exec_shares_scene_context_across_calls_and_reads_back_actions() {
    let (server, catalog) = script_server();
    let engine_entries = server.watch_script_engine();
    let mut client = Client::new(server).await;
    let first = decoded(&script_call(&mut client, "look", json!({})).await);
    assert_eq!(first["view"]["kind"], "full", "{first}");
    let second = decoded(&script_call(&mut client, "look", json!({})).await);
    assert_eq!(second["view"]["kind"], "unchanged", "{second}");
    let action = script_call(&mut client, "click", json!({"target":"w1.e1"})).await;
    assert_eq!(action["isError"], false, "{action}");
    let action = decoded(&action);
    assert_eq!(action["effects"]["dispatch"]["dispatched"], 1, "{action}");
    assert!(action["view"].is_object(), "{action}");
    let fourth = script_call(&mut client, "look", json!({})).await;
    assert_eq!(fourth["isError"], false, "{fourth}");
    assert_eq!(decoded(&fourth)["view"]["kind"], "unchanged", "{fourth}");
    assert_eq!(engine_entries.load(Ordering::SeqCst), 4);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 1);
    assert_eq!(catalog.backend.0.finishes.load(Ordering::SeqCst), 4);
    client.close().await;
    // Server::close retires contexts before and after waiting for active work.
    assert_eq!(catalog.backend.0.shutdowns.load(Ordering::SeqCst), 2);
}

fn script_params(method: &str, args: Value) -> Value {
    let prefix = if method == "readText" {
        "return await"
    } else {
        "await"
    };
    json!({"name":"exec","arguments":input(&format!("{prefix} app.{method}({args});"))})
}

async fn script_call(client: &mut Client, method: &str, args: Value) -> Value {
    let params = script_params(method, args);
    client.tool("exec", params["arguments"].clone()).await
}

#[tokio::test]
async fn sdk_exec_cancel_settles_unknown_receipt_before_recovery() {
    let (_, catalog) = script_server();
    catalog.backend.0.model.lock().unwrap().poll_action = true;
    let factory = CleanupCatalog {
        inner: catalog.clone(),
        close_gate: Arc::default(),
    };
    let server = Server::new(factory.clone());
    let engine_entries = server.watch_script_engine();
    let mut client = Client::new(server).await;
    script_call(&mut client, "look", json!({})).await;
    let action = catalog.backend.action_gate();
    let params = script_params("click", json!({"target":"w1.e1"}));
    let id = client.begin("tools/call", params.clone()).await;
    let action = reached(action).await;
    let running = decoded(&client.receipt(json!({"app":APP})).await);
    client
        .send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":params}))
        .await;
    let (gate, cleanup) = TestGate::pair();
    *factory.close_gate.lock().unwrap() = Some(gate);
    client
        .send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id}}))
        .await;
    let cleanup = reached(cleanup).await;
    let blocked = decoded(&script_call(&mut client, "look", json!({})).await);
    assert_eq!(blocked["error"]["code"], "busy", "{blocked}");
    assert_eq!(engine_entries.load(Ordering::SeqCst), 2);
    let unsettled = decoded(&client.receipt(json!({"app":APP})).await);
    assert_eq!(unsettled["status"], "running", "{unsettled}");
    cleanup.release();
    action.release();
    let settled = tokio::time::timeout(RESPONSE_TIMEOUT, async {
        loop {
            let current = decoded(&client.receipt(json!({"app":APP})).await);
            if current["status"] == "completed" {
                break current;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("exec cancellation did not settle");
    assert_eq!(settled["execution_id"], running["execution_id"]);
    assert!(settled["error"].is_object(), "{settled}");
    let projected = crate::app_protocol::projection::execution(Reply::plain(settled));
    assert_eq!(
        projected.data["effects"]["dispatch"]["unknown"], 1,
        "{}",
        projected.data
    );
    client
        .send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":params}))
        .await;
    let retained = client.reply(id).await;
    assert_eq!(
        decoded(&retained["result"])["execution_id"],
        running["execution_id"]
    );
    assert_eq!(engine_entries.load(Ordering::SeqCst), 2);
    let stale = script_call(&mut client, "click", json!({"target":"w1.e1"})).await;
    assert_eq!(stale["isError"], true, "{stale}");
    let recovered = script_call(&mut client, "look", json!({})).await;
    assert_eq!(recovered["isError"], false, "{recovered}");
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 2);
    assert_eq!(engine_entries.load(Ordering::SeqCst), 4);
    client.close().await;
}

fn native_field(id: u64, name: &str) -> Element {
    Element {
        id: ElementId(id),
        identity: Some(format!("mock:{id}")),
        rebind_identity: Some(format!("retained:{id}")),
        label: name.into(),
        role: "text_field".into(),
        name: Some(name.into()),
        value: Some(name.into()),
        enabled: Some(true),
        actions: vec!["setValue".into()],
        focused: false,
        can_press: false,
    }
}

#[tokio::test]
async fn sdk_exec_cancel_after_dispatch_preserves_cancelled_completion() {
    let (_, catalog) = script_server();
    let factory = CleanupCatalog {
        inner: catalog.clone(),
        close_gate: Arc::default(),
    };
    let server = Server::new(factory.clone());
    let engine_entries = server.watch_script_engine();
    let mut client = Client::new(server).await;
    script_call(&mut client, "look", json!({})).await;
    let finish = catalog.backend.finish_gate();
    let params = script_params("click", json!({"target":"w1.e1"}));
    let id = client.begin("tools/call", params.clone()).await;
    let finish = reached(finish).await;
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    let (gate, cleanup) = TestGate::pair();
    *factory.close_gate.lock().unwrap() = Some(gate);
    client
        .send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id}}))
        .await;
    // Catalog cleanup follows owner.cancel: this gate proves cancellation won
    // while finish_execution still holds the dispatched operation open.
    let cleanup = reached(cleanup).await;
    finish.release();
    let unsettled = tokio::time::timeout(RESPONSE_TIMEOUT, async {
        loop {
            let receipt = decoded(&client.receipt(json!({"app":APP})).await);
            if receipt["error"]["code"] == "cancelled" {
                break receipt;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("cancelled native completion was not published");
    assert_eq!(unsettled["status"], "running", "{unsettled}");
    let blocked = decoded(&script_call(&mut client, "look", json!({})).await);
    assert_eq!(blocked["error"]["code"], "busy", "{blocked}");
    cleanup.release();
    let settled = tokio::time::timeout(RESPONSE_TIMEOUT, async {
        loop {
            let receipt = decoded(&client.receipt(json!({"app":APP})).await);
            if receipt["status"] == "completed" {
                break receipt;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("cancelled native cleanup did not settle");
    assert_eq!(settled["execution_id"], unsettled["execution_id"]);
    assert_eq!(settled["error"]["code"], "cancelled", "{settled}");
    let projected = crate::app_protocol::projection::execution(Reply::plain(settled.clone()));
    assert_eq!(
        projected.data["effects"]["dispatch"]["dispatched"], 1,
        "{}",
        projected.data
    );
    client
        .send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":params}))
        .await;
    let retained = decoded(&client.reply(id).await["result"]);
    assert_eq!(retained["execution_id"], settled["execution_id"]);
    assert_eq!(retained["error"]["code"], "cancelled", "{retained}");
    assert_eq!(
        retained["effects"]["dispatch"]["dispatched"], 1,
        "{retained}"
    );
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(engine_entries.load(Ordering::SeqCst), 2);
    client.close().await;
}

#[tokio::test]
async fn sdk_public_method_enumeration_matches_app_contract() {
    let (script, _) = script_server();
    let mut script = Client::new(script).await;
    let methods = decoded(
        &script
            .tool("exec", input("return Object.keys(app).sort();"))
            .await,
    );
    let mut expected = vec![
        "look",
        "capture",
        "readText",
        "wait",
        "waitForNewWindow",
        "click",
        "setValue",
        "move",
        "drag",
        "typeText",
        "keypress",
        "scroll",
        "focus",
        "quit",
    ];
    expected.sort_unstable();
    assert_eq!(methods["result"], json!(expected));
    script.close().await;
}

fn object_argument_fixture(catalog: &Catalog) {
    let mut field = native_field(77, "Native note");
    field.value = Some("A🙂B".into());
    let mut button = native_field(78, "Press me");
    button.role = "button".into();
    button.can_press = true;
    button.actions = vec!["press".into()];
    button.value = None;
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, 20, 10);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&vec![255; 20 * 10 * 4])
            .unwrap();
    }
    let mut model = catalog.backend.0.model.lock().unwrap();
    model.semantic_elements = Some(vec![field, button]);
    model.filtered_observation = true;
    model.subtree_scope = true;
    model.capture_png = Some(png);
    model.inventory = Some(test_inventory(vec![WindowInfo {
        target: WindowTarget {
            app: application(),
            native_id: 101,
            generation: 3,
        },
        title: "Owned argument fixture".into(),
        visible: true,
        scene: WindowLayout::new([0., 0., 20., 10.], 0),
    }]));
}

#[tokio::test]
async fn sdk_canonical_object_arguments_return_results_and_dispatch_expected_input() {
    for (method, args, actions) in [
        (
            "look",
            json!({"full":true,"role":"text_field","text":"Native note","scope":"window"}),
            0,
        ),
        ("look", json!({"target":"w1.e1"}), 0),
        ("capture", json!({"full":true}), 0),
        (
            "readText",
            json!({"target":"w1.e1","offset":0,"limit":3}),
            0,
        ),
        (
            "readText",
            json!({"target":"w1.e1","offset":3,"limit":2}),
            0,
        ),
        (
            "wait",
            json!({"text":"Native note","role":"text_field","timeout_ms":1000}),
            0,
        ),
        // Integral floating-point JSON values remain valid script arguments.
        ("wait", serde_json::from_str(r#"{"ms":1.0}"#).unwrap(), 0),
        (
            "wait",
            serde_json::from_str(r#"{"text":"Native note","timeout_ms":1e3}"#).unwrap(),
            0,
        ),
        (
            "readText",
            serde_json::from_str(r#"{"target":"w1.e1","offset":0.0,"limit":3e0}"#).unwrap(),
            0,
        ),
        (
            "readText",
            serde_json::from_str(r#"{"target":"w1.e1","offset":3e0,"limit":2.0}"#).unwrap(),
            0,
        ),
        (
            "click",
            serde_json::from_str(r#"{"x":5,"y":4,"count":2.0}"#).unwrap(),
            1,
        ),
        (
            "scroll",
            serde_json::from_str(r#"{"delta_y":-1.0,"delta_x":1e0}"#).unwrap(),
            1,
        ),
        ("keypress", json!({"keys":["META","A"]}), 1),
        (
            "click",
            json!({"selector":{"role":"button","text":"Press me","scope":"window"}}),
            1,
        ),
        ("typeText", json!({"text":"中文🙂 \\"}), 1),
        (
            "setValue",
            json!({"selector":{"role":"text_field","text":"Native note","scope":"window"},"value":"fresh 中文🙂"}),
            1,
        ),
    ] {
        let (script, script_catalog) = script_server();
        object_argument_fixture(&script_catalog);
        let script_entries = script.watch_script_engine();
        let mut script = Client::new(script).await;
        let seed_method = if method == "click" && args.get("x").is_some() {
            "capture"
        } else {
            "look"
        };
        let seed = script_call(&mut script, seed_method, json!({})).await;
        assert_eq!(seed["isError"], false, "{seed}");
        let script_reply = script_call(&mut script, method, args.clone()).await;
        assert_eq!(
            script_reply["isError"], false,
            "{method} {args}: {script_reply}"
        );
        let script_result = decoded(&script_reply);
        if method == "readText" {
            assert_eq!(
                script_result["result"]["text"],
                if args["offset"].as_f64() == Some(0.0) {
                    "A🙂"
                } else {
                    "B"
                }
            );
        }
        if method == "capture" {
            assert!(script_reply["content"]
                .as_array()
                .unwrap()
                .iter()
                .any(|block| block["type"] == "image" && block["mimeType"] == "image/png"));
        }
        assert_eq!(
            script_catalog.backend.0.actions.load(Ordering::SeqCst),
            actions,
            "{method} {args}"
        );
        if method == "setValue" {
            assert_eq!(
                *script_catalog.backend.0.set_values.lock().unwrap(),
                vec![(ElementId(77), "fresh 中文🙂".into())]
            );
            assert!(script_catalog
                .backend
                .0
                .input_targets
                .lock()
                .unwrap()
                .is_empty());
        }
        assert_eq!(script_entries.load(Ordering::SeqCst), 2);
        script.close().await;
    }
}

#[tokio::test]
async fn sdk_malformed_object_arguments_return_business_errors_without_backend_input() {
    for (method, args) in [
        ("look", json!({"full":1})),
        ("look", json!({"text":false})),
        ("look", json!({"target":"w1.e1","scope":"window"})),
        ("capture", json!({"text":"extra"})),
        ("readText", json!({"target":"w1.e1","limit":1})),
        ("readText", json!({"target":"w1.e1","offset":null})),
        ("wait", json!({"ms":1,"text":"Native note"})),
        ("wait", json!({"timeout_ms":1})),
        ("wait", json!({"ms":1.5})),
        ("wait", json!({"text":"Native note","timeout_ms":1.5})),
        ("readText", json!({"target":"w1.e1","offset":0.5,"limit":3})),
        ("readText", json!({"target":"w1.e1","offset":0,"limit":3.5})),
        ("click", json!({"x":5,"y":4,"count":1.5})),
        ("scroll", json!({"delta_y":-1.5})),
        ("scroll", json!({"delta_y":-1,"delta_x":0.5})),
        ("keypress", json!({"keys":"META+A"})),
        ("keypress", json!({"keys":["META",3]})),
        (
            "click",
            json!({"selector":{"text":"Native note","extra":true}}),
        ),
        (
            "click",
            json!({"selector":{"text":"Native note"},"target":"w1.e1"}),
        ),
        ("setValue", json!({"selector":{"role":" "},"value":"x"})),
        ("typeText", json!({"text":{"text":"nested"}})),
        (
            "drag",
            json!({"path":[{"x":1,"y":1},{"x":2,"y":2,"extra":true}]}),
        ),
    ] {
        // Fresh contexts isolate each validation case from prior cleanup.
        let (server, catalog) = script_server();
        object_argument_fixture(&catalog);
        let mut client = Client::new(server).await;
        let reply = script_call(&mut client, method, args.clone()).await;
        assert_eq!(reply["isError"], true, "{method} {args}: {reply}");
        let result = decoded(&reply);
        assert_eq!(
            result["error"]["code"], "invalid_arguments",
            "{method} {args}: {result}"
        );
        assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
        assert!(catalog.backend.0.input_targets.lock().unwrap().is_empty());
        assert!(catalog.backend.0.press_targets.lock().unwrap().is_empty());
        assert!(catalog.backend.0.set_values.lock().unwrap().is_empty());
        client.close().await;
    }
}
