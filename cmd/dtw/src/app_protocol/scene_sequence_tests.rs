use super::*;

fn button(id: u64, identity: Option<&str>, name: &str) -> Element {
    Element {
        id: ElementId(id),
        rebind_identity: identity.map(|id| format!("retained:{id}")),
        identity: identity.map(str::to_owned),
        label: name.into(),
        name: Some(name.into()),
        role: "button".into(),
        value: None,
        enabled: Some(true),
        actions: vec!["press".into()],
        focused: false,
        can_press: true,
    }
}

#[test]
fn known_targets_support_sequential_input_without_explicit_reads() {
    let (server, catalog) = agent();
    catalog.backend.0.model.lock().unwrap().semantic_elements = Some(vec![
        button(1, Some("one"), "1"),
        button(2, Some("two"), "2"),
    ]);
    let reply = exec(&server, "sequence", "const v=await app.look(); const [a,b]=v.elements; await app.click(a.id); await app.click(b.id); await app.click(a.id);");
    assert!(!reply.is_error, "{}", reply.data);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 3);
}

#[test]
fn renewed_target_must_keep_object_identity_and_meaning() {
    for change in ["replace", "rename", "disable", "remove", "duplicate"] {
        let (server, catalog) = agent();
        catalog.backend.0.model.lock().unwrap().semantic_elements =
            Some(vec![button(1, Some("one"), "Continue")]);
        exec(&server, "seen", "await app.look();");
        let mut next = button(1, Some("one"), "Continue");
        match change {
            "replace" => {
                next.identity = Some("replacement".into());
                next.rebind_identity = Some("replacement".into());
            }
            "rename" => next.name = Some("Delete".into()),
            "disable" => next.enabled = Some(false),
            _ => {}
        }
        catalog.backend.0.model.lock().unwrap().semantic_elements = Some(match change {
            "remove" => vec![],
            "duplicate" => vec![next.clone(), button(2, Some("one"), "Continue")],
            _ => vec![next],
        });
        let reply = exec(&server, "changed", "await app.click('w1.e1');");
        assert!(reply.is_error, "{change}: {}", reply.data);
        assert_eq!(
            catalog.backend.0.actions.load(Ordering::SeqCst),
            0,
            "{change}"
        );
    }
}

#[test]
fn unknown_input_is_not_replayed_to_complete_a_sequence() {
    let (server, catalog) = agent();
    catalog.backend.0.model.lock().unwrap().outcome =
        ActionOutcome::unknown(Error::BackendFailed("interrupted".into()));
    let reply = exec(
        &server,
        "uncertain",
        "await app.look(); await app.click('w1.e1'); await app.click('w1.e1');",
    );
    assert!(reply.is_error, "{}", reply.data);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
}

#[test]
fn unidentified_targets_cannot_be_rebound_by_label_or_position() {
    let (server, catalog) = agent();
    catalog.backend.0.model.lock().unwrap().semantic_elements =
        Some(vec![button(1, None, "Continue")]);
    let reply = exec(&server, "no_identity", "const v=await app.look(); const id=v.elements[0].id; await app.click(id); await app.click(id);");
    assert!(reply.is_error, "{}", reply.data);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
}

#[test]
fn implicit_keyboard_renews_only_one_transient_inventory_without_replaying_input() {
    for (cause, failures, succeeds) in [
        (Error::TargetChanged, 1, true),
        (
            Error::NativeFailure {
                code: "target_changed".into(),
                message: "Popup closed during inventory".into(),
            },
            1,
            true,
        ),
        (Error::TargetChanged, 2, false),
        (Error::WrongApp, 1, false),
    ] {
        let (server, catalog) = agent();
        let gate = catalog.backend.action_gate();
        let running = server.clone();
        let worker = std::thread::spawn(move || {
            exec(&running, "keyboard_inventory", "await app.look(); await app.keypress('META+L'); await app.typeText('example.com');")
        });
        gate.entered();
        for _ in 0..failures {
            catalog
                .backend
                .0
                .model
                .lock()
                .unwrap()
                .window_errors
                .push_back(cause.clone());
        }
        gate.release();
        let reply = worker.join().unwrap();
        assert_eq!(!reply.is_error, succeeds, "{}", reply.data);
        assert_eq!(
            catalog.backend.0.actions.load(Ordering::SeqCst),
            if succeeds { 2 } else { 1 }
        );
    }
}

