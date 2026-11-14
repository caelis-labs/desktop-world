//! Real MCP transport and QuickJS, controlled native backend; no desktop evidence.
use super::*;
use crate::app_protocol::toolset::Toolset;

fn selected(mode: Toolset) -> (Arc<Server<Catalog>>, Catalog) {
    let (_, catalog) = server();
    (Server::with_toolset(catalog.clone(), mode), catalog)
}

#[tokio::test]
async fn sdk_toolset_selection_is_enforced_before_provider_connection() {
    for (mode, count, disabled) in [(Toolset::Script, 2, "look"), (Toolset::Atomic, 15, "exec")] {
        let (server, catalog) = selected(mode);
        let mut client = Client::new(server).await;
        let listing = client.rpc("tools/list", json!({})).await;
        let tools = listing["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), count);
        for tool in tools.iter().filter(|tool| tool["name"] != "apps") {
            assert!(tool["inputSchema"]["required"]
                .as_array()
                .unwrap()
                .contains(&json!("app")));
            assert_eq!(tool["inputSchema"]["additionalProperties"], false);
            assert_eq!(tool["annotations"]["readOnlyHint"], false);
        }
        let rejected = decoded(&client.tool(disabled, input("return 1;")).await);
        assert_eq!(rejected["error"]["code"], "invalid_arguments");
        if mode == Toolset::Atomic {
            for alias in ["read_text", "wait_for_new_window", "set_value", "type_text"] {
                let rejected = decoded(&client.tool(alias, json!({"app":APP})).await);
                assert_eq!(
                    rejected["error"]["code"], "invalid_arguments",
                    "{alias}: {rejected}"
                );
            }
            for (args, code) in [
                (json!({}), "invalid_arguments"),
                (json!({"app":42}), "invalid_app"),
                (
                    json!({"app":APP,"source":"await app.quit();"}),
                    "invalid_arguments",
                ),
                (json!({"app":APP,"full":null}), "invalid_arguments"),
            ] {
                let rejected = client.tool("look", args).await;
                assert_eq!(rejected["isError"], true, "{rejected}");
                assert_eq!(decoded(&rejected)["error"]["code"], code);
            }
            let unavailable = decoded(
                &client
                    .tool("look", json!({"app":"org.example.Unavailable"}))
                    .await,
            );
            assert_eq!(unavailable["error"]["code"], "app_unavailable");
        }
        assert_eq!(catalog.connects.load(Ordering::SeqCst), 0);
        client.close().await;
    }
}

#[tokio::test]
async fn sdk_selected_modes_share_scene_context_across_calls_and_read_back_actions() {
    for mode in [Toolset::Script, Toolset::Atomic] {
        let (server, catalog) = selected(mode);
        let engine_entries = server.watch_script_engine();
        let mut client = Client::new(server).await;
        let first = decoded(&mode_call(&mut client, mode, "look", json!({})).await);
        assert_eq!(first["view"]["kind"], "full", "{first}");
        let second = decoded(&mode_call(&mut client, mode, "look", json!({})).await);
        assert_eq!(second["view"]["kind"], "unchanged", "{second}");
        let action = mode_call(&mut client, mode, "click", json!({"target":"w1.e1"})).await;
        assert_eq!(action["isError"], false, "{action}");
        let action = decoded(&action);
        assert_eq!(action["effects"]["dispatch"]["dispatched"], 1, "{action}");
        assert!(action["view"].is_object(), "{action}");
        let fourth = mode_call(&mut client, mode, "look", json!({})).await;
        assert_eq!(fourth["isError"], false, "{fourth}");
        assert_eq!(decoded(&fourth)["view"]["kind"], "unchanged", "{fourth}");
        assert_eq!(
            engine_entries.load(Ordering::SeqCst),
            if mode == Toolset::Script { 4 } else { 0 }
        );
        assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
        assert_eq!(catalog.connects.load(Ordering::SeqCst), 1);
        assert_eq!(catalog.backend.0.finishes.load(Ordering::SeqCst), 4);
        client.close().await;
        // Server::close retires contexts before and after waiting for active work.
        assert_eq!(catalog.backend.0.shutdowns.load(Ordering::SeqCst), 2);
    }
}

fn mode_params(mode: Toolset, method: &str, args: Value) -> Value {
    if mode == Toolset::Script {
        let prefix = if method == "readText" {
            "return await"
        } else {
            "await"
        };
        json!({"name":"exec","arguments":input(&format!("{prefix} app.{method}({args});"))})
    } else {
        let mut args = args.as_object().unwrap().clone();
        args.insert("app".into(), json!(APP));
        json!({"name":method,"arguments":args})
    }
}

async fn mode_call(client: &mut Client, mode: Toolset, method: &str, args: Value) -> Value {
    let params = mode_params(mode, method, args);
    client
        .tool(
            params["name"].as_str().unwrap(),
            params["arguments"].clone(),
        )
        .await
}

#[tokio::test]
async fn sdk_atomic_json_is_data_and_native_setter_receives_exact_value() {
    let (server, catalog) = selected(Toolset::Atomic);
    let field = Element {
        id: ElementId(77),
        rebind_identity: Some("retained:77".into()),
        identity: Some("mock:77".into()),
        label: "Owned field".into(),
        focused: false,
        can_press: false,
        role: "text_field".into(),
        name: Some("Owned field".into()),
        value: Some("A🙂B".into()),
        enabled: Some(true),
        actions: vec!["setValue".into()],
    };
    catalog.backend.0.model.lock().unwrap().semantic_elements = Some(vec![field]);
    let mut client = Client::new(server).await;
    client.tool("look", json!({"app":APP})).await;
    let page = decoded(
        &client
            .tool("readText", json!({"app":APP,"target":"w1.e1","limit":3}))
            .await,
    );
    assert_eq!(page["result"]["text"], "A🙂", "{page}");
    assert_eq!(page["result"]["next_offset"], 3);
    let end = decoded(
        &client
            .tool("readText", json!({"app":APP,"target":"w1.e1","offset":3}))
            .await,
    );
    assert_eq!(end["result"]["text"], "B", "{end}");
    assert!(end["result"]["next_offset"].is_null());
    let value = "\"}); await app.quit(); //\n中文🙂 \\ ${app.quit()}\u{2028}";
    let result = client
        .tool(
            "setValue",
            json!({"app":APP,"target":"w1.e1","value":value}),
        )
        .await;
    assert_eq!(result["isError"], false, "{result}");
    let writes = catalog.backend.0.set_values.lock().unwrap().clone();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].1, value);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    client.close().await;
}

