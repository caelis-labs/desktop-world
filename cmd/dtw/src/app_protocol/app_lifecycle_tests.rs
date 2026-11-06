use super::*;

#[derive(Default)]
struct Life {
    instances: Vec<AppInstance>,
    generation: u64,
    launches: usize,
    quits: usize,
    closed: Vec<u64>,
    quit_pending: bool,
    quit_dialog: bool,
    omit_launch_identity: bool,
    unverified_launch: bool,
    exit_on_press: bool,
    exit_probe_live: bool,
    exit_probe_error: bool,
    exit_probe_gaps: usize,
    exit_reads: usize,
    presses: usize,
}
#[derive(Clone)]
struct Provider {
    life: Arc<Mutex<Life>>,
    mock: MockBackend,
}
fn confirmed(dispatch: DispatchState, condition: &str) -> ActionOutcome {
    ActionOutcome {
        dispatch,
        evidence: EffectEvidence::Confirmed(condition.into()),
        error: None,
        delivery: None,
    }
}
impl Backend for Provider {
    type Binding = ();
    fn stamp(&self) -> ProviderStamp {
        self.mock.stamp()
    }
    fn supports(&self, _: Capability) -> bool {
        true
    }
    fn launch_application(&self, _: &str, _: &CallContext<'_>) -> ApplicationLaunch {
        let mut life = self.life.lock().unwrap();
        let dispatch = if life.instances.is_empty() {
            life.generation += 1;
            let mut app = application();
            app.generation = life.generation;
            life.instances.push(app);
            life.closed.clear();
            life.launches += 1;
            DispatchState::Dispatched
        } else {
            DispatchState::NotDispatched
        };
        ApplicationLaunch {
            outcome: if life.unverified_launch {
                ActionOutcome {
                    dispatch,
                    evidence: EffectEvidence::Unverified,
                    error: None,
                    delivery: None,
                }
            } else {
                confirmed(dispatch, "application_running")
            },
            instances: if life.omit_launch_identity {
                vec![]
            } else {
                life.instances.clone()
            },
        }
    }
    fn quit_application(&self, app: &AppInstance, _: &CallContext<'_>) -> ActionOutcome {
        let mut life = self.life.lock().unwrap();
        if !life.instances.contains(app) {
            return ActionOutcome::rejected(Error::TargetChanged);
        }
        life.quits += 1;
        if life.quit_pending {
            return ActionOutcome {
                dispatch: DispatchState::Dispatched,
                evidence: EffectEvidence::Unverified,
                error: None,
                delivery: None,
            };
        }
        life.instances.retain(|member| member != app);
        confirmed(DispatchState::Dispatched, "application_exited")
    }
    fn close_window(&self, target: &WindowTarget, _: &CallContext<'_>) -> ActionOutcome {
        let mut life = self.life.lock().unwrap();
        if !life.instances.contains(&target.app) {
            return ActionOutcome::rejected(Error::TargetChanged);
        }
        life.closed.push(target.native_id);
        confirmed(DispatchState::Dispatched, "window_closed")
    }
    fn windows(&self, app: &AppInstance, call: &CallContext<'_>) -> Result<WindowInventory, Error> {
        let inventory = self.mock.windows(app, call)?;
        let life = self.life.lock().unwrap();
        if !life.instances.contains(app) {
            if life.exit_on_press {
                return Err(Error::NativeFailure {
                    code: "app_changed".into(),
                    message: "Exact process identity disappeared".into(),
                });
            }
            return Err(Error::TargetChanged);
        }
        let mut windows = inventory.windows;
        if life.quit_dialog && life.quits > 0 {
            let mut dialog = windows[0].clone();
            dialog.target.native_id = 303;
            dialog.title = "Quit confirmation".into();
            dialog.scene = WindowLayout::new([0., 0., 500., 300.], 0);
            dialog.scene.as_mut().unwrap().keyboard_modal = Some(true);
            windows.push(dialog);
        }
        Ok(test_inventory(
            windows
                .into_iter()
                .filter(|w| !life.closed.contains(&w.target.native_id))
                .collect(),
        ))
    }
    fn observe(
        &self,
        target: &WindowTarget,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<()>, Error> {
        self.mock.observe(target, call)
    }
    fn capture(
        &self,
        target: &WindowTarget,
        call: &CallContext<'_>,
    ) -> Result<BackendObservation<()>, Error> {
        self.mock.capture(target, call)
    }
    fn press(
        &self,
        target: &WindowTarget,
        binding: &(),
        element: ElementId,
        call: &CallContext<'_>,
    ) -> ActionOutcome {
        let outcome = self.mock.press(target, binding, element, call);
        let mut life = self.life.lock().unwrap();
        life.presses += 1;
        if life.exit_on_press && outcome.dispatch == DispatchState::Dispatched {
            life.instances.clear();
        }
        outcome
    }
    fn application_exited(&self, app: &AppInstance, _: &CallContext<'_>) -> Result<bool, Error> {
        let mut life = self.life.lock().unwrap();
        life.exit_reads += 1;
        if life.exit_reads <= life.exit_probe_gaps {
            return Err(Error::NativeFailure {
                code: "app_changed".into(),
                message: "Terminating process metadata is transitional".into(),
            });
        }
        if life.exit_probe_error {
            return Err(Error::BackendFailed("Exit metadata unavailable".into()));
        }
        Ok(!life.exit_probe_live && !life.instances.contains(app))
    }
}
#[derive(Clone)]
struct Installed {
    provider: Provider,
}
impl Factory for Installed {
    type Provider = Provider;
    fn catalog(&self) -> Result<Vec<Application>, Error> {
        Ok(self
            .provider
            .life
            .lock()
            .unwrap()
            .instances
            .iter()
            .map(|app| Application {
                app_id: APP.into(),
                name: "Installed".into(),
                app: app.clone(),
            })
            .collect())
    }
    fn resolve(&self, id: &str) -> Result<ApplicationTarget, Error> {
        if id != APP {
            return Err(Error::WrongApp);
        }
        Ok(ApplicationTarget {
            app_id: id.into(),
            name: "Installed".into(),
            instances: self.provider.life.lock().unwrap().instances.clone(),
        })
    }
    fn connect(&self) -> Result<Provider, Error> {
        Ok(self.provider.clone())
    }
    fn close_catalog(&self) -> Result<(), Error> {
        Ok(())
    }
}
fn installed() -> (Arc<Server<Installed>>, Arc<Mutex<Life>>) {
    let life = Arc::new(Mutex::new(Life::default()));
    let factory = Installed {
        provider: Provider {
            life: life.clone(),
            mock: MockBackend::new(),
        },
    };
    (Server::new_atomic_test(factory), life)
}
fn run(server: &Arc<Server<Installed>>, source: &str) -> Reply {
    match server.begin(
        json!(uuid::Uuid::new_v4().to_string()),
        "exec",
        input(source),
    ) {
        Started::Call(call) => server.run(&call),
        Started::Reply(reply) => reply,
        _ => panic!("duplicate"),
    }
}

#[test]
fn stopped_application_launches_then_binds_and_quits_in_one_script() {
    let (server, life) = installed();
    let result=run(&server,"await app.launch();const w=await app.window({title:'Window 101'});state.window=w.window_ref;await w.capture();state.saved=17;return await app.quit();");
    assert_eq!(
        success(&result)["outcome"]["evidence"]["postcondition"],
        "application_exited"
    );
    assert!(result.image.is_some());
    let life = life.lock().unwrap();
    assert_eq!((life.launches, life.quits), (1, 1));
    assert!(life.instances.is_empty());
    drop(life);
    let next=run(&server,"await app.launch();let stale=false;try {await app.capture({window_ref:state.window});}catch(e){stale=e.code==='target_changed';}return {saved:session.state.saved,stale};");
    assert_eq!(success(&next), &json!({"saved":17,"stale":true}));
    assert_eq!(
        run(&server, "return await app.quit();").data["value"]["result"]["outcome"]["evidence"]
            ["postcondition"],
        "application_exited"
    );
    server.close().unwrap();
}

#[test]
fn launch_reuses_running_instance_and_close_retires_only_one_window() {
    let (server, life) = installed();
    let result=run(&server,"await app.launch();const again=await app.launch();const w=await app.window({title:'Window 101'});state.closed=w.window_ref;await w.close();return {again,windows:(await app.windows()).windows};");
    assert_eq!(
        success(&result)["again"]["outcome"]["dispatch"],
        "not_dispatched"
    );
    assert_eq!(success(&result)["windows"].as_array().unwrap().len(), 1);
    assert_eq!(life.lock().unwrap().launches, 1);
    assert_eq!(life.lock().unwrap().instances.len(), 1);
    let stale = run(&server, "await app.capture({window_ref:state.closed});");
    assert_eq!(stale.data["script"]["failure"]["code"], "target_changed");
    success(&run(&server, "await app.quit();"));
    server.close().unwrap();
}

#[test]
fn pending_quit_does_not_report_exit_or_replay() {
    let (server, life) = installed();
    success(&run(&server, "await app.launch();"));
    life.lock().unwrap().quit_pending = true;
    let result=run(&server,"const r=await app.quit();const w=await app.windows();return {receipt:r,remaining:w.windows.length};");
    assert_eq!(
        success(&result)["receipt"]["outcome"]["evidence"]["kind"],
        "unverified"
    );
    assert_eq!(success(&result)["remaining"], 2);
    assert_eq!(life.lock().unwrap().quits, 1);
    assert_eq!(life.lock().unwrap().instances.len(), 1);
    life.lock().unwrap().quit_pending = false;
    success(&run(&server, "await app.quit();"));
    server.close().unwrap();
}

#[test]
fn quit_requires_exact_instance_when_multiple_members_are_running() {
    let (server, life) = installed();
    success(&run(&server, "await app.launch();"));
    let mut other = application();
    other.pid += 1;
    other.generation = 8;
    life.lock().unwrap().instances.push(other);
    let rejected = run(&server, "await app.quit();");
    assert_eq!(rejected.data["script"]["failure"]["code"], "ambiguous_app");
    assert_eq!(life.lock().unwrap().quits, 0);
    success(&run(&server,"const windows=(await app.windows()).windows;await app.quit({window_ref:windows[0].window_ref});"));
    assert_eq!(life.lock().unwrap().instances.len(), 1);
    assert_eq!(life.lock().unwrap().quits, 1);
    success(&run(&server, "await app.quit();"));
    server.close().unwrap();
}

#[test]
fn unverified_launch_without_readback_preserves_dispatch_and_membership() {
    for previous in [vec![], vec![application()]] {
        let life = Arc::new(Mutex::new(Life {
            omit_launch_identity: true,
            unverified_launch: true,
            ..Life::default()
        }));
        let runtime = crate::runtime::Runtime::for_application(
            Provider {
                life: life.clone(),
                mock: MockBackend::new(),
            },
            APP.into(),
            previous.clone(),
            Duration::from_secs(30),
        )
        .unwrap();
        let receipt = runtime.launch_until(Instant::now() + Duration::from_secs(2));
        assert_eq!(receipt.outcome.dispatch, DispatchState::Dispatched);
        assert_eq!(receipt.outcome.evidence, EffectEvidence::Unverified);
        assert!(receipt.outcome.error.is_none());
        assert_eq!(runtime.instances(), previous);
        assert_eq!(life.lock().unwrap().launches, 1);
    }
}

#[test]
fn confirmed_noop_launch_binds_returned_running_instances() {
    let life = Arc::new(Mutex::new(Life {
        instances: vec![application()],
        ..Life::default()
    }));
    let runtime = crate::runtime::Runtime::for_application(
        Provider {
            life: life.clone(),
            mock: MockBackend::new(),
        },
        APP.into(),
        vec![],
        Duration::from_secs(30),
    )
    .unwrap();
    let receipt = runtime.launch_until(Instant::now() + Duration::from_secs(2));
    assert_eq!(receipt.outcome.dispatch, DispatchState::NotDispatched);
    assert_eq!(
        receipt.outcome.evidence,
        EffectEvidence::Confirmed("application_running".into())
    );
    assert!(receipt.outcome.error.is_none());
    assert_eq!(runtime.instances(), vec![application()]);
    assert_eq!(life.lock().unwrap().launches, 0);
}

#[test]
fn launch_ack_without_native_identity_is_unknown_and_does_not_authorize_input() {
    let (server, life) = installed();
    life.lock().unwrap().omit_launch_identity = true;
    let rejected = run(&server, "await app.launch();");
    assert!(rejected.is_error);
    assert_eq!(
        rejected.data["script"]["steps"][0]["receipt"]["outcome"]["dispatch"],
        "unknown"
    );
    assert_eq!(
        rejected.data["script"]["steps"][0]["receipt"]["outcome"]["error"]["code"],
        "invalid_observation"
    );
    assert_eq!(life.lock().unwrap().launches, 1);
    // A new execution can discover the process, but never replays the launch.
    let read = run(&server, "return (await app.windows()).windows.length;");
    assert_eq!(success(&read), 2);
    assert_eq!(life.lock().unwrap().launches, 1);
    success(&run(&server, "await app.quit();"));
    server.close().unwrap();
}

#[test]
fn final_view_after_self_terminating_button_requires_separate_exit_proof_and_never_replays() {
    for (live, unreadable, gaps) in [
        (false, false, 0),
        (false, false, 2),
        (true, false, 0),
        (false, true, 0),
    ] {
        let life = Arc::new(Mutex::new(Life {
            exit_on_press: true,
            exit_probe_live: live,
            exit_probe_error: unreadable,
            exit_probe_gaps: gaps,
            ..Life::default()
        }));
        let server = Server::new(Installed {
            provider: Provider {
                life: life.clone(),
                mock: MockBackend::new(),
            },
        });
        let result = run(
            &server,
            "await app.look(); await app.click('w1.e1'); return {clicked:true};",
        );
        if !live && !unreadable {
            assert!(!result.is_error, "{}", result.data);
            assert_eq!(result.data["value"]["view"]["running"], false);
            assert_eq!(
                result.data["value"]["view"]["semantics"]["reason"],
                "app_stopped"
            );
            let reopened = run(&server, "await app.look();");
            assert!(!reopened.is_error, "{}", reopened.data);
            assert!(reopened.data["value"]["view"]["windows"]
                .as_array()
                .is_some_and(|windows| !windows.is_empty()));
            assert_eq!(life.lock().unwrap().launches, 2);
        } else {
            assert!(result.is_error, "{}", result.data);
            assert_eq!(result.data["error"]["code"], "app_changed");
            {
                let mut life = life.lock().unwrap();
                life.exit_probe_live = false;
                life.exit_probe_error = false;
            }
            let reopened = run(&server, "await app.look();");
            assert!(
                !reopened.is_error,
                "An ordinary failed exit read must not seal the context: {}",
                reopened.data
            );
        }
        let life = life.lock().unwrap();
        assert_eq!(life.presses, 1);
        assert_eq!(
            life.quits, 0,
            "Final observation must never repeat termination"
        );
    }
}

#[test]
fn dispatched_quit_can_wait_for_confirmation_without_repeating_input() {
    let life = Arc::new(Mutex::new(Life {
        quit_pending: true,
        quit_dialog: true,
        ..Life::default()
    }));
    let server = Server::new(Installed {
        provider: Provider {
            life: life.clone(),
            mock: MockBackend::new(),
        },
    });
    let result = run(&server, "await app.look(); await app.quit(); return (await app.waitForNewWindow({timeout_ms:80})).windows;");
    assert!(!result.is_error, "{}", result.data);
    assert!(result.data["value"]["result"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w["title"] == "Quit confirmation"));
    let life = life.lock().unwrap();
    assert_eq!((life.launches, life.quits), (1, 1));
}

#[test]
fn wait_after_confirmed_quit_never_cold_starts_the_application() {
    let life = Arc::new(Mutex::new(Life::default()));
    let server = Server::new(Installed {
        provider: Provider {
            life: life.clone(),
            mock: MockBackend::new(),
        },
    });
    let result = run(
        &server,
        "await app.look(); await app.quit(); await app.waitForNewWindow({timeout_ms:30});",
    );
    assert_eq!(
        result.data["error"]["code"], "wait_timeout",
        "{}",
        result.data
    );
    assert_eq!(result.data["value"]["view"]["running"], false);
    let life = life.lock().unwrap();
    assert_eq!((life.launches, life.quits), (1, 1));
}
