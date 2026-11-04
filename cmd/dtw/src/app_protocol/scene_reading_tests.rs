use super::*;

fn content(id: u64, value: &str) -> Element {
    Element {
        id: ElementId(id),
        identity: Some(format!("text:{id}")),
        rebind_identity: Some(format!("retained:{id}")),
        label: value.into(),
        role: "static_text".into(),
        name: None,
        value: Some(value.into()),
        enabled: Some(true),
        actions: vec![],
        can_press: false,
    }
}

#[test]
fn omitted_text_is_explicit_and_tail_only_edits_change_the_view() {
    let (server, catalog) = agent();
    let prefix = "文😀".repeat(125);
    catalog.backend.0.model.lock().unwrap().semantic_elements =
        Some(vec![content(1, &format!("{prefix}first"))]);
    let first = exec(&server, "long", "await app.look();");
    let view = &first.data["value"]["view"];
    let preview = &view["elements"][0];
    assert_eq!(preview["value"].as_str().unwrap().chars().count(), 240);
    assert_eq!(preview["value_length"], 255);
    assert_eq!(preview["value_truncated"], true);
    assert!(preview.get("name_truncated").is_none());
    assert_eq!(view["semantics"]["complete"], false);
    assert_eq!(view["semantics"]["reason"], "text_truncated");
    let revision = preview["value_revision"].clone();
    catalog.backend.0.model.lock().unwrap().semantic_elements =
        Some(vec![content(1, &format!("{prefix}other"))]);
    let changed = exec(&server, "tail_edit", "await app.look();");
    let view = &changed.data["value"]["view"];
    assert_eq!(view["changed"], true);
    let updated = &view["elements"]["changed"][0];
    assert_eq!(updated["value"], preview["value"]);
    assert_eq!(updated["value_length"], preview["value_length"]);
    assert_ne!(updated["value_revision"], revision);
    let unchanged =
        crate::app_protocol::projection::execution(exec(&server, "unchanged", "await app.look();"));
    assert_eq!(unchanged.data["view"], json!({"changed":false}));
}

#[test]
fn native_query_reaches_content_beyond_the_default_budget_and_keeps_scope_explicit() {
    let (server, catalog) = agent();
    let mut elements: Vec<_> = (1..=200).map(|id| content(id, "unrelated")).collect();
    elements.push(content(201, "Unique needle in the final paragraph"));
    {
        let mut model = catalog.backend.0.model.lock().unwrap();
        model.filtered_observation = true;
        model.semantic_elements = Some(elements);
    }
    let narrowed = exec(
        &server,
        "needle",
        "await app.look({role:'static_text',text:'Unique needle'});",
    );
    assert!(!narrowed.is_error, "{}", narrowed.data);
    let view = &narrowed.data["value"]["view"];
    assert_eq!(view["elements"].as_array().unwrap().len(), 1);
    assert_eq!(
        view["elements"][0]["value"],
        "Unique needle in the final paragraph"
    );
    assert_eq!(
        view["semantics"]["query"],
        json!({"role":"static_text","text":"Unique needle"})
    );
    let absent = exec(
        &server,
        "absent",
        "await app.look({text:'does not exist'});",
    );
    assert!(!absent.is_error, "{}", absent.data);
    assert_eq!(
        absent.data["value"]["view"]["semantics"]["query"],
        json!({"text":"does not exist"})
    );
    assert_eq!(absent.data["value"]["view"]["semantics"]["complete"], true);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
}

#[test]
fn text_pages_are_requested_output_and_do_not_refresh_or_invent_input() {
    let (server, catalog) = agent();
    let text = format!("{}😀tail marker", "a".repeat(240));
    catalog.backend.0.model.lock().unwrap().semantic_elements = Some(vec![content(1, &text)]);
    let first = exec(
        &server,
        "page_one",
        "await app.look(); return await app.readText({target:'w1.e1',limit:241});",
    );
    assert!(!first.is_error, "{}", first.data);
    let first_page = &first.data["value"]["result"];
    assert_eq!(first_page["text"], "a".repeat(240));
    assert_eq!(first_page["next_offset"], 240);
    assert_eq!(first_page["unit"], "utf16");
    let second = crate::app_protocol::projection::execution(exec(
        &server,
        "page_two",
        "return await app.readText({target:'w1.e1',offset:240});",
    ));
    assert!(!second.is_error, "{}", second.data);
    assert_eq!(second.data["result"]["text"], "😀tail marker");
    assert!(second.data["result"]["next_offset"].is_null());
    assert!(second.data.get("view").is_none());
    assert_eq!(catalog.backend.0.observes.load(Ordering::SeqCst), 1);
    assert_eq!(catalog.backend.0.captures.load(Ordering::SeqCst), 0);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
    let whole = exec(&server, "whole", "return await app.readText('w1.e1');");
    assert_eq!(whole.data["value"]["result"]["text"], text);
}

