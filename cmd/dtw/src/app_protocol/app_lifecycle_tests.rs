use super::*;

#[derive(Default)]
struct Life {
    instances: Vec<AppInstance>,
    generation: u64,
    launches: usize,
    quits: usize,
    closed: Vec<u64>,
    quit_pending: bool,
    omit_launch_identity: bool,
    unverified_launch: bool,
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
            return Err(Error::TargetChanged);
        }
        Ok(test_inventory(
            inventory
                .windows
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
        self.mock.press(target, binding, element, call)
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
