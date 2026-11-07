use super::*;
use crate::app_protocol::server::{Application, Factory, Server, Started};

// The real two-tool facade must admit fresh PNGs when a semantic page fails.
#[derive(Clone)]
struct SceneCatalog(PagedBackend);
impl Factory for SceneCatalog {
    type Provider = PagedBackend;
    fn catalog(&self) -> Result<Vec<Application>, Error> {
        Ok(vec![Application {
            app_id: "org.example.Paged".into(),
            app: application(),
            name: "Paged".into(),
        }])
    }
    fn connect(&self) -> Result<PagedBackend, Error> {
        Ok(self.0.clone())
    }
    fn close_catalog(&self) -> Result<(), Error> {
        Ok(())
    }
}
fn scene_agent() -> (Arc<Server<SceneCatalog>>, PagedBackend) {
    let (_, backend) = setup();
    backend.base.0.model.lock().unwrap().inventory = Some(test_inventory(vec![WindowInfo {
        target: WindowTarget {
            app: application(),
            native_id: 101,
            generation: 3,
        },
        title: "WindowServer surface".into(),
        visible: true,
        scene: WindowLayout::new([0., 0., 20., 10.], 0),
    }]));
    (Server::new(SceneCatalog(backend.clone())), backend)
}
fn scene_exec(server: &Arc<Server<SceneCatalog>>, source: &str) -> Reply {
    match server.begin(
        json!(uuid::Uuid::new_v4().to_string()),
        "exec",
        json!({"app":"org.example.Paged", "source":source}),
    ) {
        Started::Reply(reply) => reply,
        Started::Call(call) => server.run(&call),
        _ => panic!("Unexpected in-flight scene request"),
    }
}

#[test]
fn app_view_renews_a_departing_surface_without_replaying_input() {
    let (server, backend) = scene_agent();
    backend
        .base
        .0
        .model
        .lock()
        .unwrap()
        .window_error_after_action = Some(Error::TargetChanged);
    let reply = scene_exec(&server, "await app.capture(); await app.click({x:5,y:5});");
    assert!(!reply.is_error, "{}", reply.data);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);

    for failures in [1, 2] {
        let (server, backend) = scene_agent();
        for _ in 0..failures {
            backend
                .base
                .0
                .model
                .lock()
                .unwrap()
                .window_errors
                .push_back(Error::TargetChanged);
        }
        let reply = scene_exec(&server, "return await app.look();");
        assert_eq!(reply.is_error, failures == 2, "{}", reply.data);
        assert_eq!(backend.base.0.windows.load(Ordering::SeqCst), 2);
        assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
    }
    for cause in [
        Error::NativeFailure {
            code: "app_changed".into(),
            message: "Process incarnation changed".into(),
        },
        Error::WrongApp,
        Error::WrongProvider,
        Error::NativeFailure {
            code: "permission_denied".into(),
            message: "Permission was revoked".into(),
        },
    ] {
        let (server, backend) = scene_agent();
        backend
            .base
            .0
            .model
            .lock()
            .unwrap()
            .window_errors
            .push_back(cause);
        let reply = scene_exec(&server, "return await app.look();");
        assert!(reply.is_error, "{}", reply.data);
        assert_eq!(backend.base.0.windows.load(Ordering::SeqCst), 1);
    }
}
#[test]
fn a_transient_preflight_capture_refreshes_reads_once_before_any_input() {
    for failures in [1, 2] {
        let (server, backend) = scene_agent();
        let captured = scene_exec(&server, "await app.capture();");
        assert!(!captured.is_error, "{}", captured.data);
        for _ in 0..failures {
            backend
                .base
                .0
                .model
                .lock()
                .unwrap()
                .capture_errors
                .push_back(Error::NativeFailure {
                    code: "scene_changed".into(),
                    message: "popup joined capture".into(),
                });
        }
        let clicked = scene_exec(&server, "await app.click({x:5,y:5});");
        assert_eq!(clicked.is_error, failures == 2, "{}", clicked.data);
        assert_eq!(
            backend.base.0.actions.load(Ordering::SeqCst),
            usize::from(failures == 1)
        );
        // Initial capture, at most two preflight reads and one final observation.
        assert_eq!(backend.base.0.captures.load(Ordering::SeqCst), 4);
    }
}

