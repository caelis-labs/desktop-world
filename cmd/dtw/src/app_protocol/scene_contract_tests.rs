//! Audit regressions through the production app facade.
use super::*;

struct InterruptedResolver {
    catalog: Catalog,
    gate: Mutex<Option<WorkerGate>>,
}
impl Factory for InterruptedResolver {
    type Provider = MockBackend;
    fn catalog(&self) -> Result<Vec<Application>, Error> {
        self.catalog.catalog()
    }
    fn connect(&self) -> Result<MockBackend, Error> {
        self.catalog.connect()
    }
    fn close_catalog(&self) -> Result<(), Error> {
        Ok(())
    }
    fn resolve(&self, _: &str) -> Result<ApplicationTarget, Error> {
        self.gate.lock().unwrap().take().unwrap().wait();
        Err(Error::BackendFailed(
            "Stopped (request may have been sent: true)".into(),
        ))
    }
    fn description(&self) -> Value {
        json!({"provider":"controlled", "coverage":"bounded windows", "conditions":["owned surface"],
            "supported_capabilities":["observe","press"], "supported_observe_scopes":["window"],
            "private_diagnostic":"not public"})
    }
}

#[test]
fn resolution_cancellation_preserves_diagnostic_and_never_connects_or_replays() {
    let (_, catalog) = agent();
    let (worker_gate, gate) = TestGate::pair();
    let server = Server::new(InterruptedResolver {
        catalog: catalog.clone(),
        gate: Mutex::new(Some(worker_gate)),
    });
    let args = input("return 1");
    let Started::Call(active) = server.begin(json!("resolve"), "exec", args.clone()) else {
        panic!("not admitted")
    };
    let running = server.clone();
    let worker = std::thread::spawn(move || running.run(&active));
    gate.entered();
    server.cancel(&json!("resolve"));
    gate.release();
    let reply = worker.join().unwrap();
    assert!(reply.is_error);
    assert_eq!(reply.data["error"]["code"], "cancelled");
    assert_eq!(
        reply.data["error"]["details"]["cause"]["code"],
        "backend_failed"
    );
    assert_eq!(
        reply.data["error"]["details"]["cause"]["message"],
        "Stopped (request may have been sent: true)"
    );
    let Started::Reply(duplicate) = server.begin(json!("resolve"), "exec", args) else {
        panic!("replayed")
    };
    assert_eq!(duplicate.data, reply.data);
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 0);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
}

#[test]
fn apps_omits_static_provider_metadata_and_does_not_connect() {
    let (_, catalog) = agent();
    let server = Server::new(InterruptedResolver {
        catalog: catalog.clone(),
        gate: Mutex::new(None),
    });
    let Started::Call(active) = server.begin(json!("apps"), "apps", json!({})) else {
        panic!("not admitted")
    };
    let reply = server.run(&active);
    assert!(!reply.is_error, "{}", reply.data);
    assert!(reply.data.get("provider").is_none());
    assert_eq!(server.provider_id().as_deref(), Some("controlled"));
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 0);
}

#[test]
fn reordered_instances_preserve_scene_across_exec_but_replacement_retires_it() {
    let (server, catalog) = agent();
    catalog.backend.0.model.lock().unwrap().filtered_observation = true;
    let mut sibling = catalog.entries.lock().unwrap()[0].clone();
    sibling.app.pid += 1;
    catalog.entries.lock().unwrap().push(sibling);
    let looked = exec(&server, "initial", "await app.look();");
    assert!(!looked.is_error, "{}", looked.data);
    for round in 0..6 {
        catalog.entries.lock().unwrap().reverse();
        let clicked = exec(
            &server,
            &format!("click-{round}"),
            "await app.click({selector:{text:'Increment'}});",
        );
        assert!(!clicked.is_error, "{}", clicked.data);
        assert_eq!(clicked.data["context_reset"], false);
    }
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 6);
    for entry in catalog.entries.lock().unwrap().iter_mut() {
        entry.app.generation -= 1;
    }
    let replaced = exec(&server, "replaced", "await app.click({target:'w1.e1'});");
    assert!(replaced.is_error);
    assert_eq!(replaced.data["context_reset"], true);
    assert_eq!(replaced.data["script"]["failure"]["code"], "look_required");
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 6);
}