#[tokio::test]
async fn sdk_atomic_rpc_identity_includes_tool_name_and_never_replays_input() {
    let (server, catalog) = selected(Toolset::Atomic);
    let engine_entries = server.watch_script_engine();
    let mut client = Client::new(server).await;
    client.tool("look", json!({"app":APP})).await;
    let params = json!({"name":"click","arguments":{"app":APP,"target":"w1.e1"}});
    let id = client.begin("tools/call", params.clone()).await;
    let original = client.reply(id.clone()).await;
    assert_eq!(original["result"]["isError"], false, "{original}");
    client
        .send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":params}))
        .await;
    let retained = client.reply(id.clone()).await;
    assert_eq!(retained, original);
    client.send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"look","arguments":{"app":APP,"target":"w1.e1"}}})).await;
    let conflict = client.reply(id).await;
    assert_eq!(
        decoded(&conflict["result"])["error"]["code"],
        "request_conflict"
    );
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(engine_entries.load(Ordering::SeqCst), 0);
    client.close().await;
}

#[tokio::test]
async fn sdk_atomic_capture_preserves_png_content_and_compact_view() {
    use base64::Engine;
    let (server, catalog) = selected(Toolset::Atomic);
    let mut image = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut image, 20, 10);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&vec![255; 20 * 10 * 4])
            .unwrap();
    }
    {
        let mut model = catalog.backend.0.model.lock().unwrap();
        model.capture_png = Some(image);
        model.inventory = Some(test_inventory(vec![WindowInfo {
            target: WindowTarget {
                app: application(),
                native_id: 101,
                generation: 3,
            },
            title: "Owned capture fixture".into(),
            visible: true,
            scene: WindowLayout::new([0., 0., 20., 10.], 0),
        }]));
    }
    let mut client = Client::new(server).await;
    let result = client.tool("capture", json!({"app":APP})).await;
    assert_eq!(result["isError"], false, "{result}");
    let data = decoded(&result);
    assert_eq!(data["view"]["kind"], "full", "{data}");
    let png = result["content"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["type"] == "image")
        .expect(&format!("PNG block missing: {result}"));
    assert_eq!(png["mimeType"], "image/png");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(png["data"].as_str().unwrap())
        .unwrap();
    assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    for private in ["window_ref", "observation_ref", "element_ref", "session_id"] {
        assert!(!data.to_string().contains(private));
    }
    let read = client.tool("look", json!({"app":APP})).await;
    assert_eq!(read["isError"], false, "{read}");
    let clicked = client.tool("click", json!({"app":APP,"x":5,"y":4})).await;
    assert_eq!(clicked["isError"], false, "{clicked}");
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    client.close().await;
}