#[test]
fn decorations_remain_in_pixels_and_inventory_but_not_active_semantics() {
    let mut reference_image = None;
    for candidate in [None, Some(true), Some(false)] {
        let (server, backend) = scene_agent();
        let rows = [
            (102, "Popup", [-5., 0., 10., 5.], candidate),
            (103, "Shadow", [-4., 1., 10., 5.], Some(false)),
            (101, "Open dialog", [0., 0., 20., 10.], None),
        ]
        .into_iter()
        .enumerate()
        .map(|(rank, (native_id, title, bounds, hint))| {
            let mut scene = WindowLayout::new(bounds, rank as u32).unwrap();
            scene.semantic_candidate = hint;
            scene.keyboard_focused = Some(native_id == 101);
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
        backend.base.0.model.lock().unwrap().inventory = Some(test_inventory(rows));
        let reply = scene_exec(&server, "await app.capture();");
        assert!(!reply.is_error, "{}", reply.data);
        let view = &reply.data["value"]["view"];
        let prefix = if candidate == Some(false) {
            "w3."
        } else {
            "w1."
        };
        let elements = view["elements"].as_array().unwrap();
        assert!(!elements.is_empty());
        assert!(
            elements
                .iter()
                .all(|element| element["id"].as_str().unwrap().starts_with(prefix)),
            "{view}"
        );
        assert_eq!(
            view["windows"]
                .as_array()
                .unwrap()
                .iter()
                .map(|w| w["title"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["Popup", "Shadow", "Open dialog"]
        );
        assert_eq!(view["image"]["bounds"], json!([-5.0, 0.0, 25.0, 10.0]));
        assert_eq!(backend.base.0.captures.load(Ordering::SeqCst), 3);
        assert!(!view.to_string().contains("semantic_candidate"));
        let image = reply.image.unwrap();
        if let Some(reference) = &reference_image {
            assert_eq!(
                &image, reference,
                "Semantic eligibility cannot change composition"
            );
        } else {
            use base64::Engine;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(image.as_str())
                .unwrap();
            let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes))
                .read_info()
                .unwrap();
            let mut rgba = vec![0; decoder.output_buffer_size().unwrap()];
            decoder.next_frame(&mut rgba).unwrap();
            // This pixel is left of the dialog and is supplied only by Popup.
            assert_eq!(&rgba[..4], &[255, 255, 255, 255]);
            reference_image = Some(image);
        }
    }
}

fn ax_parent_failure() -> Error {
    Error::NativeFailure {
        code: "ax_unavailable".into(),
        message: "AX attribute AXParent failed: -25200".into(),
    }
}
#[test]
fn scene_semantic_failure_on_first_or_later_page_falls_back_to_usable_pixels() {
    for (first_page, cause) in [
        (true, ax_parent_failure()),
        (false, ax_parent_failure()),
        (true, Error::BackendFailed("UIA root unavailable".into())),
        (
            false,
            Error::BackendFailed("UIA continuation unavailable".into()),
        ),
    ] {
        let reason = wire::core_error(&cause).code;
        let (server, backend) = scene_agent();
        let mut failures = backend.semantic_errors.lock().unwrap();
        if !first_page {
            failures.push_back(None);
        }
        failures.push_back(Some(cause));
        drop(failures);
        let reply = scene_exec(&server, "return await app.look();");
        assert!(!reply.is_error, "{}", reply.data);
        let view = &reply.data["value"]["view"];
        assert_eq!(view["semantics"]["reason"], reason);
        assert_eq!(view["semantics"]["complete"], false);
        assert_eq!(view["elements"], json!([]));
        assert!(view["image"]["error"].is_null(), "{view}");
        assert!(reply.image.is_some());
        assert_eq!(
            backend.advances.load(Ordering::SeqCst),
            usize::from(!first_page)
        );
        let clicked = scene_exec(&server, "await app.click({x:5,y:5});");
        assert!(!clicked.is_error, "{}", clicked.data);
        assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);
    }
}
#[test]
fn scene_semantic_hard_errors_never_degrade_or_capture_on_any_page() {
    for code in [
        "permission_required",
        "app_changed",
        "cancelled",
        "native_stopping",
        "timed_out",
        "deadline_exceeded",
        "invalid_observation",
    ] {
        for first_page in [true, false] {
            let (server, backend) = scene_agent();
            let mut failures = backend.semantic_errors.lock().unwrap();
            if !first_page {
                failures.push_back(None);
            }
            failures.push_back(Some(Error::NativeFailure {
                code: code.into(),
                message: "original hard failure".into(),
            }));
            drop(failures);
            let reply = scene_exec(&server, "await app.capture();");
            assert!(reply.is_error, "{}", reply.data);
            assert_eq!(reply.data["script"]["failure"]["code"], code);
            assert_eq!(backend.base.0.captures.load(Ordering::SeqCst), 0);
            assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
        }
    }
}
#[test]
fn scene_capture_preserves_permission_details_and_other_hard_failures() {
    for error in [
        Error::PermissionRequired {
            capability: "screen_recording".into(),
            message: "original capture permission".into(),
            host_name: Some("Test Host".into()),
            host_bundle_id: Some("org.example.Host".into()),
            restart_may_be_required: true,
        },
        Error::NativeFailure {
            code: "cancelled".into(),
            message: "original capture cancellation".into(),
        },
        Error::NativeFailure {
            code: "app_changed".into(),
            message: "original capture identity failure".into(),
        },
    ] {
        let expected = wire::core_error(&error);
        let (server, backend) = scene_agent();
        backend
            .base
            .0
            .model
            .lock()
            .unwrap()
            .capture_errors
            .push_back(error);
        let reply = scene_exec(&server, "await app.capture();");
        assert!(reply.is_error, "{}", reply.data);
        let failure = &reply.data["script"]["failure"];
        assert_eq!(failure["code"], expected.code);
        assert!(failure["message"]
            .as_str()
            .unwrap()
            .contains(&expected.message));
        assert_eq!(
            reply.data["script"]["steps"][0]["error"].get("details"),
            expected.details.as_ref()
        );
        assert_eq!(backend.base.0.captures.load(Ordering::SeqCst), 1);
        assert!(reply.image.is_none());
    }
}
#[test]
fn scene_failed_refresh_revokes_previous_element_and_pixel_bindings() {
    let (server, backend) = scene_agent();
    let seed = scene_exec(&server, "await app.capture();");
    assert!(!seed.is_error && seed.image.is_some(), "{}", seed.data);
    let target = seed.data["value"]["view"]["elements"][0]["id"]
        .as_str()
        .unwrap();
    backend.base.0.model.lock().unwrap().windows_error = Some(Error::NativeFailure {
        code: "windows_unavailable".into(),
        message: "fresh WindowServer read unavailable".into(),
    });
    let failed = scene_exec(&server, "await app.look();");
    assert!(failed.is_error);
    let clicked = scene_exec(
        &server,
        &format!("await app.click({{target:{}}});", json!(target)),
    );
    assert!(clicked.is_error, "{}", clicked.data);
    assert_eq!(clicked.data["script"]["failure"]["code"], "target_changed");
    let clicked = scene_exec(&server, "await app.click({x:5,y:5});");
    assert!(clicked.is_error, "{}", clicked.data);
    assert_eq!(
        clicked.data["script"]["failure"]["code"],
        "capture_required"
    );
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
}

#[test]
fn scene_optional_inventory_gaps_stay_visible_without_blocking_pixels() {
    let (server, backend) = scene_agent();
    {
        let mut model = backend.base.0.model.lock().unwrap();
        let report = &mut model.inventory.as_mut().unwrap().report;
        report.scope = InventoryScope::ApplicationAndProvenAssociatedSurfaces {
            associated_count: 0,
        };
        report.diagnostics.error_codes = vec![-25200];
    }
    let reply = scene_exec(&server, "await app.capture();");
    assert!(!reply.is_error && reply.image.is_some(), "{}", reply.data);
    assert_eq!(
        reply.data["value"]["view"]["coverage"]["read_error_codes"],
        json!([-25200])
    );
    assert_eq!(
        reply.data["value"]["view"]["coverage"]["association_complete"],
        false
    );
    let clicked = scene_exec(&server, "await app.click({x:5,y:5});");
    assert!(!clicked.is_error, "{}", clicked.data);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);
}
#[test]
fn scene_post_dispatch_read_failure_keeps_receipt_and_never_replays_input() {
    for hard in [false, true] {
        let (server, backend) = scene_agent();
        let seed = scene_exec(&server, "await app.capture();");
        assert!(!seed.is_error && seed.image.is_some(), "{}", seed.data);
        if hard {
            backend
                .base
                .0
                .model
                .lock()
                .unwrap()
                .capture_error_after_action = Some(Error::NativeFailure {
                code: "permission_required".into(),
                message: "permission revoked after dispatch".into(),
            });
        } else {
            backend
                .semantic_errors
                .lock()
                .unwrap()
                .extend([None, Some(ax_parent_failure())]);
        }
        let reply = scene_exec(&server, "return await app.click({x:5,y:5});");
        assert!(!reply.is_error, "{}", reply.data);
        assert_eq!(
            reply.data["script"]["steps"][0]["receipt"]["outcome"]["dispatch"],
            "dispatched"
        );
        assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);
        assert_eq!(reply.data["script"]["steps"].as_array().unwrap().len(), 2);
        assert_eq!(backend.base.0.captures.load(Ordering::SeqCst), 3);
        if hard {
            assert_eq!(
                reply.data["value"]["observation"]["error"]["code"],
                "permission_required"
            );
            assert!(reply.data["script"]["failure"].is_null());
            assert!(reply.image.is_none());
        } else {
            assert_eq!(
                reply.data["value"]["view"]["semantics"]["reason"],
                "ax_unavailable"
            );
            assert!(reply.image.is_some());
        }
        let projected = crate::app_protocol::projection::execution(reply);
        assert_eq!(projected.data["effects"]["dispatch"]["dispatched"], 1);
        assert_eq!(
            projected.data["result"]["outcome"]["dispatch"],
            "dispatched"
        );
        assert_eq!(
            projected.data["observation"]["status"],
            if hard { "failed" } else { "observed" }
        );
        if hard {
            backend
                .base
                .0
                .model
                .lock()
                .unwrap()
                .capture_error_after_action = None;
            let recovered = scene_exec(&server, "await app.capture();");
            assert!(
                !recovered.is_error && recovered.image.is_some(),
                "{}",
                recovered.data
            );
            assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);
            let next = scene_exec(&server, "await app.click({x:6,y:5});");
            assert!(!next.is_error, "{}", next.data);
            assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 2);
        }
    }
}

