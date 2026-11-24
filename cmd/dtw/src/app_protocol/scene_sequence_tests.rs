use super::*;

fn button(id: u64, identity: Option<&str>, name: &str) -> Element {
    Element {
        id: ElementId(id),
        rebind_identity: identity.map(|id| format!("retained:{id}")),
        identity: identity.map(str::to_owned),
        label: name.into(),
        name: Some(name.into()),
        name_incomplete: false,
        role: "button".into(),
        value: None,
        value_truncated: false,
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
fn locator_exact_name_acts_on_native_names_and_reads_back_literal_values() {
    let (server, catalog) = agent();
    let mut field = button(4, Some("field"), "Note");
    field.role = "text_field".into();
    field.can_press = false;
    field.actions = vec!["setValue".into()];
    field.value = Some("exact value".into());
    let mut other = button(3, Some("other"), "Other");
    other.label = "Save".into();
    other.value = Some("Save".into());
    {
        let mut model = catalog.backend.0.model.lock().unwrap();
        model.filtered_observation = true;
        model.semantic_elements = Some(vec![
            button(1, Some("save"), "Save"),
            button(2, Some("save_as"), "Save As"),
            other,
            field,
        ]);
    }
    let reply = exec(
        &server,
        "exact-locator",
        r#"
        const options = {name:'Save', exact:true};
        const save = app.getByRole('button', options);
        options.name = 'Other';
        await save.click();
        const field = app.getByRole('text_field', {name:'Note', exact:true});
        await field.setValue('exact value');
        return await field.waitFor({value:'exact value',timeout:0});
    "#,
    );
    assert_eq!(
        *success(&reply),
        json!({"state":"attached","element":{
            "role":"text_field","name":"Note","value":"exact value","enabled":true,"actions":["setValue"]
        }})
    );
    assert_eq!(
        *catalog.backend.0.pressed_elements.lock().unwrap(),
        [ElementId(1)]
    );
    assert_eq!(
        *catalog.backend.0.set_values.lock().unwrap(),
        [(ElementId(4), "exact value".into())]
    );
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 2);
    assert!(reply.data["value"]["view"].is_object());
}

#[test]
fn locator_states_keep_duplicates_and_require_complete_proofs() {
    let (server, catalog) = agent();
    let mut disabled = button(2, Some("disabled"), "Save");
    disabled.enabled = Some(false);
    disabled.actions.clear();
    disabled.can_press = false;
    let enabled = button(1, Some("enabled"), "Save");
    {
        let mut model = catalog.backend.0.model.lock().unwrap();
        model.filtered_observation = true;
        model.semantic_elements = Some(vec![enabled.clone(), disabled.clone()]);
    }
    let ambiguous = exec(&server, "locator-dupes", "await app.getByRole('button',{name:'Save',exact:true}).waitFor({state:'enabled',timeout:0});");
    assert_eq!(
        ambiguous.data["error"]["code"], "selector_ambiguous",
        "{}",
        ambiguous.data
    );
    let mut unreadable = disabled.clone();
    unreadable.enabled = None;
    let mut clipped = enabled.clone();
    clipped.value = Some("shown prefix".into());
    clipped.value_truncated = true;
    for (index, (rows, options, code)) in [
        (
            vec![disabled.clone()],
            "{state:'enabled',timeout:0}",
            "wait_timeout",
        ),
        (
            vec![unreadable],
            "{state:'disabled',timeout:0}",
            "selector_unproven",
        ),
        (
            vec![clipped],
            "{value:'shown prefix',timeout:0}",
            "selector_unproven",
        ),
        (vec![enabled], "{value:'',timeout:0}", "selector_unproven"),
    ]
    .into_iter()
    .enumerate()
    {
        catalog.backend.0.model.lock().unwrap().semantic_elements = Some(rows);
        let reply = exec(
            &server,
            &format!("locator-state-{index}"),
            &format!(
                "await app.getByRole('button',{{name:'Save',exact:true}}).waitFor({options});"
            ),
        );
        assert_eq!(reply.data["error"]["code"], code, "{}", reply.data);
    }
    catalog.backend.0.model.lock().unwrap().semantic_elements = Some(vec![disabled]);
    let reply = exec(
        &server,
        "locator-states",
        r#"
        const save = app.getByRole('button', {name:'sAv'});
        const disabled = await save.waitFor({state:'disabled',timeout:0});
        const detached = await app.getByRole('button',{name:'save',exact:true}).waitFor({state:'detached',timeout:0});
        return [disabled.state, detached];
    "#,
    );
    assert_eq!(*success(&reply), json!(["disabled", {"state":"detached"}]));
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
}

#[test]
fn locator_capability_failure_is_recorded_before_action_resolution() {
    let (server, catalog) = agent();
    let mut unsupported = button(1, Some("unsupported"), "Read only");
    unsupported.can_press = false;
    unsupported.actions.clear();
    {
        let mut model = catalog.backend.0.model.lock().unwrap();
        model.filtered_observation = true;
        model.semantic_elements = Some(vec![unsupported]);
    }
    for (method, code) in [
        ("click()", "element_not_pressable"),
        ("setValue('new')", "unsupported_capability"),
    ] {
        let reply = exec(
            &server,
            code,
            &format!("await app.getByRole('button',{{name:'Read only',exact:true}}).{method};"),
        );
        assert_eq!(reply.data["error"]["code"], code, "{}", reply.data);
        let step = &reply.data["script"]["steps"][0];
        assert_eq!(step["resolution"]["status"], "unsupported");
        assert_eq!(step["receipt"]["outcome"]["dispatch"], "not_dispatched");
    }
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
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