#[tokio::test]
async fn sdk_atomic_cancel_settles_unknown_receipt_before_recovery() {
    for mode in [Toolset::Script, Toolset::Atomic] {
        let (_, catalog) = selected(mode);
        catalog.backend.0.model.lock().unwrap().poll_action = true;
        let factory = CleanupCatalog {
            inner: catalog.clone(),
            close_gate: Arc::default(),
        };
        let server = Server::with_toolset(factory.clone(), mode);
        let engine_entries = server.watch_script_engine();
        let mut client = Client::new(server).await;
        mode_call(&mut client, mode, "look", json!({})).await;
        let action = catalog.backend.action_gate();
        let params = mode_params(mode, "click", json!({"target":"w1.e1"}));
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
        let blocked = decoded(&mode_call(&mut client, mode, "look", json!({})).await);
        assert_eq!(blocked["error"]["code"], "busy", "{blocked}");
        assert_eq!(
            engine_entries.load(Ordering::SeqCst),
            if mode == Toolset::Script { 2 } else { 0 }
        );
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
        .expect("atomic cancellation did not settle");
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
        assert_eq!(
            engine_entries.load(Ordering::SeqCst),
            if mode == Toolset::Script { 2 } else { 0 }
        );
        let stale = mode_call(&mut client, mode, "click", json!({"target":"w1.e1"})).await;
        assert_eq!(stale["isError"], true, "{stale}");
        let recovered = mode_call(&mut client, mode, "look", json!({})).await;
        assert_eq!(recovered["isError"], false, "{recovered}");
        assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
        assert_eq!(catalog.connects.load(Ordering::SeqCst), 2);
        assert_eq!(
            engine_entries.load(Ordering::SeqCst),
            if mode == Toolset::Script { 4 } else { 0 }
        );
        client.close().await;
    }
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
async fn sdk_atomic_native_handles_survive_other_reads_but_reject_changed_targets() {
    for change in ["unchanged", "removed", "replaced", "renamed"] {
        let (server, catalog) = selected(Toolset::Atomic);
        let engine_entries = server.watch_script_engine();
        let mut field = native_field(77, "Native note");
        {
            let mut model = catalog.backend.0.model.lock().unwrap();
            model.filtered_observation = true;
            model.semantic_elements = Some(vec![field.clone(), native_field(78, "Other")]);
        }
        let mut client = Client::new(server).await;
        let first = decoded(&client.tool("look", json!({"app":APP})).await);
        let target = first["view"]["elements"][0]["id"].clone();
        assert!(target.is_string(), "{first}");
        let filtered = client.tool("look", json!({"app":APP,"text":"Other"})).await;
        assert_eq!(filtered["isError"], false, "{filtered}");
        let read = client
            .tool("readText", json!({"app":APP,"target":"w1.e2"}))
            .await;
        assert_eq!(read["isError"], false, "{read}");
        match change {
            "replaced" => field.rebind_identity = Some("different retained object".into()),
            "renamed" => field.name = Some("Different meaning".into()),
            _ => {}
        }
        catalog.backend.0.model.lock().unwrap().semantic_elements = Some(if change == "removed" {
            vec![]
        } else {
            vec![field, native_field(78, "Other")]
        });
        let write = client
            .tool(
                "setValue",
                json!({"app":APP,"target":target,"value":"fresh 中文 🌍"}),
            )
            .await;
        if change == "unchanged" {
            assert_eq!(write["isError"], false, "{write}");
            assert_eq!(
                *catalog.backend.0.set_values.lock().unwrap(),
                vec![(ElementId(77), "fresh 中文 🌍".into())]
            );
            assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
        } else {
            assert_eq!(write["isError"], true, "{change}: {write}");
            assert_eq!(
                decoded(&write)["error"]["code"],
                "target_changed",
                "{change}: {write}"
            );
            assert!(catalog.backend.0.set_values.lock().unwrap().is_empty());
            assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
        }
        assert_eq!(engine_entries.load(Ordering::SeqCst), 0);
        client.close().await;
    }
}

#[tokio::test]
async fn sdk_atomic_validates_native_text_field_limits() {
    let (server, catalog) = selected(Toolset::Atomic);
    let engine_entries = server.watch_script_engine();
    let mut client = Client::new(server).await;
    client.tool("look", json!({"app":APP})).await;
    let text = "\\".repeat(16 * 1024);
    let result = client
        .tool("typeText", json!({"app":APP,"text":text}))
        .await;
    assert_eq!(result["isError"], false, "{result}");
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    let oversized = client
        .tool(
            "typeText",
            json!({"app":APP,"text":"x".repeat(16 * 1024 + 1)}),
        )
        .await;
    assert_eq!(oversized["isError"], true, "{oversized}");
    assert_eq!(
        decoded(&oversized)["error"]["code"],
        "invalid_arguments",
        "{oversized}"
    );
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(engine_entries.load(Ordering::SeqCst), 0);
    client.close().await;
}

#[tokio::test]
async fn sdk_atomic_cleanup_failure_remains_explicit() {
    let (server, catalog) = selected(Toolset::Atomic);
    let engine_entries = server.watch_script_engine();
    let mut client = Client::new(server).await;
    client.tool("look", json!({"app":APP})).await;
    catalog.backend.0.model.lock().unwrap().finish_error = Some(Error::NativeFailure {
        code: "scene_cleanup_failed".into(),
        message: "controlled atomic cleanup failure".into(),
    });
    let result = client
        .tool("click", json!({"app":APP,"target":"w1.e1"}))
        .await;
    assert_eq!(result["isError"], true, "{result}");
    let result = decoded(&result);
    assert_eq!(
        result["shutdown_error"]["code"], "scene_cleanup_failed",
        "{result}"
    );
    assert_eq!(
        result["shutdown_error"]["message"],
        "controlled atomic cleanup failure"
    );
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(engine_entries.load(Ordering::SeqCst), 0);
    client.input.shutdown().await.unwrap();
    let shutdown = tokio::time::timeout(RESPONSE_TIMEOUT, client.service)
        .await
        .expect("MCP service did not close")
        .expect("MCP service panicked")
        .expect_err("cleanup failure must remain explicit at shutdown");
    assert_eq!(shutdown.code, "shutdown_failed");
    assert_eq!(
        shutdown.details.unwrap()["errors"][0]["code"],
        "scene_cleanup_failed"
    );
}

#[tokio::test]
async fn sdk_atomic_cancel_after_dispatch_preserves_cancelled_completion() {
    for mode in [Toolset::Script, Toolset::Atomic] {
        let (_, catalog) = selected(mode);
        let factory = CleanupCatalog {
            inner: catalog.clone(),
            close_gate: Arc::default(),
        };
        let server = Server::with_toolset(factory.clone(), mode);
        let engine_entries = server.watch_script_engine();
        let mut client = Client::new(server).await;
        mode_call(&mut client, mode, "look", json!({})).await;
        let finish = catalog.backend.finish_gate();
        let params = mode_params(mode, "click", json!({"target":"w1.e1"}));
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
        let blocked = decoded(&mode_call(&mut client, mode, "look", json!({})).await);
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
        assert_eq!(
            engine_entries.load(Ordering::SeqCst),
            if mode == Toolset::Script { 2 } else { 0 }
        );
        client.close().await;
    }
}

#[tokio::test]
async fn sdk_public_method_enumeration_matches_atomic_tools() {
    let (script, _) = selected(Toolset::Script);
    let (atomic, _) = selected(Toolset::Atomic);
    let mut script = Client::new(script).await;
    let mut atomic = Client::new(atomic).await;
    let methods = decoded(
        &script
            .tool("exec", input("return Object.keys(app).sort();"))
            .await,
    );
    let listing = atomic.rpc("tools/list", json!({})).await;
    let mut names: Vec<_> = listing["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .filter(|name| *name != "apps")
        .collect();
    names.sort_unstable();
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
    assert_eq!(names, expected);
    assert_eq!(methods["result"], json!(expected));
    script.close().await;
    atomic.close().await;
}

fn parity_fixture(catalog: &Catalog) {
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
        title: "Owned parity fixture".into(),
        visible: true,
        scene: WindowLayout::new([0., 0., 20., 10.], 0),
    }]));
}