#[test]
fn automatic_observation_failure_does_not_hide_a_script_error() {
    let (server, backend) = scene_agent();
    assert!(scene_exec(&server, "await app.capture();").image.is_some());
    backend
        .base
        .0
        .model
        .lock()
        .unwrap()
        .capture_error_after_action = Some(Error::BackendFailed("render unavailable".into()));
    let reply = scene_exec(
        &server,
        "await app.click({x:5,y:5}); throw Error('stop here');",
    );
    let projected = crate::app_protocol::projection::execution(reply);
    assert!(projected.is_error, "{}", projected.data);
    assert_eq!(projected.data["error"]["code"], "script_exception");
    assert_eq!(projected.data["effects"]["dispatch"]["dispatched"], 1);
    assert_eq!(
        projected.data["observation"]["error"]["code"],
        "backend_failed"
    );
    assert!(projected.image.is_none());
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);
}

#[test]
fn scene_explicit_failed_read_after_input_is_not_repeated_at_settlement() {
    let (server, backend) = scene_agent();
    let seed = scene_exec(&server, "await app.capture();");
    assert!(!seed.is_error && seed.image.is_some(), "{}", seed.data);
    backend
        .base
        .0
        .model
        .lock()
        .unwrap()
        .capture_error_after_action = Some(Error::NativeFailure {
        code: "permission_required".into(),
        message: "original explicit read failure".into(),
    });
    let reply = scene_exec(&server, "await app.click({x:5,y:5}); await app.capture();");
    assert!(reply.is_error, "{}", reply.data);
    assert_eq!(
        reply.data["script"]["failure"]["code"],
        "permission_required"
    );
    assert_eq!(reply.data["script"]["steps"].as_array().unwrap().len(), 2);
    assert_eq!(backend.base.0.captures.load(Ordering::SeqCst), 3);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);
    assert!(reply.image.is_none());
}