#[test]
fn invalid_text_ranges_keep_the_original_binding_and_removed_ids_cannot_be_read() {
    let (server, catalog) = agent();
    catalog.backend.0.model.lock().unwrap().semantic_elements = Some(vec![content(1, "a😀b")]);
    let looked = exec(&server, "looked", "await app.look();");
    assert!(!looked.is_error);
    let invalid = exec(
        &server,
        "split_surrogate",
        "await app.readText({target:'w1.e1',offset:2});",
    );
    assert!(invalid.is_error);
    assert_eq!(invalid.data["error"]["code"], "invalid_text_range");
    let valid = exec(
        &server,
        "valid",
        "return await app.readText({target:'w1.e1',offset:1});",
    );
    assert!(!valid.is_error, "{}", valid.data);
    assert_eq!(valid.data["value"]["result"]["text"], "😀b");
    catalog.backend.0.model.lock().unwrap().semantic_elements =
        Some(vec![content(2, "replacement")]);
    exec(&server, "new_look", "await app.look();");
    let removed = exec(&server, "removed", "await app.readText('w1.e1');");
    assert!(removed.is_error);
    assert_eq!(removed.data["error"]["code"], "target_changed");
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
}

#[test]
fn a_later_text_failure_retires_pending_pixels_and_targets_until_a_fresh_look() {
    let (server, catalog) = agent();
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
        model.semantic_elements = Some(vec![content(1, &"a".repeat(300))]);
        model.capture_png = Some(image);
        model.inventory = Some(test_inventory(vec![WindowInfo {
            target: WindowTarget {
                app: application(),
                native_id: 101,
                generation: 3,
            },
            title: "Text document".into(),
            visible: true,
            scene: WindowLayout::new([0., 0., 20., 10.], 0),
        }]));
    }
    let first = exec(
        &server,
        "first_text_page",
        "await app.look(); return await app.readText({target:'w1.e1',limit:240});",
    );
    assert!(!first.is_error, "{}", first.data);
    assert_eq!(first.data["value"]["result"]["next_offset"], 240);
    catalog.backend.0.model.lock().unwrap().text_error = Some(Error::PermissionRequired {
        capability: "accessibility".into(),
        message: "controlled revocation".into(),
        host_name: None,
        host_bundle_id: None,
        restart_may_be_required: false,
    });
    let failed = exec(
        &server,
        "text_permission",
        "await app.capture(); await app.readText({target:'w1.e1',offset:240});",
    );
    assert!(failed.is_error, "{}", failed.data);
    assert!(failed.image.is_none());
    let compact = crate::app_protocol::projection::execution(failed);
    assert!(compact.data.get("view").is_none(), "{}", compact.data);
    let stale = exec(&server, "old_text", "await app.readText('w1.e1');");
    assert!(stale.is_error);
    let fresh = exec(
        &server,
        "fresh_text",
        "await app.look(); return await app.readText('w1.e1');",
    );
    assert!(!fresh.is_error, "{}", fresh.data);
    assert_eq!(fresh.data["value"]["result"]["text"], "a".repeat(300));
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
}

#[test]
fn filtered_target_renewal_keeps_the_native_query_before_dispatch() {
    let (server, catalog) = agent();
    {
        let mut model = catalog.backend.0.model.lock().unwrap();
        model.filtered_observation = true;
        let mut target = content(500, "Edit the requested row");
        target.role = "button".into();
        target.can_press = true;
        target.actions = vec!["press".into()];
        model.semantic_elements = Some(vec![target]);
    }
    let look = exec(
        &server,
        "filtered_target",
        "await app.look({role:'button',text:'requested row'});",
    );
    assert!(!look.is_error, "{}", look.data);
    let clicked = exec(&server, "filtered_click", "await app.click('w1.e1');");
    assert!(!clicked.is_error, "{}", clicked.data);
    let queries = catalog.backend.0.observe_queries.lock().unwrap();
    assert!(queries.len() >= 2);
    for query in &queries[..2] {
        assert_eq!(query.role.as_deref(), Some("button"));
        assert_eq!(query.text.as_deref(), Some("requested row"));
    }
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
}

