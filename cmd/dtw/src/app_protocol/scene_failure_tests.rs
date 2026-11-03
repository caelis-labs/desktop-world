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
                .capture_errors
                .push_back(Error::NativeFailure {
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
        let reply = scene_exec(&server, "await app.click({x:5,y:5});");
        assert_eq!(reply.is_error, hard, "{}", reply.data);
        assert_eq!(
            reply.data["script"]["steps"][0]["receipt"]["outcome"]["dispatch"],
            "dispatched"
        );
        assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);
        assert_eq!(reply.data["script"]["steps"].as_array().unwrap().len(), 2);
        assert_eq!(backend.base.0.captures.load(Ordering::SeqCst), 2);
        if hard {
            assert_eq!(
                reply.data["script"]["failure"]["code"],
                "permission_required"
            );
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
    }
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
        .capture_errors
        .push_back(Error::NativeFailure {
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
    assert_eq!(backend.base.0.captures.load(Ordering::SeqCst), 2);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);
    assert!(reply.image.is_none());
}