#[test]
fn coordinate_gestures_renew_bindings_without_explicit_recapture() {
    let (server, backend) = scene_agent();
    let reply = scene_exec(&server, "await app.capture(); await app.click({x:5,y:5}); await app.click({x:6,y:5}); await app.move({x:5,y:5}); await app.drag({path:[{x:4,y:5},{x:8,y:5}]}); await app.scroll({x:5,y:5,delta_y:3});");
    assert!(!reply.is_error, "{}", reply.data);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 5);
}

#[test]
fn explicit_capture_survives_semantic_reads_in_the_same_script() {
    let (server, backend) = scene_agent();
    let reply = scene_exec(&server, "await app.capture(); return await app.look();");
    assert!(!reply.is_error, "{}", reply.data);
    assert!(
        reply.image.is_some(),
        "The explicitly requested PNG was discarded"
    );
    assert_eq!(backend.base.0.captures.load(Ordering::SeqCst), 1);
    assert_eq!(reply.data["value"]["last_capture"]["step"], 1);
    let compact = crate::app_protocol::projection::execution(reply);
    assert_eq!(compact.data["view"]["image"]["captured_at_call"], 1);
    assert!(compact.data["view"]["image"]["width"].as_u64().is_some());
}

#[test]
fn semantic_readbacks_keep_the_captured_mapping_without_automatic_images() {
    let (server, backend) = scene_agent();
    let capture = scene_exec(&server, "await app.capture();");
    assert!(
        !capture.is_error && capture.image.is_some(),
        "{}",
        capture.data
    );
    for _ in 0..2 {
        let read = scene_exec(&server, "await app.look();");
        assert!(!read.is_error && read.image.is_none(), "{}", read.data);
        let input = scene_exec(&server, "await app.drag({path:[{x:4,y:5},{x:8,y:5}]});");
        assert!(!input.is_error && input.image.is_some(), "{}", input.data);
    }
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 2);
}

