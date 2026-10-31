use super::*;

fn excluded() -> WindowInventory {
    let mut inventory = test_inventory(vec![]);
    inventory.report.candidate_count = 1;
    inventory.report.excluded_count = 1;
    inventory.report.diagnostics.counters = vec![("explicitly_excluded".into(), 1)];
    inventory
}
fn script(source: &str) -> Command {
    Command::Script {
        source: source.into(),
        timeout_ms: 5000,
        max_calls: 4,
    }
}

#[test]
fn associated_surface_coverage_preserves_known_count_and_unknown_completeness() {
    let (owner, backend) = coordinator();
    let mut inventory = test_inventory(vec![WindowInfo {
        target: WindowTarget {
            app: application(),
            native_id: 42,
            generation: 1,
        },
        title: "Associated dialog".into(),
        visible: true,
    }]);
    inventory.report.scope = InventoryScope::ApplicationAndProvenAssociatedSurfaces {
        associated_count: 1,
    };
    let report = inventory.report.clone();
    backend.0.model.lock().unwrap().inventory = Some(inventory);
    let result = run(
        &owner,
        "associated-surfaces",
        None,
        script("return await app.windows();"),
    );
    assert!(!result.is_error, "{}", result.data);
    let coverage = &result.data["value"]["result"]["coverage"];
    assert_eq!(
        coverage["scope"],
        "application_and_proven_associated_surfaces"
    );
    assert_eq!(coverage["associated_count"], 1);
    assert_eq!(coverage["association_complete"], false);
    assert_eq!(coverage["complete_within_scope"], true);
    let excludes = coverage["excludes"].as_array().unwrap();
    assert!(excludes.contains(&json!("unproven_associations")));
    assert!(!excludes.contains(&json!("helper_processes")));

    let aggregate = aggregate_inventory_report(&[&report, &report], &[], 2);
    assert_eq!(aggregate["coverage"]["scope"], "application_instances");
    assert_eq!(aggregate["coverage"]["associated_count"], 2);
    assert_eq!(aggregate["coverage"]["association_complete"], false);
    let partial = aggregate_inventory_report(&[&report], &[&Error::TimedOut], 1);
    assert_eq!(partial["coverage"]["status"], "partial");
    assert_eq!(partial["coverage"]["complete_within_scope"], false);
    assert_eq!(partial["coverage"]["associated_count"], 1);
    assert_eq!(partial["coverage"]["association_complete"], false);

    let mut invalid = report;
    invalid.scope = InventoryScope::ApplicationAndProvenAssociatedSurfaces {
        associated_count: 2,
    };
    assert!(!invalid.valid());
}

#[test]
fn scope_empty_remains_distinguishable_in_atomic_and_script_results() {
    let (owner, backend) = coordinator();
    backend.0.model.lock().unwrap().inventory = Some(excluded());
    let atomic = run(&owner, "atomic-empty", None, Command::Windows);
    assert!(!atomic.is_error, "{}", atomic.data);
    let value = &atomic.data["value"];
    assert_eq!(value["windows"], json!([]));
    assert_eq!(value["coverage"]["status"], "empty_within_scope");
    assert_eq!(value["coverage"]["complete_within_scope"], true);
    assert_eq!(value["coverage"]["excluded_count"], 1);
    let scripted = run(
        &owner,
        "script-empty",
        None,
        script("return await app.windows();"),
    );
    assert_eq!(&scripted.data["value"]["result"], value);
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 0);
}

#[test]
fn unproven_evidence_survives_atomic_js_catch_and_uncaught_step_without_retry() {
    let (owner, backend) = coordinator();
    let report = test_inventory(vec![]).report;
    backend.0.model.lock().unwrap().windows_error = Some(Error::InventoryUnproven(report));
    let atomic = run(&owner, "atomic-unproven", None, Command::Windows);
    assert_code(&atomic, "window_inventory_unproven");
    let details = &atomic.data["error"]["details"];
    assert_eq!(details["coverage"]["status"], "unproven");
    assert_eq!(details["coverage"]["complete_within_scope"], false);
    assert!(atomic.data["value"].is_null());
    let caught = run(
        &owner,
        "caught",
        None,
        script("try { await app.windows(); } catch(e) { return {code:e.code,details:e.details}; }"),
    );
    assert!(!caught.is_error, "{}", caught.data);
    assert_eq!(
        caught.data["value"]["result"]["code"],
        "window_inventory_unproven"
    );
    assert_eq!(&caught.data["value"]["result"]["details"], details);
    let failed = run(
        &owner,
        "uncaught",
        None,
        script("state.changed=true; return await app.windows();"),
    );
    assert!(failed.is_error);
    assert_eq!(failed.data["script"]["state_committed"], false);
    assert_eq!(
        &failed.data["script"]["steps"][0]["error"]["details"],
        details
    );
    assert_eq!(
        failed.data["script"]["failure"]["code"],
        "window_inventory_unproven"
    );
    assert_eq!(backend.0.windows.load(Ordering::SeqCst), 3);
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 0);
}