#[test]
fn keyboard_routes_by_native_focus_instead_of_visual_overlay_order() {
    for focused in [Some(101), Some(102), None] {
        let (server, catalog) = agent();
        let rows = [(102, "Overlay"), (101, "Editing window")]
            .into_iter()
            .enumerate()
            .map(|(rank, (native_id, title))| {
                let mut scene = WindowLayout::new([0., 0., 500., 400.], rank as u32).unwrap();
                scene.keyboard_focused = focused.map(|id| id == native_id);
                WindowInfo {
                    target: WindowTarget {
                        app: application(),
                        native_id,
                        generation: 3,
                    },
                    title: title.into(),
                    visible: true,
                    scene: Some(scene),
                }
            })
            .collect();
        catalog.backend.0.model.lock().unwrap().inventory = Some(test_inventory(rows));
        let reply = exec(&server, "keys", "await app.look(); await app.keypress('META+L'); await app.typeText('example.com'); await app.keypress('ENTER');");
        assert!(!reply.is_error, "{}", reply.data);
        let targets = catalog.backend.0.input_targets.lock().unwrap();
        assert_eq!(targets.len(), 3);
        assert!(targets
            .iter()
            .all(|t| t.native_id == focused.unwrap_or(102)));
        assert!(!reply.data.to_string().contains("keyboard_focused"));
    }
}

#[test]
fn proven_modal_receiver_overrides_stale_document_keyboard_focus() {
    for modal in [Some(true), Some(false), None] {
        let (server, catalog) = agent();
        let rows = [(101, "Editing window"), (102, "Open dialog")]
            .into_iter()
            .enumerate()
            .map(|(rank, (native_id, title))| {
                let mut scene = WindowLayout::new([0., 0., 500., 400.], rank as u32).unwrap();
                scene.keyboard_focused = Some(native_id == 101);
                scene.keyboard_modal = if native_id == 102 { modal } else { None };
                WindowInfo {
                    target: WindowTarget {
                        app: application(),
                        native_id,
                        generation: 3,
                    },
                    title: title.into(),
                    visible: true,
                    scene: Some(scene),
                }
            })
            .collect();
        catalog.backend.0.model.lock().unwrap().inventory = Some(test_inventory(rows));
        let reply = exec(
            &server,
            "modal",
            "await app.look(); await app.keypress('META+SHIFT+G'); await app.typeText('/tmp');",
        );
        assert!(!reply.is_error, "{}", reply.data);
        let targets = catalog.backend.0.input_targets.lock().unwrap();
        assert_eq!(targets.len(), 2);
        assert!(targets
            .iter()
            .all(|t| t.native_id == if modal == Some(true) { 102 } else { 101 }));
        assert!(!reply.data.to_string().contains("keyboard_modal"));
    }
}

#[test]
fn semantic_decoration_hint_does_not_replace_keyboard_focus_or_unfocused_popup_semantics() {
    for candidate in [None, Some(true), Some(false)] {
        let (server, catalog) = agent();
        let rows = [(102, "Popup"), (101, "Editing window")]
            .into_iter()
            .enumerate()
            .map(|(rank, (native_id, title))| {
                let mut scene = WindowLayout::new([0., 0., 500., 400.], rank as u32).unwrap();
                scene.keyboard_focused = Some(native_id == 101);
                scene.semantic_candidate = if native_id == 102 { candidate } else { None };
                WindowInfo {
                    target: WindowTarget {
                        app: application(),
                        native_id,
                        generation: 3,
                    },
                    title: title.into(),
                    visible: true,
                    scene: Some(scene),
                }
            })
            .collect();
        catalog.backend.0.model.lock().unwrap().inventory = Some(test_inventory(rows));
        let reply = exec(
            &server,
            "hint",
            "const v=await app.look(); await app.keypress('ENTER'); return v.elements[0].id;",
        );
        assert!(!reply.is_error, "{}", reply.data);
        let prefix = if candidate == Some(false) {
            "w2."
        } else {
            "w1."
        };
        assert!(reply.data["value"]["result"]
            .as_str()
            .unwrap()
            .starts_with(prefix));
        assert_eq!(
            catalog.backend.0.input_targets.lock().unwrap()[0].native_id,
            101
        );
        assert!(!reply.data.to_string().contains("semantic_candidate"));
    }
}