#[test]
fn coordinate_preflight_accepts_animation_but_rejects_changed_geometry() {
    for geometry in [false, true] {
        let (server, backend) = scene_agent();
        let seen = scene_exec(&server, "await app.capture();");
        assert!(!seen.is_error, "{}", seen.data);
        {
            let mut model = backend.base.0.model.lock().unwrap();
            if geometry {
                model.inventory.as_mut().unwrap().windows[0].scene =
                    WindowLayout::new([1., 0., 20., 10.], 0);
            } else {
                model.capture_rgba = [0, 0, 0, 255];
            }
        }
        let read = scene_exec(&server, "await app.look();");
        assert!(!read.is_error && read.image.is_none(), "{}", read.data);
        let reply = scene_exec(&server, "await app.click({x:5,y:5});");
        assert_eq!(reply.is_error, geometry, "{}", reply.data);
        if geometry {
            assert_eq!(
                reply.data["script"]["failure"]["code"], "stale_scene",
                "{}",
                reply.data
            );
        }
        assert_eq!(
            backend.base.0.actions.load(Ordering::SeqCst),
            usize::from(!geometry)
        );
    }
}

#[test]
fn exact_native_target_does_not_require_discovery_of_unrelated_branches() {
    let (server, backend) = scene_agent();
    backend.complete_after_first.store(true, Ordering::SeqCst);
    backend
        .queries
        .lock()
        .unwrap()
        .push(ObserveQuery::default());
    let seed = scene_exec(&server, "await app.look();");
    assert!(!seed.is_error, "{}", seed.data);
    backend.complete_after_first.store(false, Ordering::SeqCst);
    backend
        .semantic_errors
        .lock()
        .unwrap()
        .extend([None, Some(ax_parent_failure())]);
    let reply = scene_exec(&server, "await app.click('w1.e1');");
    assert!(!reply.is_error, "{}", reply.data);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(backend.advances.load(Ordering::SeqCst), 0);
}

