use super::*;

#[test]
fn known_keyboard_input_needs_no_explicit_look_and_reads_back_once_per_script() {
    let (server, catalog) = agent();
    let first = exec(&server, "first_keys", "await app.keypress('ESC');");
    assert!(!first.is_error, "{}", first.data);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(catalog.backend.0.observes.load(Ordering::SeqCst), 1);
    assert_eq!(first.data["value"]["view"]["reset"], true);
    let next = exec(
        &server,
        "next_keys",
        "await app.typeText('known text'); await app.keypress('ENTER');",
    );
    assert!(!next.is_error, "{}", next.data);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 3);
    assert_eq!(catalog.backend.0.observes.load(Ordering::SeqCst), 2);
}

#[test]
fn a_known_selector_can_start_a_script_without_publishing_a_redundant_baseline() {
    let (server, catalog) = agent();
    catalog.backend.0.model.lock().unwrap().filtered_observation = true;
    let reply = exec(&server, "known_selector", "await app.click('Increment');");
    assert!(!reply.is_error, "{}", reply.data);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    // Native selector resolution plus final readback; no initial semantic look.
    assert_eq!(catalog.backend.0.observes.load(Ordering::SeqCst), 2);
    assert_eq!(reply.data["value"]["view"]["reset"], true);
    assert!(reply.data["value"]["view"]["elements"].is_array());
}

#[test]
fn selector_wait_follows_a_popup_that_arrives_after_its_first_read() {
    selector_follows_popup_after_read(Duration::ZERO);
}

#[test]
fn slow_selector_follows_a_proven_new_surface_after_its_polling_budget_ends() {
    selector_follows_popup_after_read(Duration::from_millis(2100));
}

fn selector_follows_popup_after_read(read_delay: Duration) {
    let (server, catalog) = agent();
    let window = |native_id, rank| WindowInfo {
        target: WindowTarget {
            app: application(),
            native_id,
            generation: 3,
        },
        title: format!("Window {native_id}"),
        visible: true,
        scene: WindowLayout::new([0., 0., 500., 400.], rank),
    };
    {
        let mut model = catalog.backend.0.model.lock().unwrap();
        model.filtered_observation = true;
        model.inventory = Some(test_inventory(vec![window(101, 1)]));
        model.window_semantics.insert(101, vec![]);
        let action = Element {
            id: ElementId(1),
            identity: Some("popup-action".into()),
            rebind_identity: Some("retained-popup-action".into()),
            label: "Popup action".into(),
            name: Some("Popup action".into()),
            role: "button".into(),
            value: None,
            enabled: Some(true),
            actions: vec!["press".into()],
            can_press: true,
        };
        model.window_semantics.insert(102, vec![action]);
    }
    let gate = catalog.backend.read_gate();
    let running = server.clone();
    let worker = std::thread::spawn(move || {
        exec(&running, "late_popup", "await app.click('Popup action');")
    });
    gate.entered();
    std::thread::sleep(read_delay);
    catalog.backend.0.model.lock().unwrap().inventory =
        Some(test_inventory(vec![window(102, 0), window(101, 1)]));
    gate.release();
    let reply = worker.join().unwrap();
    assert!(!reply.is_error, "{}", reply.data);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(
        catalog.backend.0.press_targets.lock().unwrap()[0].native_id,
        102
    );
}

#[test]
fn selector_wait_cannot_follow_a_sibling_process_when_the_original_window_disappears() {
    let (server, catalog) = agent();
    let a = application();
    let mut b = a.clone();
    b.pid += 1;
    b.generation -= 1;
    catalog.entries.lock().unwrap().push(Application {
        app_id: APP.into(),
        app: b.clone(),
        name: "Sibling".into(),
    });
    let window = |app: AppInstance, native_id| WindowInfo {
        target: WindowTarget {
            app,
            native_id,
            generation: 3,
        },
        title: "Window".into(),
        visible: true,
        scene: WindowLayout::new([0., 0., 500., 400.], 0),
    };
    {
        let mut model = catalog.backend.0.model.lock().unwrap();
        model.filtered_observation = true;
        model.instance_inventories.insert(
            (a.pid, a.generation),
            test_inventory(vec![window(a.clone(), 101)]),
        );
        model
            .instance_inventories
            .insert((b.pid, b.generation), test_inventory(vec![]));
        model.window_semantics.insert(101, vec![]);
    }
    let gate = catalog.backend.read_gate();
    let running = server.clone();
    let worker = std::thread::spawn(move || {
        exec(&running, "sibling_popup", "await app.click('Increment');")
    });
    gate.entered();
    {
        let mut model = catalog.backend.0.model.lock().unwrap();
        model
            .instance_inventories
            .insert((a.pid, a.generation), test_inventory(vec![]));
        model
            .instance_inventories
            .insert((b.pid, b.generation), test_inventory(vec![window(b, 201)]));
    }
    gate.release();
    let reply = worker.join().unwrap();
    assert!(reply.is_error, "{}", reply.data);
    assert_eq!(reply.data["error"]["code"], "target_changed");
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
}