#[test]
fn unsupported_menu_never_prepares_focus_and_exposes_business_reason() {
    let (server, catalog) = agent();
    let looked = exec(&server, "initial", "await app.look();");
    assert!(!looked.is_error, "{}", looked.data);
    let failed = exec(
        &server,
        "menu",
        "await app.click({selector:{scope:'menu',text:'Absent'}});",
    );
    assert!(failed.is_error);
    assert_eq!(failed.data["status"], "completed");
    assert_eq!(failed.data["error"]["code"], "unsupported_capability");
    assert_eq!(
        failed.data["script"]["failure"]["code"],
        "unsupported_capability"
    );
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
    let projected = crate::app_protocol::projection::execution(failed);
    assert_eq!(projected.data["error"]["code"], "unsupported_capability");
    assert_eq!(projected.data["failure"]["code"], "unsupported_capability");
    assert!(projected.data["effects"]["dispatch"]["dispatched"].is_null());
    assert_eq!(projected.data["effects"]["dispatch"]["not_dispatched"], 1);
}

#[test]
fn script_cancellation_keeps_unknown_dispatch_separate_from_cancelled_reason() {
    let (server, catalog) = agent();
    let looked = exec(&server, "initial", "await app.look();");
    assert!(!looked.is_error, "{}", looked.data);
    catalog.backend.0.model.lock().unwrap().poll_action = true;
    let gate = catalog.backend.action_gate();
    let source = "await app.click({target:'w1.e1'});";
    let Started::Call(active) = server.begin(json!("script-cancel"), "exec", input(source)) else {
        panic!("not admitted")
    };
    let running = server.clone();
    let worker = std::thread::spawn(move || running.run(&active));
    gate.entered();
    server.cancel(&json!("script-cancel"));
    let reply = worker.join().unwrap();
    assert!(reply.is_error, "{}", reply.data);
    let steps = reply.data["script"]["steps"].as_array().unwrap();
    let receipt = steps
        .iter()
        .find_map(|step| step.get("receipt").filter(|r| r.is_object()))
        .unwrap();
    assert_eq!(receipt["outcome"]["dispatch"], "unknown");
    assert_eq!(receipt["interruption"]["code"], "cancelled");
    let projected = crate::app_protocol::projection::execution(reply.clone());
    assert_eq!(projected.data["error"]["code"], "cancelled");
    assert_eq!(projected.data["effects"]["dispatch"]["unknown"], 1);
    assert_eq!(exec(&server, "script-cancel", source).data, reply.data);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
}

#[test]
fn public_argument_errors_have_one_catchable_business_code() {
    let (server, catalog) = agent();
    let result = exec(
        &server,
        "invalid",
        r#"
        await app.look();
        const codes=[];
        for (const attempt of [
            () => app.look({}, {}),
            () => app.click({target:'w1.e1',x:1,y:1}),
            () => app.scroll({delta_y:101}),
            () => app.click({x:'bad',y:1}),
            () => app.click({selector:{scope:'invalid',text:'Increment'}})
        ]) { try { await attempt(); } catch(e) { codes.push(e.code); } }
        return codes;
    "#,
    );
    assert_eq!(
        success(&result),
        &json!([
            "invalid_arguments",
            "invalid_arguments",
            "invalid_arguments",
            "invalid_arguments",
            "invalid_arguments"
        ])
    );
    // Well-formed coordinates still need a retained image binding. Malformed
    // coordinates above fail shared argument validation before context checks.
    let unbound = exec(&server, "unbound", "await app.click({x:1,y:1});");
    assert!(unbound.is_error);
    assert_eq!(unbound.data["error"]["code"], "capture_required");
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
}