#[test]
fn new_same_instance_overlay_receives_current_canvas_coordinates() {
    for covers_point in [false, true] {
        let (server, backend) = scene_agent();
        let seed = scene_exec(&server, "await app.capture();");
        assert!(!seed.is_error, "{}", seed.data);
        let mut model = backend.base.0.model.lock().unwrap();
        let inventory = model.inventory.as_mut().unwrap();
        inventory.windows[0].scene.as_mut().unwrap().rank = 1;
        let mut overlay = inventory.windows[0].clone();
        overlay.target.native_id = 102;
        overlay.scene = WindowLayout::new([if covers_point { 0. } else { 15. }, 0., 5., 5.], 0);
        inventory.windows.insert(0, overlay);
        inventory.report.candidate_count += 1;
        inventory.report.supported_count += 1;
        drop(model);
        let reply = scene_exec(&server, "await app.click({x:5,y:5});");
        assert!(!reply.is_error, "{}", reply.data);
        assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn private_pixel_refresh_routes_later_points_to_current_same_instance_overlay() {
    for next in [
        "await app.click({x:35,y:5});",
        "await app.move({x:35,y:5});",
        "await app.drag({path:[{x:32,y:5},{x:36,y:5}]});",
        "await app.scroll({x:35,y:5,delta_y:3});",
    ] {
        let (server, backend) = scene_agent();
        let seed = scene_exec(&server, "await app.capture();");
        assert!(!seed.is_error, "{}", seed.data);
        let mut model = backend.base.0.model.lock().unwrap();
        let inventory = model.inventory.as_mut().unwrap();
        inventory.windows[0].scene.as_mut().unwrap().rank = 1;
        let mut overlay = inventory.windows[0].clone();
        overlay.target.native_id = 102;
        overlay.scene = WindowLayout::new([15., 0., 5., 5.], 0);
        inventory.windows.insert(0, overlay);
        inventory.report.candidate_count += 1;
        inventory.report.supported_count += 1;
        drop(model);
        let reply = scene_exec(&server, &format!("await app.click({{x:5,y:5}}); {next}"));
        assert!(!reply.is_error, "{next}: {}", reply.data);
        assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 2);
    }
}

#[test]
fn closing_observed_popup_routes_canvas_point_back_to_base() {
    let (server, backend) = scene_agent();
    {
        let mut model = backend.base.0.model.lock().unwrap();
        let base = model.inventory.as_ref().unwrap().windows[0].clone();
        let mut popup = base.clone();
        popup.target.native_id = 102;
        popup.scene = WindowLayout::new([0., 0., 5., 5.], 0);
        let mut base = base;
        base.scene.as_mut().unwrap().rank = 1;
        model.inventory = Some(test_inventory(vec![popup, base]));
    }
    assert!(!scene_exec(&server, "await app.capture();").is_error);
    {
        let mut model = backend.base.0.model.lock().unwrap();
        let mut base = model.inventory.as_ref().unwrap().windows[1].clone();
        base.scene.as_mut().unwrap().rank = 0;
        model.inventory = Some(test_inventory(vec![base]));
    }
    let reply = scene_exec(
        &server,
        "await app.click({x:5,y:5}); await app.click({x:5,y:5});",
    );
    assert!(!reply.is_error, "{}", reply.data);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 2);
}