#[test]
fn core_rejects_invalid_inventory_counts_and_unbounded_diagnostics() {
    for mutate in [
        |i: &mut WindowInventory| i.report.supported_count = 1,
        |i: &mut WindowInventory| i.report.candidate_count = 257,
        |i: &mut WindowInventory| i.report.diagnostics.error_codes = vec![1; 9],
        |i: &mut WindowInventory| {
            i.report
                .diagnostics
                .counters
                .push(("bad title data".into(), 0))
        },
        |i: &mut WindowInventory| {
            i.report
                .diagnostics
                .counters
                .push(("explicitly_excluded".into(), 1))
        },
    ] {
        let (owner, backend) = coordinator();
        let mut inventory = excluded();
        mutate(&mut inventory);
        backend.0.model.lock().unwrap().inventory = Some(inventory);
        assert_code(
            &run(&owner, "invalid", None, Command::Windows),
            "backend_failed",
        );
    }
    let (owner, backend) = coordinator();
    let mut report = excluded().report;
    report.diagnostics.provider = "x".repeat(65);
    backend.0.model.lock().unwrap().windows_error = Some(Error::InventoryUnproven(report));
    assert_code(
        &run(&owner, "invalid-error", None, Command::Windows),
        "backend_failed",
    );
}

#[test]
fn successful_empty_inventory_remains_in_ledger_after_later_script_exception() {
    let (owner, backend) = coordinator();
    backend.0.model.lock().unwrap().inventory = Some(excluded());
    let reply = run(
        &owner,
        "throw-after-empty",
        None,
        script("await app.windows(); throw new Error('ambiguous window');"),
    );
    assert!(reply.is_error);
    let evidence = &reply.data["script"]["steps"][0]["inventory"];
    assert_eq!(evidence["coverage"]["status"], "empty_within_scope");
    assert_eq!(evidence["coverage"]["excluded_count"], 1);
    assert_eq!(
        evidence["diagnostics"]["counters"]["explicitly_excluded"],
        1
    );
    assert!(evidence.get("windows").is_none());
    assert_eq!(reply.data["script"]["state_committed"], false);
    assert_eq!(backend.0.windows.load(Ordering::SeqCst), 1);
}

#[test]
fn application_surface_scope_reaches_scripts_without_claiming_ax_or_helper_coverage() {
    let (owner, backend) = coordinator();
    let mut inventory = test_inventory(vec![WindowInfo {
        target: WindowTarget {
            app: application(),
            native_id: 303,
            generation: 3,
        },
        title: "Raised palette".into(),
        visible: true,
    }]);
    inventory.report.scope = InventoryScope::SameProcessApplicationSurfaces;
    inventory.report.diagnostics.provider = "macos.window-server".into();
    backend.0.model.lock().unwrap().inventory = Some(inventory);
    let result = run(
        &owner,
        "raised-surfaces",
        None,
        script("return await app.windows();"),
    );
    assert!(!result.is_error, "{}", result.data);
    let value = &result.data["value"]["result"];
    assert_eq!(
        value["coverage"]["scope"],
        "same_process_application_surfaces"
    );
    assert_eq!(value["coverage"]["complete_within_scope"], true);
    assert_eq!(value["coverage"]["supported_count"], 1);
    let excludes = value["coverage"]["excludes"].as_array().unwrap();
    assert!(excludes.contains(&json!("helper_processes")));
    assert!(excludes.contains(&json!("unproven_associations")));
    assert!(!excludes.contains(&json!("non_default_layers")));
    assert_eq!(value["windows"][0]["title"], "Raised palette");
    assert!(value["windows"][0]["window_ref"].as_str().is_some());
    assert!(value["windows"][0].get("native_id").is_none());
    assert!(value["windows"][0].get("can_press").is_none());
    assert_eq!(backend.0.actions.load(Ordering::SeqCst), 0);
}