#[tokio::test]
async fn sdk_canonical_object_arguments_have_matching_results_and_effects() {
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
        // Preserve floating-point JSON tokens at the MCP boundary. JavaScript
        // serializes these same integral values as integers before host calls.
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
        let (script, script_catalog) = selected(Toolset::Script);
        let (atomic, atomic_catalog) = selected(Toolset::Atomic);
        parity_fixture(&script_catalog);
        parity_fixture(&atomic_catalog);
        let script_entries = script.watch_script_engine();
        let atomic_entries = atomic.watch_script_engine();
        let mut script = Client::new(script).await;
        let mut atomic = Client::new(atomic).await;
        for (client, mode) in [
            (&mut script, Toolset::Script),
            (&mut atomic, Toolset::Atomic),
        ] {
            let seed_method = if method == "click" && args.get("x").is_some() {
                "capture"
            } else {
                "look"
            };
            let seed = mode_call(client, mode, seed_method, json!({})).await;
            assert_eq!(seed["isError"], false, "{seed}");
        }
        let script_reply = mode_call(&mut script, Toolset::Script, method, args.clone()).await;
        let atomic_reply = mode_call(&mut atomic, Toolset::Atomic, method, args.clone()).await;
        assert_eq!(
            script_reply["isError"], false,
            "{method} {args}: {script_reply}"
        );
        assert_eq!(
            atomic_reply["isError"], false,
            "{method} {args}: {atomic_reply}"
        );
        let script_result = decoded(&script_reply);
        let atomic_result = decoded(&atomic_reply);
        for key in ["view", "result", "effects"] {
            assert_eq!(
                script_result[key], atomic_result[key],
                "{method} {args}: {key}\nscript: {script_result}\natomic: {atomic_result}"
            );
        }
        if method == "readText" {
            assert_eq!(
                atomic_result["result"]["text"],
                if args["offset"].as_f64() == Some(0.0) {
                    "A🙂"
                } else {
                    "B"
                }
            );
        }
        if method == "capture" {
            let image = |reply: &Value| {
                reply["content"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|block| block["type"] == "image")
                    .unwrap()
                    .clone()
            };
            assert_eq!(image(&script_reply), image(&atomic_reply));
        }
        for catalog in [&script_catalog, &atomic_catalog] {
            assert_eq!(
                catalog.backend.0.actions.load(Ordering::SeqCst),
                actions,
                "{method} {args}"
            );
        }
        assert_eq!(
            *script_catalog.backend.0.set_values.lock().unwrap(),
            *atomic_catalog.backend.0.set_values.lock().unwrap()
        );
        assert_eq!(
            script_catalog.backend.0.input_targets.lock().unwrap().len(),
            atomic_catalog.backend.0.input_targets.lock().unwrap().len()
        );
        assert_eq!(
            script_catalog.backend.0.press_targets.lock().unwrap().len(),
            atomic_catalog.backend.0.press_targets.lock().unwrap().len()
        );
        if method == "setValue" {
            assert_eq!(
                *atomic_catalog.backend.0.set_values.lock().unwrap(),
                vec![(ElementId(77), "fresh 中文🙂".into())]
            );
            assert!(atomic_catalog
                .backend
                .0
                .input_targets
                .lock()
                .unwrap()
                .is_empty());
        }
        assert_eq!(script_entries.load(Ordering::SeqCst), 2);
        assert_eq!(atomic_entries.load(Ordering::SeqCst), 0);
        script.close().await;
        atomic.close().await;
    }
}

#[tokio::test]
async fn sdk_malformed_object_arguments_match_business_errors_without_backend_input() {
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
        // Fresh paired contexts prevent a previous rejection's cleanup from
        // affecting the next validation case. Exec admission is intentionally
        // different; only the business error and absence of input are shared.
        for mode in [Toolset::Script, Toolset::Atomic] {
            let (server, catalog) = selected(mode);
            parity_fixture(&catalog);
            let mut client = Client::new(server).await;
            let reply = mode_call(&mut client, mode, method, args.clone()).await;
            assert_eq!(reply["isError"], true, "{mode:?} {method} {args}: {reply}");
            let result = decoded(&reply);
            assert_eq!(
                result["error"]["code"], "invalid_arguments",
                "{mode:?} {method} {args}: {result}"
            );
            assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
            assert!(catalog.backend.0.input_targets.lock().unwrap().is_empty());
            assert!(catalog.backend.0.press_targets.lock().unwrap().is_empty());
            assert!(catalog.backend.0.set_values.lock().unwrap().is_empty());
            client.close().await;
        }
    }
}