#[test]
fn moving_internal_popup_preserves_unaffected_base_canvas_input() {
    let (server, backend) = scene_agent();
    {
        let mut model = backend.base.0.model.lock().unwrap();
        let mut base = model.inventory.as_ref().unwrap().windows[0].clone();
        base.scene.as_mut().unwrap().rank = 1;
        let mut popup = base.clone();
        popup.target.native_id = 102;
        popup.scene = WindowLayout::new([10., 0., 5., 5.], 0);
        model.inventory = Some(test_inventory(vec![popup, base]));
    }
    assert!(!scene_exec(&server, "await app.capture();").is_error);
    backend
        .base
        .0
        .model
        .lock()
        .unwrap()
        .inventory
        .as_mut()
        .unwrap()
        .windows[0]
        .scene = WindowLayout::new([15., 0., 5., 5.], 0);
    let reply = scene_exec(&server, "await app.click({x:5,y:5});");
    assert!(!reply.is_error, "{}", reply.data);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);
}

#[test]
fn canvas_refresh_rejects_transparent_current_receiver() {
    let (server, backend) = scene_agent();
    assert!(!scene_exec(&server, "await app.capture();").is_error);
    backend.base.0.model.lock().unwrap().capture_rgba = [0; 4];
    let reply = scene_exec(&server, "await app.click({x:5,y:5});");
    assert!(reply.is_error, "{}", reply.data);
    assert_eq!(reply.data["script"]["failure"]["code"], "outside_app");
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
}

#[test]
fn canvas_refresh_rejects_unproven_foreign_surface() {
    let (server, backend) = scene_agent();
    assert!(!scene_exec(&server, "await app.capture();").is_error);
    {
        let mut model = backend.base.0.model.lock().unwrap();
        let mut foreign = model.inventory.as_ref().unwrap().windows[0].clone();
        foreign.target.native_id = 102;
        foreign.target.app.pid += 1;
        model.inventory.as_mut().unwrap().windows.insert(0, foreign);
    }
    let reply = scene_exec(&server, "await app.click({x:5,y:5});");
    assert!(reply.is_error, "{}", reply.data);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
}

#[test]
fn canvas_refresh_rejects_changed_mapping_when_anchor_disappears() {
    let (server, backend) = scene_agent();
    assert!(!scene_exec(&server, "await app.capture();").is_error);
    {
        let mut model = backend.base.0.model.lock().unwrap();
        let base = &mut model.inventory.as_mut().unwrap().windows[0];
        base.target.native_id = 102;
        base.scene = WindowLayout::new([0., 0., 40., 20.], 0);
    }
    let reply = scene_exec(&server, "await app.click({x:5,y:5});");
    assert!(reply.is_error, "{}", reply.data);
    assert_eq!(reply.data["script"]["failure"]["code"], "stale_scene");
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
}

