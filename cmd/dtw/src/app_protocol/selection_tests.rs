//! Resolver controls exercise original Core bindings and receipts, not GUI effects.
use super::*;
use std::collections::VecDeque;

#[path = "wait_tests.rs"]
mod wait_tests;

struct Page {
    elements: Vec<Element>,
    discovery: DiscoveryPage,
}
fn page(elements: Vec<Element>, more: bool, complete: bool, visited: u32) -> Page {
    Page {
        elements,
        discovery: DiscoveryPage {
            has_more: more,
            complete,
            visited,
            reason: (!more && !complete).then(|| "node_unavailable".into()),
        },
    }
}
fn element(id: u64, pressable: bool) -> Element {
    Element {
        id: ElementId(id),
        label: "Save".into(),
        role: "button".into(),
        name: Some("Save".into()),
        value: Some("summary".into()),
        enabled: Some(true),
        can_press: pressable,
        actions: if pressable {
            vec!["press".into(), "setValue".into()]
        } else {
            vec![]
        },
    }
}
#[derive(Clone)]
struct SelectionBackend {
    base: MockBackend,
    focus: MockBackend,
    pages: Arc<Mutex<VecDeque<Result<Page, Error>>>>,
    queries: Arc<Mutex<Vec<ObserveQuery>>>,
    advances: Arc<AtomicUsize>,
    menu_supported: Arc<std::sync::atomic::AtomicBool>,
    scopes: Arc<Mutex<Vec<ObserveScopeKind>>>,
    clicks: Arc<Mutex<Vec<(PixelPoint, ClickOptions)>>>,
    values: Arc<Mutex<Vec<String>>>,
    selected_elements: Arc<Mutex<Vec<ElementId>>>,
}
impl SelectionBackend {
    fn next(&self) -> Result<Page, Error> {
        self.pages
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Ok(page(vec![], false, true, 0)))
    }
}
impl Backend for SelectionBackend {
    type Binding = ();
    fn finish_execution(&self) -> Result<Option<FocusReturn>, Error> {
        self.focus.finish_execution()
    }
    fn shutdown(&self) -> Result<(), Error> {
        self.base.shutdown()?;
        self.focus.shutdown()
    }
    fn stamp(&self) -> ProviderStamp {
        self.base.stamp()
    }
    fn supports(&self, _: Capability) -> bool {
        true
    }
    fn windows(&self, app: &AppInstance, call: &CallContext<'_>) -> Result<WindowInventory, Error> {
        self.base.windows(app, call)
    }
    fn observe(
        &self,
        target: &WindowTarget,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<()>, Error> {
        self.observe_page(target, &ObserveQuery::default(), call)
    }
    fn observe_page(
        &self,
        target: &WindowTarget,
        query: &ObserveQuery,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<()>, Error> {
        self.queries.lock().unwrap().push(query.clone());
        let mut read = self.base.read(target, false);
        call.remaining()?;
        let page = self.next()?;
        read.elements = page.elements;
        read.discovery = Some(page.discovery);
        Ok(read)
    }
    fn supports_observe_scope(&self, scope: ObserveScopeKind) -> bool {
        scope == ObserveScopeKind::Window
            || (self.menu_supported.load(Ordering::SeqCst) && scope == ObserveScopeKind::Menu)
    }
    fn observe_scoped(
        &self,
        target: &WindowTarget,
        query: &ObserveQuery,
        scope: ObserveScope,
        _: Option<&()>,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<()>, Error> {
        self.scopes.lock().unwrap().push(scope.kind());
        self.observe_page(target, query, call)
    }
    fn continue_observation(
        &self,
        target: &WindowTarget,
        _: &(),
        call: &CallContext<'_>,
    ) -> Result<BackendObservationPage, Error> {
        self.advances.fetch_add(1, Ordering::SeqCst);
        call.remaining()?;
        let page = self.next()?;
        Ok(BackendObservationPage {
            target: target.clone(),
            elements: page.elements,
            discovery: page.discovery,
        })
    }
    fn press(
        &self,
        _: &WindowTarget,
        _: &(),
        element: ElementId,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        self.selected_elements.lock().unwrap().push(element);
        self.base.act(call)
    }
    fn input(
        &self,
        _: &WindowTarget,
        input: &WindowInput,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        assert_eq!(input, &WindowInput::Focus);
        let outcome = self.focus.act(call);
        if outcome.dispatch == DispatchState::Dispatched {
            self.focus.0.model.lock().unwrap().finish_focus = Some(FocusReturn::Restored);
        }
        outcome
    }
    fn set_value(
        &self,
        _: &WindowTarget,
        _: &(),
        element: ElementId,
        value: &str,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        self.values.lock().unwrap().push(value.into());
        self.selected_elements.lock().unwrap().push(element);
        self.base.act(call)
    }
    fn capture(
        &self,
        target: &WindowTarget,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<()>, Error> {
        let mut read = self.base.capture(target, call)?;
        if target.native_id == 202 {
            let image = read.image.as_mut().unwrap();
            image.width = 40;
            image.height = 30;
        }
        Ok(read)
    }
    fn click_with_options(
        &self,
        _: &WindowTarget,
        _: &(),
        point: PixelPoint,
        options: ClickOptions,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        self.clicks.lock().unwrap().push((point, options));
        self.base.act(call)
    }
}
fn setup(
    pages: Vec<Result<Page, Error>>,
) -> (Arc<Coordinator<SelectionBackend>>, SelectionBackend) {
    let backend = SelectionBackend {
        base: MockBackend::new(),
        focus: MockBackend::new(),
        pages: Arc::new(Mutex::new(pages.into())),
        queries: Arc::new(Mutex::new(vec![])),
        advances: Arc::new(AtomicUsize::new(0)),
        menu_supported: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        scopes: Arc::new(Mutex::new(vec![])),
        clicks: Arc::new(Mutex::new(vec![])),
        values: Arc::new(Mutex::new(vec![])),
        selected_elements: Arc::new(Mutex::new(vec![])),
    };
    (
        Coordinator::new(backend.clone(), application(), "Selection".into()),
        backend,
    )
}
fn script(owner: &Arc<Coordinator<SelectionBackend>>, id: &str, body: &str) -> Reply {
    run(
        owner,
        id,
        None,
        Command::Script {
            source: format!("const window_ref=(await app.windows()).windows[0].window_ref; {body}"),
            timeout_ms: 5_000,
            max_calls: 16,
        },
    )
}
fn step(reply: &Reply) -> &Value {
    &reply.data["script"]["steps"][1]
}

fn content_unproven(elements: Vec<Element>) -> Page {
    let mut page = page(elements, false, false, 13);
    page.discovery.reason = Some("content_unproven".into());
    page
}

// Inject an already-spent polling window without sleeping for native cold AX.
// The real execution deadline and every Core/native admission remain active.
fn select_after_poll_window(
    owner: &Arc<Coordinator<SelectionBackend>>,
    command: Command,
    timeout_ms: u64,
) -> Work {
    let window = windows(owner, "inventory").remove(0);
    let submitted = submit(owner, "selection", Some(&window), command);
    owner.select_element_until(
        &submitted,
        &wire::ElementSelector {
            scope: wire::SelectorScope::Window,
            role: None,
            text: Some("Save".into()),
        },
        timeout_ms,
        Instant::now() + Duration::from_secs(5),
        Instant::now() - Duration::from_millis(1),
    )
}

fn selected_press(timeout_ms: u64) -> Command {
    Command::PressSelected {
        selector: wire::ElementSelector {
            scope: wire::SelectorScope::Window,
            role: None,
            text: Some("Save".into()),
        },
        timeout_ms,
    }
}

#[test]
fn selection_read_errors_describe_resolution_without_claiming_an_action() {
    for (matches, complete, expected) in [
        (vec![element(1, true)], false, "selector_unproven"),
        (
            vec![element(1, true), element(2, true)],
            true,
            "selector_ambiguous",
        ),
        (vec![], true, "selector_not_found"),
    ] {
        for method in ["find", "press"] {
            let (owner, backend) = setup(vec![Ok(page(matches.clone(), false, complete, 17))]);
            let reply = script(
                &owner,
                "resolution-error",
                &format!(
                "return await app.{method}({{window_ref,selector:{{text:'Save'}},timeout_ms:0}});"
            ),
            );
            let error = &step(&reply)["error"];
            assert_eq!(error["code"], expected);
            let message = error["message"].as_str().unwrap();
            if method == "find" {
                assert!(message.contains("No unique element reference"), "{message}");
                assert!(!message.contains("action"), "{message}");
            } else {
                assert!(
                    message.contains("selected action was not dispatched"),
                    "{message}"
                );
            }
            assert_eq!(error["details"]["matches"], matches.len());
            assert_eq!(error["details"]["coverage"]["visited"], 17);
            assert_eq!(backend.focus.0.actions.load(Ordering::SeqCst), 0);
            assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
        }
    }
}

#[test]
fn selection_content_recovery_retains_focus_receipt_and_dispatches_original_action_once() {
    for action in [
        "await app.press({window_ref,selector:{text:'Save'}});",
        "await app.setValue({window_ref,selector:{text:'Save'},value:'new'});",
    ] {
        let (owner, backend) = setup(vec![
            Ok(content_unproven(vec![element(1, true)])),
            Ok(page(vec![element(2, true)], true, false, 1)),
            Ok(page(vec![], false, true, 2)),
        ]);
        backend.focus.0.model.lock().unwrap().outcome.delivery = Some(InputDelivery {
            route: Some(InputRoute::Foreground),
            reason: None,
            foreground_borrowed: true,
            focus_return: Some(FocusReturn::Pending),
        });
        backend.base.0.model.lock().unwrap().outcome.delivery = Some(InputDelivery {
            route: Some(InputRoute::Semantic),
            reason: None,
            foreground_borrowed: false,
            focus_return: None,
        });
        let reply = script(&owner, "recover", action);
        assert!(!reply.is_error, "{}", reply.data);
        assert_eq!(backend.focus.0.actions.load(Ordering::SeqCst), 1);
        assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);
        assert_eq!(
            *backend.selected_elements.lock().unwrap(),
            vec![ElementId(2)]
        );
        assert_eq!(backend.queries.lock().unwrap().len(), 2);
        assert_eq!(backend.advances.load(Ordering::SeqCst), 1);
        assert_eq!(
            step(&reply)["resolution"]["focus_preparation"]["attempted"],
            true
        );
        let preparation = &step(&reply)["resolution"]["focus_preparation"]["receipt"];
        assert_eq!(preparation["outcome"]["dispatch"], "dispatched");
        assert_eq!(
            preparation["outcome"]["delivery"]["focus_return"],
            "pending"
        );
        assert_eq!(step(&reply)["receipt"]["outcome"]["dispatch"], "dispatched");
        assert_eq!(reply.data["script"]["focus_return"], "restored");
        let mut external = reply.clone();
        external.data["execution_id"] = json!("execution_prepared");
        let compact = projection::execution(external);
        let summary = &compact.data["script"]["summary"];
        assert_eq!(
            summary["last_resolution"]["resolution"]["focus_preparation"],
            json!({"attempted":true,"dispatch":"dispatched","delivery":preparation["outcome"]["delivery"]})
        );
        assert_eq!(summary["actions"]["delivery"]["reported"], 1);
        assert_eq!(
            summary["actions"]["delivery"]["routes"],
            json!({"semantic":1})
        );
        assert_eq!(summary["actions"]["delivery"]["foreground_borrowed"], 0);
    }
}

#[test]
fn selection_content_recovery_runs_once_even_after_default_polling_window_expired() {
    let (owner, backend) = setup(vec![
        Ok(content_unproven(vec![])),
        Ok(page(vec![element(2, true)], false, true, 1)),
    ]);
    let work = select_after_poll_window(&owner, selected_press(2_000), 2_000);
    assert!(work.completed.error.is_none());
    assert_eq!(backend.focus.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(backend.queries.lock().unwrap().len(), 2);
}

#[test]
fn selection_content_recovery_never_focuses_for_read_zero_timeout_other_gaps_or_ambiguity() {
    let mut disabled = element(1, true);
    disabled.enabled = Some(false);
    for (first, command, timeout, expected) in [
        (
            content_unproven(vec![]),
            Command::Find {
                selector: wire::ElementSelector {
                    scope: wire::SelectorScope::Window,
                    role: None,
                    text: Some("Save".into()),
                },
                timeout_ms: 2_000,
            },
            2_000,
            "selector_unproven",
        ),
        (
            content_unproven(vec![]),
            selected_press(0),
            0,
            "selector_unproven",
        ),
        (
            page(vec![], false, false, 1),
            selected_press(2_000),
            2_000,
            "selector_unproven",
        ),
        (
            page(vec![], false, true, 1),
            selected_press(2_000),
            2_000,
            "selector_not_found",
        ),
        (
            page(vec![disabled], false, true, 1),
            selected_press(2_000),
            2_000,
            "selector_not_ready",
        ),
        (
            content_unproven(vec![element(1, true), element(2, false)]),
            selected_press(2_000),
            2_000,
            "selector_ambiguous",
        ),
    ] {
        let (owner, backend) = setup(vec![Ok(first)]);
        let work = select_after_poll_window(&owner, command, timeout);
        assert_eq!(work.completed.error.unwrap().code, expected);
        assert_eq!(backend.focus.0.actions.load(Ordering::SeqCst), 0);
        assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
        assert_eq!(backend.queries.lock().unwrap().len(), 1);
    }
}

#[test]
fn selection_content_recovery_stops_after_focus_failure_or_unknown_and_preserves_evidence() {
    let mut strict_rejection = ActionOutcome::rejected(Error::NativeFailure {
        code: "foreground_required".into(),
        message: "Service input policy prohibits focus".into(),
    });
    strict_rejection.delivery = Some(InputDelivery {
        route: None,
        reason: Some("explicit_focus".into()),
        foreground_borrowed: false,
        focus_return: None,
    });
    for outcome in [
        strict_rejection,
        ActionOutcome::rejected(Error::TargetChanged),
        ActionOutcome::rejected(Error::TimedOut),
        ActionOutcome::rejected(Error::WrongApp),
        ActionOutcome::rejected(Error::NativeFailure {
            code: "permission_denied".into(),
            message: "Missing accessibility permission".into(),
        }),
        ActionOutcome::unknown(Error::NativeFailure {
            code: "original_focus_unknown".into(),
            message: "Unknown foreground effect".into(),
        }),
        ActionOutcome {
            delivery: None,
            dispatch: DispatchState::Unknown,
            evidence: EffectEvidence::Unverified,
            error: None,
        },
        ActionOutcome {
            delivery: None,
            dispatch: DispatchState::Dispatched,
            evidence: EffectEvidence::Mismatch {
                expected: "target focus".into(),
                observed: "another target".into(),
            },
            error: None,
        },
    ] {
        let (owner, backend) = setup(vec![
            Ok(content_unproven(vec![])),
            Ok(page(vec![element(2, true)], false, true, 1)),
        ]);
        backend.focus.0.model.lock().unwrap().outcome = outcome.clone();
        let reply = script(
            &owner,
            "focus-error",
            "await app.press({window_ref,selector:{text:'Save'}});",
        );
        assert!(reply.is_error, "{}", reply.data);
        let expected = outcome
            .error
            .as_ref()
            .map(wire::core_error)
            .map(|e| e.code)
            .unwrap_or_else(|| "focus_preparation_unproven".into());
        assert_eq!(step(&reply)["error"]["code"], expected);
        assert_eq!(
            step(&reply)["resolution"]["focus_preparation"]["receipt"]["outcome"],
            wire::receipt(&ActionReceipt {
                outcome,
                provider_outcome: None,
                interruption: None
            })["outcome"]
        );
        assert_eq!(
            step(&reply)["receipt"]["outcome"]["dispatch"],
            "not_dispatched"
        );
        assert_eq!(backend.focus.0.actions.load(Ordering::SeqCst), 1);
        assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
        assert_eq!(backend.queries.lock().unwrap().len(), 1);
    }
}

#[test]
fn selection_content_recovery_does_not_restart_preparation_or_poll_after_fresh_failure() {
    for (second, expected) in [
        (Ok(content_unproven(vec![])), "selector_unproven"),
        (Ok(page(vec![], false, true, 1)), "selector_not_found"),
        (
            Ok(page(
                vec![element(2, true), element(3, true)],
                false,
                true,
                2,
            )),
            "selector_ambiguous",
        ),
        (Err(Error::TargetChanged), "target_changed"),
    ] {
        let (owner, backend) = setup(vec![
            Ok(content_unproven(vec![])),
            second,
            Ok(page(vec![element(4, true)], false, true, 1)),
        ]);
        let reply = script(
            &owner,
            "one-recovery",
            "await app.press({window_ref,selector:{text:'Save'}});",
        );
        assert_eq!(step(&reply)["error"]["code"], expected, "{}", reply.data);
        assert_eq!(backend.focus.0.actions.load(Ordering::SeqCst), 1);
        assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
        assert_eq!(backend.queries.lock().unwrap().len(), 2);
        assert_eq!(reply.data["script"]["focus_return"], "restored");
    }
}

#[test]
fn selection_content_recovery_preserves_original_action_unknown_without_more_reads() {
    let (owner, backend) = setup(vec![
        Ok(content_unproven(vec![])),
        Ok(page(vec![element(2, true)], false, true, 1)),
    ]);
    backend.base.0.model.lock().unwrap().outcome = ActionOutcome::unknown(Error::TargetChanged);
    let reply = script(
        &owner,
        "input-unknown",
        "await app.press({window_ref,selector:{text:'Save'}});",
    );
    assert_eq!(step(&reply)["receipt"]["outcome"]["dispatch"], "unknown");
    assert_eq!(backend.focus.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(backend.queries.lock().unwrap().len(), 2);
}

#[test]
fn selection_content_recovery_cancellation_during_focus_keeps_original_receipt_and_stops() {
    let (owner, backend) = setup(vec![
        Ok(content_unproven(vec![])),
        Ok(page(vec![element(2, true)], false, true, 1)),
    ]);
    backend.focus.0.model.lock().unwrap().poll_action = true;
    let gate = backend.focus.action_gate();
    start(&owner, submit(&owner,"focus-cancel",None,Command::Script {
        source:"const window_ref=(await app.windows()).windows[0].window_ref;await app.press({window_ref,selector:{text:'Save'}});".into(),
        timeout_ms:5_000,max_calls:16 }));
    gate.entered();
    owner.handle(Request::Cancel {
        request_id: "focus-cancel".into(),
    });
    gate.release();
    let reply = finish(&owner, "focus-cancel");
    assert!(reply.is_error);
    assert_eq!(
        step(&reply)["resolution"]["focus_preparation"]["receipt"]["outcome"]["dispatch"],
        "unknown"
    );
    assert_eq!(backend.focus.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
    assert_eq!(backend.queries.lock().unwrap().len(), 1);
}

#[test]
fn selection_content_recovery_keeps_cleanup_failure_separate_from_selected_action() {
    let (owner, backend) = setup(vec![
        Ok(content_unproven(vec![])),
        Ok(page(vec![element(2, true)], false, true, 1)),
    ]);
    backend.focus.0.model.lock().unwrap().finish_error =
        Some(Error::BackendFailed("Focus return failed".into()));
    let reply = script(
        &owner,
        "cleanup-error",
        "await app.press({window_ref,selector:{text:'Save'}});",
    );
    assert!(reply.is_error, "{}", reply.data);
    assert_eq!(reply.data["script"]["focus_return"], "unconfirmed");
    assert_eq!(step(&reply)["receipt"]["outcome"]["dispatch"], "dispatched");
    assert_eq!(backend.focus.0.finishes.load(Ordering::SeqCst), 1);
    assert_eq!(backend.focus.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);
}

#[test]
fn selection_waits_for_complete_unique_discovery_then_dispatches_once() {
    let (owner, backend) = setup(vec![
        Ok(page(vec![element(1, true)], true, false, 1)),
        Ok(page(vec![], false, true, 2)),
    ]);
    let reply = script(
        &owner,
        "unique",
        "return await app.press({window_ref,selector:{role:'button',text:'Save'},timeout_ms:0});",
    );
    assert!(!reply.is_error, "{}", reply.data);
    assert_eq!(backend.queries.lock().unwrap().len(), 1);
    assert_eq!(backend.queries.lock().unwrap()[0].limit, 2);
    assert_eq!(backend.advances.load(Ordering::SeqCst), 1);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(reply.data["script"]["steps"].as_array().unwrap().len(), 2);
    assert_eq!(step(&reply)["resolution"]["pages"], 2);
    assert_eq!(step(&reply)["resolution"]["status"], "resolved");
    assert_eq!(step(&reply)["receipt"]["outcome"]["dispatch"], "dispatched");
    let mut external = reply.clone();
    external.data["execution_id"] = json!("execution_resolved");
    let compact = projection::execution(external);
    let summary = &compact.data["script"]["summary"]["last_resolution"]["resolution"];
    assert_eq!(summary["status"], "resolved");
    assert_eq!(summary["pages"], 2);
    assert!(summary.get("candidates").is_none());
    assert!(summary.get("coverage").is_none());
}

#[test]
fn selection_does_not_hide_nonpressable_duplicate_or_act_on_partial_result() {
    for (pages, code) in [
        (
            vec![Ok(page(
                vec![element(1, true), element(2, false)],
                false,
                true,
                2,
            ))],
            "selector_ambiguous",
        ),
        (
            vec![Ok(page(vec![element(1, true)], false, false, 1))],
            "selector_unproven",
        ),
        (vec![Ok(page(vec![], false, true, 0))], "selector_not_found"),
    ] {
        let (owner, backend) = setup(pages);
        let reply = script(
            &owner,
            "reject",
            "await app.press({window_ref,selector:{text:'Save'},timeout_ms:0});",
        );
        assert!(reply.is_error);
        assert_eq!(step(&reply)["error"]["code"], code);
        assert_eq!(
            step(&reply)["receipt"]["outcome"]["dispatch"],
            "not_dispatched"
        );
        assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
        assert_eq!(backend.queries.lock().unwrap().len(), 1);
        let mut external = reply.clone();
        external.data["execution_id"] = json!("execution_resolution");
        let compact = projection::execution(external);
        assert_eq!(
            compact.data["script"]["summary"]["last_issue"]["resolution"],
            step(&reply)["resolution"]
        );
    }
}

#[test]
fn selection_read_failure_keeps_partial_progress_without_replaying_cursor() {
    let (owner, backend) = setup(vec![
        Ok(page(vec![element(1, true)], true, false, 1)),
        Err(Error::NativeFailure {
            code: "ax_unavailable".into(),
            message: "original read error".into(),
        }),
    ]);
    let reply = script(
        &owner,
        "read-error",
        "await app.press({window_ref,selector:{text:'Save'}});",
    );
    assert_eq!(step(&reply)["error"]["code"], "ax_unavailable");
    assert_eq!(step(&reply)["resolution"]["status"], "read_failed");
    assert_eq!(step(&reply)["resolution"]["matches"], 1);
    assert_eq!(backend.advances.load(Ordering::SeqCst), 1);
    assert_eq!(backend.queries.lock().unwrap().len(), 1);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
}

#[test]
fn selection_wait_restarts_only_reads_and_expiry_keeps_context_healthy() {
    let (owner, backend) = setup(vec![
        Ok(page(vec![], false, true, 0)),
        Ok(page(vec![element(1, true)], false, true, 1)),
    ]);
    let reply = script(&owner, "appears", "return await app.setValue({window_ref,selector:{text:'Save'},value:'new value',timeout_ms:2000});");
    assert!(!reply.is_error, "{}", reply.data);
    assert_eq!(backend.queries.lock().unwrap().len(), 2);
    assert_eq!(*backend.values.lock().unwrap(), ["new value"]);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);
    let expired = script(
        &owner,
        "absent",
        "await app.find({window_ref,selector:{text:'Missing'},timeout_ms:60});",
    );
    assert_eq!(step(&expired)["error"]["code"], "selector_not_found");
    assert_eq!(backend.base.0.shutdowns.load(Ordering::SeqCst), 0);
    let healthy = script(&owner, "healthy", "return 7;");
    assert!(!healthy.is_error, "{}", healthy.data);
}

// Success cases above and below test transitions, not scheduler latency. Their
// two-second polling budget leaves room for parallel CI; expiry is injected
// directly here so its no-dispatch contract does not depend on a timed sleep.
#[test]
fn disabled_selection_with_spent_poll_window_does_not_read_the_next_page_or_dispatch() {
    let mut disabled = element(1, false);
    disabled.enabled = Some(false);
    let (owner, backend) = setup(vec![
        Ok(page(vec![disabled], false, true, 1)),
        Ok(page(vec![element(2, true)], false, true, 1)),
    ]);
    let work = select_after_poll_window(&owner, selected_press(200), 200);
    assert_eq!(work.completed.error.unwrap().code, "selector_not_ready");
    assert_eq!(backend.queries.lock().unwrap().len(), 1);
    assert_eq!(backend.pages.lock().unwrap().len(), 1);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
    assert_eq!(backend.focus.0.actions.load(Ordering::SeqCst), 0);
}

#[test]
fn selection_waits_for_disabled_target_without_filtering_it_or_delaying_find() {
    let mut disabled = element(1, false);
    disabled.enabled = Some(false);
    let (owner, backend) = setup(vec![
        Ok(page(vec![disabled.clone()], false, true, 1)),
        Ok(page(vec![element(2, true)], false, true, 1)),
    ]);
    let reply = script(
        &owner,
        "enabled",
        "await app.press({window_ref,selector:{text:'Save'},timeout_ms:2000});",
    );
    assert!(!reply.is_error, "{}", reply.data);
    assert_eq!(backend.queries.lock().unwrap().len(), 2);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);

    let (owner, backend) = setup(vec![Ok(page(vec![disabled.clone()], false, true, 1))]);
    let reply = script(
        &owner,
        "disabled",
        "await app.press({window_ref,selector:{text:'Save'},timeout_ms:0});",
    );
    assert_eq!(step(&reply)["error"]["code"], "selector_not_ready");
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
    backend
        .pages
        .lock()
        .unwrap()
        .push_back(Ok(page(vec![disabled], false, true, 1)));
    let found = script(
        &owner,
        "disabled-found",
        "return await app.find({window_ref,selector:{text:'Save'}});",
    );
    assert!(!found.is_error, "{}", found.data);
    assert_eq!(found.data["value"]["result"]["element"]["enabled"], false);
    assert_eq!(backend.queries.lock().unwrap().len(), 2);
}

#[test]
fn selection_find_returns_same_live_binding_and_bounded_element_summary() {
    let (owner, backend) = setup(vec![Ok(page(vec![element(1, true)], false, true, 1))]);
    let reply = script(&owner, "find", "const e=await app.find({window_ref,selector:{text:'Save'},timeout_ms:0}); await app.press({window_ref:e.window_ref,observation_ref:e.observation_ref,element_ref:e.element_ref}); return e.element;");
    assert!(!reply.is_error, "{}", reply.data);
    assert_eq!(reply.data["value"]["result"]["value"], "summary");
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(backend.queries.lock().unwrap().len(), 1);
}

#[test]
fn selection_unknown_action_is_not_rediscovered_or_replayed() {
    let (owner, backend) = setup(vec![Ok(page(vec![element(1, true)], false, true, 1))]);
    let original = ActionOutcome::unknown(Error::NativeFailure {
        code: "original_unknown".into(),
        message: "unknown effect".into(),
    });
    backend.base.0.model.lock().unwrap().outcome = original.clone();
    let reply = script(
        &owner,
        "unknown",
        "await app.press({window_ref,selector:{text:'Save'}});",
    );
    assert_eq!(
        step(&reply)["receipt"]["outcome"],
        wire::receipt(&ActionReceipt {
            outcome: original,
            provider_outcome: None,
            interruption: None
        })["outcome"]
    );
    assert_eq!(step(&reply)["resolution"]["status"], "resolved");
    assert_eq!(backend.queries.lock().unwrap().len(), 1);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);
}

#[test]
fn selection_cancel_during_read_retains_progress_and_never_dispatches_input() {
    let (owner, backend) = setup(vec![Ok(page(vec![element(1, true)], false, true, 1))]);
    let gate = backend.base.read_gate();
    let started = start(&owner, submit(&owner, "cancel", None, Command::Script {
        source: "const window_ref=(await app.windows()).windows[0].window_ref;await app.press({window_ref,selector:{text:'Save'}});".into(),
        timeout_ms: 5_000, max_calls: 16,
    }));
    assert!(!started.is_error);
    gate.entered();
    owner.handle(Request::Cancel {
        request_id: "cancel".into(),
    });
    gate.release();
    let reply = finish(&owner, "cancel");
    assert!(reply.is_error);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
    assert_eq!(step(&reply)["resolution"]["status"], "read_failed");
    assert_eq!(
        step(&reply)["receipt"]["outcome"]["dispatch"],
        "not_dispatched"
    );
}

#[test]
fn selection_never_waits_or_substitutes_an_input_for_unsupported_action() {
    let (owner, backend) = setup(vec![Ok(page(vec![element(1, false)], false, true, 1))]);
    let reply = script(
        &owner,
        "not-pressable",
        "await app.press({window_ref,selector:{text:'Save'}});",
    );
    assert_eq!(
        step(&reply)["receipt"]["outcome"]["dispatch"],
        "not_dispatched"
    );
    assert_eq!(step(&reply)["error"]["code"], "element_not_pressable");
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
    assert_eq!(backend.queries.lock().unwrap().len(), 1);
}

#[test]
fn normalized_click_uses_bound_capture_dimensions_and_preserves_options() {
    let (owner, backend) = setup(vec![]);
    let reply = script(&owner, "normalized", "const other=(await app.windows()).windows[1].window_ref;const o=await app.capture({window_ref});await app.capture({window_ref:other});await app.click({window_ref,observation_ref:o.observation_ref,x:500,y:250,coordinate_space:'normalized_1000',button:'right',count:2});");
    assert!(!reply.is_error, "{}", reply.data);
    assert_eq!(
        *backend.clicks.lock().unwrap(),
        [(
            PixelPoint { x: 10.0, y: 2.5 },
            ClickOptions {
                button: MouseButton::Right,
                count: 2
            }
        )]
    );
}

#[test]
fn normalized_click_rejects_semantic_ref_and_does_not_consume_it() {
    let (owner, backend) = setup(vec![Ok(page(vec![element(1, true)], false, true, 1))]);
    let reply = script(&owner, "wrong-kind", "const e=await app.find({window_ref,selector:{text:'Save'},timeout_ms:0}); try {await app.click({window_ref,observation_ref:e.observation_ref,x:500,y:500,coordinate_space:'normalized_1000'});} catch(_) {} await app.press({window_ref,observation_ref:e.observation_ref,element_ref:e.element_ref});");
    assert!(!reply.is_error, "{}", reply.data);
    assert!(backend.clicks.lock().unwrap().is_empty());
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);
}

#[test]
fn selection_and_normalized_wire_shapes_reject_mixed_or_unbounded_arguments() {
    for args in [
        json!({"window_ref":"w","selector":{}}),
        json!({"window_ref":"w","selector":{"text":" "}}),
        json!({"window_ref":"w","selector":{"text":"Save","first":true}}),
        json!({"window_ref":"w","selector":{"text":"Save"},"element_ref":"e","observation_ref":"o"}),
        json!({"window_ref":"w","selector":{"text":"Save"},"timeout_ms":30001}),
        json!({"window_ref":"w","selector":{"text":"Save"},"timeout_ms":-1}),
    ] {
        assert!(
            wire::script_submit("a", "r", "press", &args).is_err(),
            "{args}"
        );
    }
    for point in [-1.0, 1000.0, 1001.0] {
        let args = json!({"window_ref":"w","observation_ref":"o","x":point,"y":0,"coordinate_space":"normalized_1000"});
        assert!(wire::script_submit("a", "r", "click", &args).is_err());
    }
    for space in [json!("desktop"), Value::Null, json!(true)] {
        let args =
            json!({"window_ref":"w","observation_ref":"o","x":0,"y":0,"coordinate_space":space});
        assert!(wire::script_submit("a", "r", "click", &args).is_err());
    }
    let args = json!({"window_ref":"w","observation_ref":"o","x":0,"y":0,"coordinate_space":"image_pixels"});
    assert!(matches!(
        wire::script_submit("a", "r", "click", &args)
            .unwrap()
            .command,
        Command::Click { .. }
    ));
}

#[test]
fn normalized_click_keeps_invalid_point_capture_but_rejects_replaced_capture() {
    let (owner, backend) = setup(vec![]);
    let reply = script(&owner, "bounds", "const o=await app.capture({window_ref});try {await app.click({window_ref,observation_ref:o.observation_ref,x:1000,y:0,coordinate_space:'normalized_1000'});} catch(_) {} await app.click({window_ref,observation_ref:o.observation_ref,x:0,y:0,coordinate_space:'normalized_1000'});");
    assert!(!reply.is_error, "{}", reply.data);
    assert_eq!(backend.clicks.lock().unwrap().len(), 1);
    let stale = script(&owner, "stale", "const o=await app.capture({window_ref});await app.capture({window_ref});await app.click({window_ref,observation_ref:o.observation_ref,x:500,y:500,coordinate_space:'normalized_1000'});");
    assert!(stale.is_error);
    let steps = stale.data["script"]["steps"].as_array().unwrap();
    assert_eq!(
        steps.last().unwrap()["receipt"]["outcome"]["dispatch"],
        "not_dispatched"
    );
    assert_eq!(backend.clicks.lock().unwrap().len(), 1);
}

#[test]
fn window_wait_wire_requires_existing_anchor_and_one_bounded_condition() {
    for state in ["dialog", "unblocked", "closed"] {
        let parsed = wire::script_submit(
            "a",
            "r",
            "waitForWindow",
            &json!({"window_ref":"w","state":state}),
        )
        .unwrap();
        assert!(matches!(
            parsed.command,
            Command::WaitForWindow {
                timeout_ms: 2_000,
                ..
            }
        ));
        assert_eq!(parsed.target.window_ref.as_deref(), Some("w"));
    }
    for args in [
        json!({"state":"dialog"}),
        json!({"window_ref":"w","state":"visible"}),
        json!({"window_ref":"w","state":"dialog","timeout_ms":30001}),
        json!({"window_ref":"w","state":"dialog","title":"Save"}),
    ] {
        assert!(
            wire::script_submit("a", "r", "waitForWindow", &args).is_err(),
            "{args}"
        );
    }
}

#[test]
fn menu_selection_find_paginates_only_menu_scope_without_focus() {
    let (owner, backend) = setup(vec![
        Ok(page(vec![element(1, true)], true, false, 1)),
        Ok(page(vec![], false, true, 2)),
    ]);
    let reply = script(
        &owner,
        "menu-find",
        "return await app.find({window_ref,selector:{scope:'menu',text:'Save'},timeout_ms:0});",
    );
    assert!(!reply.is_error, "{}", reply.data);
    assert_eq!(
        *backend.scopes.lock().unwrap(),
        vec![ObserveScopeKind::Menu]
    );
    assert_eq!(backend.advances.load(Ordering::SeqCst), 1);
    assert_eq!(backend.focus.0.actions.load(Ordering::SeqCst), 0);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
}

#[test]
fn menu_selection_prepares_focus_then_resolves_fresh_and_dispatches_once() {
    for action in [
        "return await app.press({window_ref,selector:{scope:'menu',text:'Save'},timeout_ms:0});",
        "return await app.setValue({window_ref,selector:{scope:'menu',text:'Save'},value:'new',timeout_ms:0});",
    ] {
        let (owner, backend) = setup(vec![
            Ok(page(vec![element(1, true)], true, false, 1)),
            Ok(page(vec![], false, true, 2)),
            Ok(page(vec![element(2, true)], true, false, 1)),
            Ok(page(vec![], false, true, 2)),
        ]);
        let reply = script(&owner, "menu-action", action);
        assert!(!reply.is_error, "{}", reply.data);
        assert_eq!(*backend.scopes.lock().unwrap(), vec![ObserveScopeKind::Menu; 2]);
        assert_eq!(backend.advances.load(Ordering::SeqCst), 2);
        assert_eq!(backend.focus.0.actions.load(Ordering::SeqCst), 1);
        assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);
        assert_eq!(*backend.selected_elements.lock().unwrap(), vec![ElementId(2)]);
        assert_eq!(step(&reply)["resolution"]["focus_preparation"]["trigger"], "menu_scope");
        assert_eq!(step(&reply)["resolution"]["focus_preparation"]["receipt"]["outcome"]["dispatch"], "dispatched");
        assert_eq!(reply.data["script"]["focus_return"], "restored");
    }
}

#[test]
fn menu_selection_never_focuses_an_unproven_or_missing_target() {
    for (initial, expected) in [
        (
            page(vec![element(1, true), element(2, true)], false, true, 2),
            "selector_ambiguous",
        ),
        (
            page(vec![element(1, true)], false, false, 1),
            "selector_unproven",
        ),
        (content_unproven(vec![]), "selector_unproven"),
        (page(vec![], false, true, 0), "selector_not_found"),
    ] {
        let (owner, backend) = setup(vec![Ok(initial)]);
        let reply = script(
            &owner,
            "menu-rejected",
            "await app.press({window_ref,selector:{scope:'menu',text:'Save'},timeout_ms:0});",
        );
        assert_eq!(step(&reply)["error"]["code"], expected);
        assert_eq!(backend.focus.0.actions.load(Ordering::SeqCst), 0);
        assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn menu_selection_revalidates_disabled_state_and_ambiguity_after_focus() {
    let mut disabled = element(1, false);
    disabled.enabled = Some(false);
    for (fresh, expected) in [
        (page(vec![element(2, true)], false, true, 1), None),
        (
            page(vec![element(2, true), element(3, true)], false, true, 2),
            Some("selector_ambiguous"),
        ),
        (
            page(vec![disabled.clone()], false, true, 1),
            Some("selector_not_ready"),
        ),
    ] {
        let (owner, backend) = setup(vec![
            Ok(page(vec![disabled.clone()], false, true, 1)),
            Ok(fresh),
        ]);
        let reply = script(
            &owner,
            "menu-enabled",
            "await app.press({window_ref,selector:{scope:'menu',text:'Save'},timeout_ms:0});",
        );
        assert_eq!(
            step(&reply)["error"]["code"].as_str(),
            expected,
            "{}",
            reply.data
        );
        assert_eq!(backend.focus.0.actions.load(Ordering::SeqCst), 1);
        assert_eq!(
            backend.base.0.actions.load(Ordering::SeqCst),
            usize::from(expected.is_none())
        );
        assert_eq!(reply.data["script"]["focus_return"], "restored");
    }
}

#[test]
fn menu_selection_unsupported_or_focus_failure_never_replays_or_dispatches() {
    let (owner, backend) = setup(vec![]);
    backend.menu_supported.store(false, Ordering::SeqCst);
    let reply = script(
        &owner,
        "unsupported-menu",
        "await app.press({window_ref,selector:{scope:'menu',text:'Save'}});",
    );
    assert_eq!(step(&reply)["error"]["code"], "unsupported_capability");
    assert!(backend.queries.lock().unwrap().is_empty());
    assert_eq!(backend.focus.0.actions.load(Ordering::SeqCst), 0);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);

    for outcome in [
        ActionOutcome::rejected(Error::TargetChanged),
        ActionOutcome::unknown(Error::TargetChanged),
    ] {
        let (owner, backend) = setup(vec![Ok(page(vec![element(1, true)], false, true, 1))]);
        backend.focus.0.model.lock().unwrap().outcome = outcome.clone();
        let reply = script(
            &owner,
            "menu-focus-failed",
            "await app.press({window_ref,selector:{scope:'menu',text:'Save'}});",
        );
        assert_eq!(
            step(&reply)["resolution"]["focus_preparation"]["receipt"]["outcome"]["dispatch"],
            if outcome.dispatch == DispatchState::Unknown {
                "unknown"
            } else {
                "not_dispatched"
            }
        );
        assert_eq!(backend.queries.lock().unwrap().len(), 1);
        assert_eq!(backend.focus.0.actions.load(Ordering::SeqCst), 1);
        assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn selection_menu_scope_wire_and_missing_menu_hint_are_explicit() {
    for scope in [json!("window"), json!("menu")] {
        let parsed = wire::script_submit(
            "a",
            "r",
            "find",
            &json!({"window_ref":"w","selector":{"scope":scope,"text":"Save"}}),
        )
        .unwrap();
        assert!(matches!(parsed.command, Command::Find { .. }));
    }
    for scope in [Value::Null, json!(true), json!("subtree"), json!("Menu")] {
        assert!(wire::script_submit(
            "a",
            "r",
            "find",
            &json!({"window_ref":"w","selector":{"scope":scope,"text":"Save"}})
        )
        .is_err());
    }
    assert!(wire::script_submit(
        "a",
        "r",
        "find",
        &json!({"window_ref":"w","selector":{"scope":"menu"}})
    )
    .is_err());
    let (owner, backend) = setup(vec![Ok(page(vec![], false, true, 0))]);
    let reply = script(
        &owner,
        "missing-menu",
        "await app.press({window_ref,selector:{role:'menu_item',text:'Save'},timeout_ms:0});",
    );
    assert_eq!(step(&reply)["error"]["code"], "selector_not_found");
    assert!(step(&reply)["error"]["message"]
        .as_str()
        .unwrap()
        .contains("scope:'menu'"));
    assert_eq!(backend.focus.0.actions.load(Ordering::SeqCst), 0);
    assert_eq!(
        *backend.scopes.lock().unwrap(),
        vec![ObserveScopeKind::Window]
    );
}

#[test]
fn menu_selection_unsupported_action_does_not_borrow_focus_and_unknown_is_not_replayed() {
    let (owner, backend) = setup(vec![Ok(page(vec![element(1, false)], false, true, 1))]);
    let rejected = script(
        &owner,
        "menu-no-action",
        "await app.press({window_ref,selector:{scope:'menu',text:'Save'}});",
    );
    assert!(rejected.is_error, "{}", rejected.data);
    assert_eq!(
        step(&rejected)["receipt"]["outcome"]["dispatch"],
        "not_dispatched"
    );
    assert_eq!(backend.focus.0.actions.load(Ordering::SeqCst), 0);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
    assert_eq!(backend.queries.lock().unwrap().len(), 1);

    let (owner, backend) = setup(vec![
        Ok(page(vec![element(1, true)], false, true, 1)),
        Ok(page(vec![element(2, true)], false, true, 1)),
    ]);
    backend.base.0.model.lock().unwrap().outcome = ActionOutcome::unknown(Error::TargetChanged);
    let reply = script(
        &owner,
        "menu-unknown",
        "await app.press({window_ref,selector:{scope:'menu',text:'Save'}});",
    );
    assert_eq!(step(&reply)["receipt"]["outcome"]["dispatch"], "unknown");
    assert_eq!(backend.focus.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(backend.queries.lock().unwrap().len(), 2);
}

#[test]
fn menu_selection_waits_for_readiness_after_focus_without_repeating_focus() {
    let mut disabled = element(2, false);
    disabled.enabled = Some(false);
    let (owner, backend) = setup(vec![
        Ok(page(vec![element(1, true)], false, true, 1)),
        Ok(page(vec![disabled], false, true, 1)),
        Ok(page(vec![element(3, true)], false, true, 1)),
    ]);
    let reply = script(
        &owner,
        "menu-readiness",
        "await app.press({window_ref,selector:{scope:'menu',text:'Save'},timeout_ms:2000});",
    );
    assert!(!reply.is_error, "{}", reply.data);
    assert_eq!(backend.focus.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(backend.queries.lock().unwrap().len(), 3);
    assert_eq!(
        *backend.selected_elements.lock().unwrap(),
        vec![ElementId(3)]
    );
    assert_eq!(reply.data["script"]["focus_return"], "restored");
}

#[test]
fn selection_ownership_gap_stops_the_cursor_without_dispatching() {
    for matches in 0..=2 {
        let mut partial = page(
            (1..=matches).map(|id| element(id, true)).collect(),
            true,
            false,
            3,
        );
        partial.discovery.reason = Some("ownership_unproven".into());
        let (owner, backend) = setup(vec![Ok(partial), Err(Error::UnsupportedCapability)]);
        let reply = script(
            &owner,
            "ownership-partial",
            "await app.setValue({window_ref,selector:{text:'Save'},value:'updated',timeout_ms:0});",
        );
        assert!(reply.is_error);
        assert_eq!(
            step(&reply)["error"]["code"],
            if matches == 2 {
                "selector_ambiguous"
            } else {
                "selector_unproven"
            }
        );
        assert_eq!(step(&reply)["resolution"]["pages"], 1);
        assert_eq!(
            step(&reply)["resolution"]["coverage"]["reason"],
            "ownership_unproven"
        );
        assert_eq!(
            step(&reply)["receipt"]["outcome"]["dispatch"],
            "not_dispatched"
        );
        assert_eq!(backend.advances.load(Ordering::SeqCst), 0);
        assert_eq!(backend.queries.lock().unwrap().len(), 1);
        assert_eq!(backend.focus.0.actions.load(Ordering::SeqCst), 0);
        assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn selection_ownership_gap_can_recover_with_a_fresh_query_and_one_action() {
    let mut partial = page(vec![element(1, true)], true, false, 1);
    partial.discovery.reason = Some("ownership_unproven".into());
    let (owner, backend) = setup(vec![
        Ok(partial),
        Ok(page(vec![element(2, true)], false, true, 1)),
    ]);
    let reply = script(
        &owner,
        "ownership-recovered",
        "await app.setValue({window_ref,selector:{text:'Save'},value:'updated',timeout_ms:2000});",
    );
    assert!(!reply.is_error, "{}", reply.data);
    assert_eq!(backend.advances.load(Ordering::SeqCst), 0);
    assert_eq!(backend.queries.lock().unwrap().len(), 2);
    assert_eq!(backend.focus.0.actions.load(Ordering::SeqCst), 0);
    assert_eq!(backend.base.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(
        *backend.selected_elements.lock().unwrap(),
        vec![ElementId(2)]
    );
    assert_eq!(*backend.values.lock().unwrap(), vec!["updated"]);
    assert_eq!(step(&reply)["resolution"]["attempts"], 2);
    assert_eq!(step(&reply)["resolution"]["pages"], 2);
}

#[test]
fn selection_windows_page_limit_keeps_paging_to_prove_uniqueness() {
    for duplicate in [false, true] {
        let mut first = page(vec![element(1, true)], true, false, 1);
        first.discovery.reason = Some("page_limit".into());
        let last = if duplicate {
            vec![element(2, true)]
        } else {
            vec![]
        };
        let (owner, backend) = setup(vec![Ok(first), Ok(page(last, false, true, 2))]);
        let reply = script(
            &owner,
            "windows-page-limit",
            "await app.press({window_ref,selector:{text:'Save'},timeout_ms:0});",
        );
        assert_eq!(backend.advances.load(Ordering::SeqCst), 1);
        assert_eq!(backend.queries.lock().unwrap().len(), 1);
        assert_eq!(
            backend.base.0.actions.load(Ordering::SeqCst),
            usize::from(!duplicate)
        );
        assert_eq!(reply.is_error, duplicate, "{}", reply.data);
        if duplicate {
            assert_eq!(step(&reply)["error"]["code"], "selector_ambiguous");
        }
    }
}