#[test]
fn scripts_without_ui_operations_do_not_observe_the_app() {
    let (server, catalog) = agent();
    let reply = exec(&server, "no_ui", "return {ready:true};");
    assert!(!reply.is_error, "{}", reply.data);
    assert_eq!(success(&reply), &json!({"ready":true}));
    assert_eq!(catalog.backend.0.observes.load(Ordering::SeqCst), 0);
    assert_eq!(catalog.backend.0.windows.load(Ordering::SeqCst), 0);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
}

#[test]
fn menu_observation_is_read_only_and_target_renewal_keeps_the_scope() {
    let (server, catalog) = agent();
    {
        let mut model = catalog.backend.0.model.lock().unwrap();
        model.menu_scope = true;
        model.filtered_observation = true;
    }
    let read = exec(
        &server,
        "menu_read",
        "await app.look({scope:'menu',text:'Increment'});",
    );
    assert!(!read.is_error, "{}", read.data);
    assert_eq!(
        read.data["value"]["view"]["semantics"]["query"],
        json!({"scope":"menu","text":"Increment"})
    );
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
    assert_eq!(
        catalog.backend.0.observe_scopes.lock().unwrap().as_slice(),
        &[ObserveScopeKind::Menu]
    );
    let target = read.data["value"]["view"]["elements"][0]["id"]
        .as_str()
        .unwrap();
    let clicked = exec(
        &server,
        "menu_target",
        &format!("await app.click({});", json!(target)),
    );
    assert!(!clicked.is_error, "{}", clicked.data);
    assert_eq!(
        catalog.backend.0.observe_scopes.lock().unwrap().as_slice(),
        &[ObserveScopeKind::Menu, ObserveScopeKind::Menu]
    );
}

#[test]
fn explicit_window_scope_is_the_default_and_unknown_scopes_never_read() {
    let (server, catalog) = agent();
    let first = exec(&server, "window_scope", "await app.look({scope:'window'});");
    assert!(!first.is_error, "{}", first.data);
    assert!(first.data["value"]["view"]["semantics"]
        .get("query")
        .is_none());
    let same = exec(&server, "default_scope", "await app.look();");
    assert_eq!(same.data["value"]["view"], json!({"changed":false}));
    let reads = catalog.backend.0.observes.load(Ordering::SeqCst);
    let bad = exec(&server, "bad_scope", "await app.look({scope:'desktop'});");
    assert!(bad.is_error);
    assert_eq!(catalog.backend.0.observes.load(Ordering::SeqCst), reads);
}

#[test]
fn implicit_semantic_and_scroll_routes_skip_decorations_without_changing_keyboard_focus() {
    for operation in ["selector", "scroll", "keys"] {
        let (server, catalog) = agent();
        let rows = [(102, false), (101, true)]
            .into_iter()
            .enumerate()
            .map(|(rank, (native_id, candidate))| {
                let mut scene = WindowLayout::new([0., 0., 500., 400.], rank as u32).unwrap();
                scene.semantic_candidate = Some(candidate);
                scene.keyboard_focused = Some(native_id == 101);
                WindowInfo {
                    target: WindowTarget {
                        app: application(),
                        native_id,
                        generation: 3,
                    },
                    title: if candidate { "Editing" } else { "Decoration" }.into(),
                    visible: true,
                    scene: Some(scene),
                }
            })
            .collect();
        {
            let mut model = catalog.backend.0.model.lock().unwrap();
            model.inventory = Some(test_inventory(rows));
            model.filtered_observation = true;
        }
        let source = match operation {
            "selector" => "await app.click('Increment');",
            "scroll" => "await app.scroll({delta_y:1});",
            _ => "await app.keypress('ESC');",
        };
        let reply = exec(&server, operation, source);
        assert!(!reply.is_error, "{operation}: {}", reply.data);
        let received = match operation {
            "selector" => catalog.backend.0.press_targets.lock().unwrap()[0].clone(),
            "scroll" => catalog.backend.0.anchored_scrolls.lock().unwrap()[0]
                .0
                .clone(),
            _ => catalog.backend.0.input_targets.lock().unwrap()[0].clone(),
        };
        assert_eq!(received.native_id, 101, "{operation}");
    }
}

