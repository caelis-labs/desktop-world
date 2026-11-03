use super::*;

fn button(id: u64, identity: Option<&str>, name: &str) -> Element {
    Element {
        id: ElementId(id),
        identity: identity.map(str::to_owned),
        label: name.into(),
        name: Some(name.into()),
        role: "button".into(),
        value: None,
        enabled: Some(true),
        actions: vec!["press".into()],
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
            "replace" => next.identity = Some("replacement".into()),
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