#[test]
fn new_popup_outside_canvas_requires_a_new_mapping() {
    let (server, backend) = scene_agent();
    assert!(!scene_exec(&server, "await app.capture();").is_error);
    {
        let mut model = backend.base.0.model.lock().unwrap();
        let mut base = model.inventory.as_ref().unwrap().windows[0].clone();
        base.scene.as_mut().unwrap().rank = 1;
        let mut popup = base.clone();
        popup.target.native_id = 102;
        popup.scene = WindowLayout::new([15., 0., 10., 5.], 0);
        model.inventory = Some(test_inventory(vec![popup, base]));
    }
    let reply = scene_exec(&server, "await app.click({x:5,y:5});");
    assert!(reply.is_error, "{}", reply.data);
    assert_eq!(reply.data["script"]["failure"]["code"], "stale_scene");
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
}

#[test]
fn canvas_input_with_unknown_effect_is_never_replayed() {
    let (server, backend) = scene_agent();
    assert!(!scene_exec(&server, "await app.capture();").is_error);
    backend.base.0.model.lock().unwrap().outcome =
        ActionOutcome::unknown(Error::BackendFailed("interrupted".into()));
    let reply = scene_exec(
        &server,
        "await app.click({x:5,y:5}); await app.click({x:5,y:5});",
    );
    assert!(reply.is_error, "{}", reply.data);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);
}

#[test]
fn failed_second_read_never_publishes_the_first_read_as_current() {
    for caught in [false, true] {
        let (server, backend) = scene_agent();
        backend.base.0.model.lock().unwrap().capture_error_at = Some((
            2,
            Error::NativeFailure {
                code: "permission_required".into(),
                message: "Screen Recording revoked before second capture".into(),
            },
        ));
        let source = if caught {
            "await app.capture(); try { await app.capture(); } catch {}"
        } else {
            "await app.capture(); await app.capture();"
        };
        let reply = scene_exec(&server, source);
        assert_eq!(reply.is_error, !caught, "{}", reply.data);
        assert!(reply.image.is_none());
        assert!(reply.data["value"]["view"].is_null(), "{}", reply.data);
        assert!(reply.data["value"]["snapshot"].is_null());
        assert_eq!(backend.base.0.captures.load(Ordering::SeqCst), 2);
        let projected = crate::app_protocol::projection::execution(reply);
        assert!(projected.data.get("view").is_none(), "{}", projected.data);
    }
}

#[test]
fn worker_error_roundtrip_preserves_semantic_fallback_and_hard_failures() {
    use crate::backends::error_wire::{error_reply, reply_error};
    for cause in [
        Error::BackendFailed("ElementFromHandle failed: UIA root unavailable".into()),
        Error::NativeFailure {
            code: "semantic_window_unavailable".into(),
            message: "UIA root unavailable".into(),
        },
        Error::NativeFailure {
            code: "native_failure".into(),
            message: "unclassified worker fault".into(),
        },
        Error::NativeFailure {
            code: "permission_required".into(),
            message: "UIA access denied".into(),
        },
        Error::WrongApp,
        Error::WrongProvider,
        Error::InvalidObservation,
        Error::Cancelled,
    ] {
        // Use the production worker encoder, JSON wire bytes and client decoder,
        // rather than injecting the original Rust error into the facade.
        let bytes = serde_json::to_vec(&error_reply(cause.clone(), false)).unwrap();
        let packet: Value = serde_json::from_slice(&bytes).unwrap();
        let decoded = reply_error(&packet).unwrap();
        let can_capture = matches!(cause, Error::BackendFailed(_))
            || matches!(&cause, Error::NativeFailure { code, .. } if code == "semantic_window_unavailable");
        let (server, backend) = scene_agent();
        backend
            .semantic_errors
            .lock()
            .unwrap()
            .push_back(Some(decoded));
        let reply = scene_exec(&server, "await app.capture();");
        assert_eq!(reply.is_error, !can_capture, "{cause:?}: {}", reply.data);
        assert_eq!(reply.image.is_some(), can_capture);
        assert_eq!(
            backend.base.0.captures.load(Ordering::SeqCst),
            usize::from(can_capture)
        );
    }
}
