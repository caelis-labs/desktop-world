//! Real MCP transport and QuickJS, controlled native backend; no desktop evidence.
use super::*;
use crate::app_protocol::toolset::Toolset;

fn selected(mode: Toolset) -> (Arc<Server<Catalog>>, Catalog) {
    let (_, catalog) = server();
    (Server::with_toolset(catalog.clone(), mode), catalog)
}

#[tokio::test]
async fn sdk_toolset_selection_is_enforced_before_provider_connection() {
    for (mode, count, disabled) in [
        (Toolset::Script, 2, "look"),
        (Toolset::Atomic, 15, "exec"),
        (Toolset::All, 16, "windows"),
    ] {
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
        if mode != Toolset::Script {
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
async fn sdk_atomic_and_exec_share_scene_context_and_automatic_readback() {
    let (server, catalog) = selected(Toolset::All);
    let engine_entries = server.watch_script_engine();
    let mut client = Client::new(server).await;
    let first = decoded(&client.tool("look", json!({"app":APP})).await);
    assert_eq!(first["view"]["kind"], "full", "{first}");
    let second = decoded(&client.tool("look", json!({"app":APP})).await);
    assert_eq!(second["view"]["kind"], "unchanged", "{second}");
    let action = client
        .tool("click", json!({"app":APP,"target":"w1.e1"}))
        .await;
    assert_eq!(action["isError"], false, "{action}");
    let action = decoded(&action);
    assert_eq!(action["effects"]["dispatch"]["dispatched"], 1, "{action}");
    assert!(action["view"].is_object(), "{action}");
    assert!(action.get("script").is_none());
    assert_eq!(engine_entries.load(Ordering::SeqCst), 0);
    let third = decoded(
        &client
            .tool("exec", input("await app.look(); return 7;"))
            .await,
    );
    assert_eq!(third["result"], 7, "{third}");
    assert_eq!(engine_entries.load(Ordering::SeqCst), 1);
    let fourth = client.tool("look", json!({"app":APP})).await;
    assert_eq!(fourth["isError"], false, "{fourth}");
    assert_eq!(decoded(&fourth)["view"]["kind"], "unchanged", "{fourth}");
    assert_eq!(engine_entries.load(Ordering::SeqCst), 1);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 1);
    assert_eq!(catalog.backend.0.finishes.load(Ordering::SeqCst), 5);
    client.close().await;
    // Server::close retires contexts before and after waiting for active work.
    assert_eq!(catalog.backend.0.shutdowns.load(Ordering::SeqCst), 2);
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
            .tool("read_text", json!({"app":APP,"target":"w1.e1","limit":3}))
            .await,
    );
    assert_eq!(page["result"]["text"], "A🙂", "{page}");
    assert_eq!(page["result"]["next_offset"], 3);
    let end = decoded(
        &client
            .tool("read_text", json!({"app":APP,"target":"w1.e1","offset":3}))
            .await,
    );
    assert_eq!(end["result"]["text"], "B", "{end}");
    assert!(end["result"]["next_offset"].is_null());
    let value = "\"}); await app.quit(); //\n中文🙂 \\ ${app.quit()}\u{2028}";
    let result = client
        .tool(
            "set_value",
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
    let (_, catalog) = selected(Toolset::Atomic);
    catalog.backend.0.model.lock().unwrap().poll_action = true;
    let factory = CleanupCatalog {
        inner: catalog.clone(),
        close_gate: Arc::default(),
    };
    let server = Server::with_toolset(factory.clone(), Toolset::All);
    let engine_entries = server.watch_script_engine();
    let mut client = Client::new(server).await;
    client.tool("look", json!({"app":APP})).await;
    let action = catalog.backend.action_gate();
    let params = json!({"name":"click","arguments":{"app":APP,"target":"w1.e1"}});
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
    let blocked = decoded(&client.tool("exec", input("return 1;")).await);
    assert_eq!(blocked["error"]["code"], "busy", "{blocked}");
    let blocked = decoded(&client.tool("look", json!({"app":APP})).await);
    assert_eq!(blocked["error"]["code"], "busy", "{blocked}");
    assert_eq!(engine_entries.load(Ordering::SeqCst), 0);
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
    assert_eq!(engine_entries.load(Ordering::SeqCst), 0);
    let stale = client
        .tool("click", json!({"app":APP,"target":"w1.e1"}))
        .await;
    assert_eq!(stale["isError"], true, "{stale}");
    let recovered = client.tool("look", json!({"app":APP})).await;
    assert_eq!(recovered["isError"], false, "{recovered}");
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 2);
    assert_eq!(engine_entries.load(Ordering::SeqCst), 0);
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
            .tool("read_text", json!({"app":APP,"target":"w1.e2"}))
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
                "set_value",
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
        .tool("type_text", json!({"app":APP,"text":text}))
        .await;
    assert_eq!(result["isError"], false, "{result}");
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    let oversized = client
        .tool(
            "type_text",
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
    let (_, catalog) = selected(Toolset::Atomic);
    let factory = CleanupCatalog {
        inner: catalog.clone(),
        close_gate: Arc::default(),
    };
    let server = Server::with_toolset(factory.clone(), Toolset::Atomic);
    let engine_entries = server.watch_script_engine();
    let mut client = Client::new(server).await;
    client.tool("look", json!({"app":APP})).await;
    let finish = catalog.backend.finish_gate();
    let params = json!({"name":"click","arguments":{"app":APP,"target":"w1.e1"}});
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
    let blocked = decoded(&client.tool("look", json!({"app":APP})).await);
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
    assert_eq!(engine_entries.load(Ordering::SeqCst), 0);
    client.close().await;
}