#[test]
fn explicit_focus_can_start_without_look_and_keeps_the_public_app_scope() {
    let (server, catalog) = agent();
    let focused = exec(&server, "explicit_focus", "await app.focus();");
    assert!(!focused.is_error, "{}", focused.data);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(catalog.backend.0.observes.load(Ordering::SeqCst), 1);
    let invalid = exec(&server, "focus_arguments", "await app.focus({pid:7});");
    assert!(invalid.is_error);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
}

#[test]
fn windowless_shortcut_waits_for_its_new_surface_before_following_text_without_replay() {
    windowless_shortcut_waits_for_surface(false);
}

#[test]
fn windowless_shortcut_waits_before_the_same_script_reads_its_new_window() {
    windowless_shortcut_waits_for_surface(true);
}

fn windowless_shortcut_waits_for_surface(read_in_script: bool) {
    let (server, catalog) = agent();
    {
        let mut model = catalog.backend.0.model.lock().unwrap();
        model.inventory = Some(test_inventory(vec![]));
        model.application_input = true;
    }
    let gate = catalog.backend.action_gate();
    let running = server.clone();
    let worker = std::thread::spawn(move || {
        exec(
            &running,
            "new_then_text",
            if read_in_script {
                "await app.keypress('META+N'); return (await app.look()).windows[0]?.title;"
            } else {
                "await app.keypress('META+N'); await app.typeText('new document text');"
            },
        )
    });
    gate.entered();
    gate.release();
    std::thread::sleep(Duration::from_millis(80));
    catalog.backend.0.model.lock().unwrap().inventory = Some(test_inventory(vec![WindowInfo {
        target: WindowTarget {
            app: application(),
            native_id: 301,
            generation: 3,
        },
        title: "New document".into(),
        visible: true,
        scene: WindowLayout::new([0., 0., 500., 400.], 0),
    }]));
    let reply = worker.join().unwrap();
    assert!(!reply.is_error, "{}", reply.data);
    if read_in_script {
        assert_eq!(reply.data["value"]["result"], "New document");
        assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    } else {
        assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 2);
        assert_eq!(
            catalog.backend.0.input_targets.lock().unwrap()[0].native_id,
            301
        );
    }
}

#[test]
fn new_window_wait_is_read_only_and_survives_automatic_readback_between_execs() {
    for separate in [false, true] {
        let (server, catalog) = agent();
        let row = |native_id, rank| WindowInfo {
            target: WindowTarget {
                app: application(),
                native_id,
                generation: 3,
            },
            title: format!("Window {native_id}"),
            visible: true,
            scene: WindowLayout::new([0., 0., 500., 400.], rank),
        };
        catalog.backend.0.model.lock().unwrap().inventory = Some(test_inventory(vec![row(101, 0)]));
        let gate = catalog.backend.action_gate();
        let running = server.clone();
        let worker = std::thread::spawn(move || {
            exec(
                &running,
                "new",
                if separate {
                    "await app.keypress('META+N');"
                } else {
                    "await app.keypress('META+N'); await app.waitForNewWindow(); await app.typeText('body');"
                },
            )
        });
        gate.entered();
        catalog.backend.0.model.lock().unwrap().inventory =
            Some(test_inventory(vec![row(303, 0), row(101, 1)]));
        gate.release();
        let first = worker.join().unwrap();
        assert!(!first.is_error, "{}", first.data);
        if separate {
            let next = exec(
                &server,
                "wait_next",
                "await app.waitForNewWindow(); await app.typeText('body');",
            );
            assert!(!next.is_error, "{}", next.data);
        }
        let targets = catalog.backend.0.input_targets.lock().unwrap();
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].native_id, 101);
        assert_eq!(targets[1].native_id, 303);
        assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 2);
    }
}

#[test]
fn new_window_wait_requires_input_and_times_out_without_replaying_it() {
    let (server, catalog) = agent();
    let first = exec(&server, "no_input", "await app.waitForNewWindow();");
    assert!(first.is_error, "{}", first.data);
    assert_eq!(first.data["error"]["code"], "input_required");
    assert_eq!(catalog.backend.0.windows.load(Ordering::SeqCst), 0);
    let timeout = exec(
        &server,
        "timeout",
        "await app.look(); await app.keypress('META+N'); await app.waitForNewWindow({timeout_ms:40});",
    );
    assert!(timeout.is_error, "{}", timeout.data);
    assert_eq!(timeout.data["error"]["code"], "wait_timeout");
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    assert!(
        timeout.data["value"]["view"].is_object(),
        "{}",
        timeout.data
    );
}