#[test]
fn read_time_decoration_proof_can_choose_content_but_arbitrary_ax_errors_cannot() {
    for code in ["non_content_surface", "ax_unavailable"] {
        let (server, catalog) = agent();
        let rows = [(102, "Decoration"), (101, "Document")]
            .into_iter()
            .enumerate()
            .map(|(rank, (native_id, title))| WindowInfo {
                target: WindowTarget {
                    app: application(),
                    native_id,
                    generation: 3,
                },
                title: title.into(),
                visible: true,
                scene: Some(WindowLayout::new([0., 0., 500., 400.], rank as u32).unwrap()),
            })
            .collect();
        {
            let mut model = catalog.backend.0.model.lock().unwrap();
            model.inventory = Some(test_inventory(rows));
            model.semantic_pages.push_back(Err(Error::NativeFailure {
                code: code.into(),
                message: "Native read proof".into(),
            }));
        }
        let reply = exec(
            &server,
            "read_proof",
            "const v=await app.look(); return v.elements.map(e=>e.id);",
        );
        assert!(!reply.is_error, "{}", reply.data);
        if code == "non_content_surface" {
            assert!(reply.data["value"]["result"][0]
                .as_str()
                .unwrap()
                .starts_with("w2."));
            assert_eq!(catalog.backend.0.observes.load(Ordering::SeqCst), 2);
        } else {
            assert_eq!(reply.data["value"]["result"], json!([]));
            assert_eq!(catalog.backend.0.observes.load(Ordering::SeqCst), 1);
        }
    }
}

#[test]
fn keyboard_focus_hints_cannot_select_a_sibling_process_instance() {
    for remove_selected in [false, true] {
        let (server, catalog) = agent();
        let a = application();
        let mut b = a.clone();
        b.pid += 1;
        b.generation -= 1;
        catalog.entries.lock().unwrap().push(Application {
            app_id: APP.into(),
            app: b.clone(),
            name: "Second instance".into(),
        });
        let row = |app: &AppInstance, native_id, rank, focus| {
            let mut scene = WindowLayout::new([0., 0., 500., 400.], rank).unwrap();
            scene.keyboard_focused = focus;
            WindowInfo {
                target: WindowTarget {
                    app: app.clone(),
                    native_id,
                    generation: 3,
                },
                scene: Some(scene),
                title: format!("Instance {}", app.pid),
                visible: true,
            }
        };
        {
            let mut model = catalog.backend.0.model.lock().unwrap();
            model.instance_inventories.insert(
                (a.pid, a.generation),
                test_inventory(vec![row(&a, 101, 0, None)]),
            );
            model.instance_inventories.insert(
                (b.pid, b.generation),
                test_inventory(vec![
                    row(&b, 201, 1, Some(true)),
                    row(&b, 202, 2, Some(false)),
                ]),
            );
        }
        let seen = exec(&server, "two_pids_seen", "await app.look();");
        assert!(!seen.is_error, "{}", seen.data);
        if remove_selected {
            catalog
                .backend
                .0
                .model
                .lock()
                .unwrap()
                .instance_inventories
                .insert((a.pid, a.generation), test_inventory(vec![]));
        }
        let reply = exec(
            &server,
            "two_pids_input",
            "await app.typeText('hello'); await app.keypress('META+W');",
        );
        assert_eq!(reply.is_error, remove_selected, "{}", reply.data);
        let inputs = catalog.backend.0.input_targets.lock().unwrap();
        assert_eq!(inputs.len(), if remove_selected { 0 } else { 2 });
        assert!(inputs.iter().all(|target| target.app == a));
    }
}

#[test]
fn comparison_id_reuse_cannot_renew_a_consumed_native_object() {
    let (server, catalog) = agent();
    let mut original = button(1, Some("uia:reusable-runtime-id"), "Delete");
    original.rebind_identity = None; // UIA runtime IDs are comparison facts only.
    let mut replacement = original.clone();
    replacement.id = ElementId(2);
    {
        let mut model = catalog.backend.0.model.lock().unwrap();
        model.semantic_elements = Some(vec![original]);
        model.semantics_after_action = Some(vec![replacement]);
    }
    let reply = exec(&server, "uia_reuse", "const view=await app.look(); const id=view.elements[0].id; await app.click(id); await app.click(id);");
    assert!(reply.is_error, "{}", reply.data);
    assert_eq!(reply.data["script"]["failure"]["code"], "target_changed");
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
}

#[test]
fn invalid_local_read_arguments_do_not_cancel_pending_action_readback() {
    for read in ["look", "capture"] {
        let (server, catalog) = agent();
        let reply = exec(&server, "bad_local_read", &format!("await app.look(); await app.typeText('hello'); try {{ await app.{read}({{full:NaN}}); }} catch {{}}"));
        assert!(!reply.is_error, "{}", reply.data);
        let ops: Vec<_> = reply.data["script"]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["operation"].as_str().unwrap())
            .collect();
        assert_eq!(ops, ["scene.look", "scene.typeText", "scene.after"]);
        assert!(reply.data["value"]["view"].is_object());
        assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    }
}