#[test]
fn observation_filters_do_not_limit_text_input() {
    let (server, catalog) = agent();
    let source = format!(
        "await app.look(); await app.typeText({});",
        json!("多语言 paragraph 😀".repeat(100))
    );
    let reply = exec(&server, "long_input", &source);
    assert!(!reply.is_error, "{}", reply.data);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
}

#[test]
fn failed_text_read_after_input_does_not_repeat_readback_or_the_action() {
    let (server, catalog) = agent();
    let reply = exec(
        &server,
        "consumed_text",
        "await app.look(); await app.click('w1.e1'); await app.readText('w1.e1');",
    );
    assert!(reply.is_error, "{}", reply.data);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    // One explicit look and one credential renewal before the press. The failed
    // explicit text read must not trigger an implicit scene read at settlement.
    assert_eq!(catalog.backend.0.observes.load(Ordering::SeqCst), 2);
    let compact = crate::app_protocol::projection::execution(reply);
    assert!(compact.data.get("view").is_none());
    assert_eq!(compact.data["effects"]["dispatch"]["dispatched"], 1);
}

#[test]
fn query_and_text_arguments_reject_before_native_reads() {
    let (server, catalog) = agent();
    let reply = exec(
        &server,
        "arguments",
        r#"
        const codes=[];
        for (const read of [
            () => app.look({role:''}), () => app.look({text:1}),
            () => app.readText({target:'w1.e1',limit:1}),
            () => app.readText({target:'w1.e1',offset:-1}),
            () => app.readText({target:'w1.e1',window_ref:'native'}),
            () => app.capture({text:'not a capture option'})
        ]) { try { await read(); } catch(e) { codes.push(e.code); } }
        return codes;
    "#,
    );
    assert_eq!(success(&reply), &json!(vec!["invalid_arguments"; 6]));
    assert_eq!(catalog.backend.0.observes.load(Ordering::SeqCst), 0);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
}

fn queue_deep_target(model: &mut Model, last: ControlledSemanticPage) {
    model
        .semantic_pages
        .extend((0..7).map(|index| controlled_page(index, vec![], true)));
    model.semantic_pages.push_back(last);
}

fn deep_button() -> Element {
    let mut target = content(137, "Audit checkpoint 137");
    target.role = "button".into();
    target.can_press = true;
    target.actions = vec!["press".into()];
    target
}

#[test]
fn observed_id_renews_the_same_object_beyond_four_native_pages() {
    let (server, catalog) = agent();
    let target = deep_button();
    {
        let mut model = catalog.backend.0.model.lock().unwrap();
        model.filtered_observation = true;
        model.semantic_elements = Some(vec![target.clone()]);
        queue_deep_target(&mut model, controlled_page(7, vec![target.clone()], false));
    }
    let looked = exec(
        &server,
        "deep_object",
        "return await app.look({text:'Audit checkpoint 137'});",
    );
    assert!(!looked.is_error, "{}", looked.data);
    let id = looked.data["value"]["result"]["elements"][0]["id"].clone();
    assert!(id.is_string(), "{}", looked.data);
    let mut renewed = target;
    renewed.id = ElementId(80137);
    queue_deep_target(
        &mut catalog.backend.0.model.lock().unwrap(),
        controlled_page(7, vec![renewed], false),
    );
    let clicked = exec(
        &server,
        "deep_object_click",
        &format!("await app.click({{target:{id}}});"),
    );
    assert!(!clicked.is_error, "{}", clicked.data);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    let queries = catalog.backend.0.observe_queries.lock().unwrap();
    assert_eq!(queries[0].text.as_deref(), Some("Audit checkpoint 137"));
    assert_eq!(queries[1], queries[0]);
}

#[test]
fn deep_observed_id_never_dispatches_for_replaced_credentials_or_read_failures() {
    for read_failure in [false, true] {
        let (server, catalog) = agent();
        let target = deep_button();
        {
            let mut model = catalog.backend.0.model.lock().unwrap();
            model.filtered_observation = true;
            queue_deep_target(&mut model, controlled_page(7, vec![target.clone()], false));
        }
        let looked = exec(
            &server,
            "deep_reject_object",
            "return await app.look({text:'Audit checkpoint 137'});",
        );
        assert!(!looked.is_error, "{}", looked.data);
        let id = looked.data["value"]["result"]["elements"][0]["id"].clone();
        assert!(id.is_string(), "{}", looked.data);
        let last = if read_failure {
            Err(Error::NativeFailure {
                code: "ax_unavailable".into(),
                message: "controlled eighth-page renewal failure".into(),
            })
        } else {
            let mut replacement = target;
            replacement.rebind_identity = Some("retained:replacement".into());
            controlled_page(7, vec![replacement], false)
        };
        queue_deep_target(&mut catalog.backend.0.model.lock().unwrap(), last);
        let clicked = exec(
            &server,
            "deep_reject_click",
            &format!("await app.click({{target:{id}}});"),
        );
        assert!(clicked.is_error, "{}", clicked.data);
        assert_eq!(
            clicked.data["error"]["code"],
            if read_failure {
                "ax_unavailable"
            } else {
                "target_changed"
            }
        );
        assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
    }
}

fn controlled_page(index: u32, elements: Vec<Element>, has_more: bool) -> ControlledSemanticPage {
    Ok((
        elements,
        DiscoveryPage {
            has_more,
            complete: !has_more,
            reason: None,
            visited: index + 1,
        },
    ))
}

#[test]
fn filtered_look_continues_beyond_four_native_pages_to_the_match() {
    for query in ["{text:'Audit checkpoint 137'}", "{role:'static_text'}"] {
        let (server, catalog) = agent();
        {
            let mut model = catalog.backend.0.model.lock().unwrap();
            model.filtered_observation = true;
            model.semantic_pages = (0..7)
                .map(|index| controlled_page(index, vec![], true))
                .collect();
            model.semantic_pages.push_back(controlled_page(
                7,
                vec![content(137, "Audit checkpoint 137")],
                false,
            ));
        }
        let reply = exec(
            &server,
            "deep_filtered_look",
            &format!("await app.look({query});"),
        );
        assert!(!reply.is_error, "{}", reply.data);
        let view = &reply.data["value"]["view"];
        assert_eq!(
            view["elements"][0]["value"], "Audit checkpoint 137",
            "{view}"
        );
        assert_eq!(view["semantics"]["complete"], true);
        assert_eq!(view["semantics"]["has_more"], false);
        assert_eq!(catalog.backend.0.observes.load(Ordering::SeqCst), 8);
        assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn look_native_page_limits_distinguish_explicit_filters_from_scope_only() {
    for (query, expected_pages) in [("{}", 4), ("{scope:'window'}", 4), ("{text:'absent'}", 32)] {
        let (server, catalog) = agent();
        {
            let mut model = catalog.backend.0.model.lock().unwrap();
            model.filtered_observation = true;
            model.semantic_pages = (0..40)
                .map(|index| controlled_page(index, vec![], true))
                .collect();
        }
        let reply = exec(
            &server,
            "bounded_look",
            &format!("await app.look({query});"),
        );
        assert!(!reply.is_error, "{}", reply.data);
        let view = &reply.data["value"]["view"];
        assert_eq!(view["elements"], json!([]));
        assert_eq!(view["semantics"]["complete"], false);
        assert_eq!(view["semantics"]["has_more"], true);
        assert_eq!(
            catalog.backend.0.observes.load(Ordering::SeqCst),
            expected_pages
        );
    }
}

#[test]
fn filtered_look_late_page_failure_discards_candidates_and_previous_targets() {
    for code in ["ax_unavailable", "permission_required"] {
        let (server, catalog) = agent();
        let first = exec(&server, "baseline_target", "await app.look();");
        assert!(!first.is_error, "{}", first.data);
        {
            let mut model = catalog.backend.0.model.lock().unwrap();
            model.filtered_observation = true;
            model.semantic_pages.push_back(controlled_page(
                0,
                vec![content(137, "Audit checkpoint 137")],
                true,
            ));
            model
                .semantic_pages
                .extend((1..5).map(|index| controlled_page(index, vec![], true)));
            model.semantic_pages.push_back(Err(Error::NativeFailure {
                code: code.into(),
                message: "controlled late native read failure".into(),
            }));
        }
        let reply = exec(
            &server,
            "failed_deep_look",
            "return await app.look({text:'Audit checkpoint 137'});",
        );
        if code == "ax_unavailable" {
            assert!(!reply.is_error, "{}", reply.data);
            let view = &reply.data["value"]["result"];
            assert_eq!(view["elements"], json!([]));
            assert_eq!(view["semantics"]["complete"], false);
            assert_eq!(view["semantics"]["reason"], code);
        } else {
            assert!(reply.is_error, "{}", reply.data);
            assert_eq!(reply.data["error"]["code"], code);
        }
        assert_eq!(catalog.backend.0.observes.load(Ordering::SeqCst), 7);
        let stale = exec(&server, "retired_target", "await app.readText('w1.e1');");
        assert!(stale.is_error, "{}", stale.data);
        assert_eq!(stale.data["error"]["code"], "target_changed");
        assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
    }
}
