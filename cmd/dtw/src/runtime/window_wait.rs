//! A condition wait serializes the app context, but holds no input/scene lock
//! between probes. Native workers finish and reap their scoped helpers per read.
use super::*;

fn validate_probe(
    target: &WindowTarget,
    state: WindowWaitState,
    probe: WindowStateProbe,
) -> Result<WindowStateProbe, Error> {
    let valid = match &probe {
        WindowStateProbe::Pending { reason } => !reason.is_empty() && reason.len() <= 128,
        WindowStateProbe::Closed => state == WindowWaitState::Closed,
        WindowStateProbe::Window(found) => {
            if found.window.target.app != target.app {
                return Err(Error::WrongApp);
            }
            found.window.target.native_id != 0
                && found.window.target.generation != 0
                && found.window.title.len() <= 4096
                && found.window.visible
                && match state {
                    WindowWaitState::Closed => false,
                    WindowWaitState::Unblocked => {
                        found.window.target == *target
                            && found.relation == WindowRelation::SameWindow
                    }
                    WindowWaitState::Dialog => {
                        found.window.target != *target
                            && found.kind != WindowKind::Window
                            && found.relation != WindowRelation::SameWindow
                    }
                }
        }
    };
    if valid {
        Ok(probe)
    } else {
        Err(Error::InvalidObservation)
    }
}

impl<B: Backend, C: Clock> Runtime<B, C> {
    pub fn supported_window_wait_states(&self) -> &'static [WindowWaitState] {
        self.backend.supported_window_wait_states()
    }

    /// A condition timeout is a normal pending result, not a worker/script
    /// timeout. An in-flight read keeps the original caller budget; do not turn
    /// the last few condition milliseconds into a child-termination deadline.
    pub fn wait_for_window_until(
        &self,
        target: &WindowTarget,
        desired: WindowWaitState,
        timeout: Duration,
        deadline: Instant,
        mut progress: impl FnMut(u32, &WindowStateProbe),
    ) -> Result<WindowWaitResult, Error> {
        let flight = {
            let mut state = self.state();
            if state.busy {
                return Err(Error::Busy);
            }
            self.validate_target(&state, &target.app, Capability::Windows)?;
            if !self
                .backend
                .supported_window_wait_states()
                .contains(&desired)
            {
                return Err(Error::UnsupportedCapability);
            }
            self.check_deadline(Some(deadline))?;
            state.busy = true;
            Flight::new(&self.state)
        };
        let provider = self.backend.stamp();
        let until = self.clock.now() + timeout.min(Duration::from_secs(30));
        let result = (|| {
            let mut samples = 0u32;
            loop {
                self.remaining(deadline, Some(deadline))?;
                if self.backend.stamp() != provider {
                    return Err(Error::WrongProvider);
                }
                let probe_deadline = deadline.min(self.clock.now() + MAX_CALL_DURATION);
                let poll = || self.remaining(probe_deadline, Some(deadline));
                let call = CallContext { poll: &poll };
                let probe = validate_probe(
                    target,
                    desired,
                    self.backend.probe_window_state(target, desired, &call)?,
                )?;
                self.remaining(deadline, Some(deadline))?;
                if self.backend.stamp() != provider {
                    return Err(Error::WrongProvider);
                }
                samples += 1;
                progress(samples, &probe);
                if !matches!(probe, WindowStateProbe::Pending { .. })
                    || timeout.is_zero()
                    || self.clock.now() >= until
                {
                    return Ok(WindowWaitResult { probe, samples });
                }
                // No native worker is in flight while sleeping. Cancellation is
                // checked at most 20ms later, without another discovery call.
                let delay = until
                    .saturating_duration_since(self.clock.now())
                    .min(deadline.saturating_duration_since(self.clock.now()))
                    .min(Duration::from_millis(20));
                std::thread::sleep(delay);
                if self.clock.now() >= until {
                    return Ok(WindowWaitResult { probe, samples });
                }
            }
        })();
        let mut state = self.state();
        let result = self
            .live_context(&state)
            .and_then(|_| self.check_deadline(Some(deadline)))
            .and_then(|_| {
                if self.backend.stamp() == provider {
                    Ok(())
                } else {
                    Err(Error::WrongProvider)
                }
            })
            .and(result);
        let retired = match &result {
            Ok(WindowWaitResult {
                probe: WindowStateProbe::Closed,
                ..
            }) => take_observations(&mut state, |entry| entry.target == *target),
            Ok(WindowWaitResult {
                probe: WindowStateProbe::Window(found),
                ..
            }) => take_observations(&mut state, |entry| {
                entry.target.app == found.window.target.app
                    && entry.target.native_id == found.window.target.native_id
                    && entry.target != found.window.target
            }),
            _ => Vec::new(),
        };
        drop(state);
        // Releasing a NativeBinding can perform a worker RPC. Keep admission
        // until that cleanup finishes, without holding the runtime state lock.
        drop(retired);
        let state = flight.finish();
        self.live_context(&state)?;
        self.check_deadline(Some(deadline))?;
        if self.backend.stamp() != provider {
            return Err(Error::WrongProvider);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{mpsc, Arc};

    struct WaitBinding(Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>);
    impl Drop for WaitBinding {
        fn drop(&mut self) {
            if let Some((entered, release)) = self.0.take() {
                let _ = entered.send(());
                let _ = release.recv();
            }
        }
    }

    #[derive(Clone)]
    struct ClockState(Arc<Mutex<Instant>>);
    impl Clock for ClockState {
        fn now(&self) -> Instant {
            *self.0.lock().unwrap()
        }
    }
    #[derive(Clone)]
    struct ProbeBackend {
        replies: Arc<Mutex<VecDeque<Result<WindowStateProbe, Error>>>>,
        calls: Arc<Mutex<usize>>,
        clock: ClockState,
        supported: bool,
        drop_gate: Arc<Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>>>,
    }
    impl Backend for ProbeBackend {
        type Binding = WaitBinding;
        fn stamp(&self) -> ProviderStamp {
            ProviderStamp {
                id: "wait-test".into(),
                generation: 1,
            }
        }
        fn supports(&self, _: Capability) -> bool {
            true
        }
        fn supported_window_wait_states(&self) -> &'static [WindowWaitState] {
            if self.supported {
                &[
                    WindowWaitState::Dialog,
                    WindowWaitState::Unblocked,
                    WindowWaitState::Closed,
                ]
            } else {
                &[]
            }
        }
        fn probe_window_state(
            &self,
            _: &WindowTarget,
            _: WindowWaitState,
            call: &CallContext<'_>,
        ) -> Result<WindowStateProbe, Error> {
            assert!(call.remaining()? > Duration::from_secs(1));
            *self.calls.lock().unwrap() += 1;
            *self.clock.0.lock().unwrap() += Duration::from_millis(30);
            self.replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("no extra probe")
        }
        fn observe(
            &self,
            target: &WindowTarget,
            _: &CallContext<'_>,
        ) -> Result<BackendObservation<WaitBinding>, Error> {
            Ok(BackendObservation {
                target: target.clone(),
                binding: WaitBinding(self.drop_gate.lock().unwrap().take()),
                image: None,
                discovery: None,
                elements: vec![],
            })
        }
        fn press(
            &self,
            _: &WindowTarget,
            _: &WaitBinding,
            _: ElementId,
            _: &CallContext<'_>,
        ) -> ActionOutcome {
            panic!("never replay input")
        }
    }
    fn target(id: u64) -> WindowTarget {
        WindowTarget {
            app: AppInstance {
                identity: AppIdentity("wait.app".into()),
                pid: 42,
                generation: 7,
            },
            native_id: id,
            generation: 3,
        }
    }
    fn pending() -> WindowStateProbe {
        WindowStateProbe::Pending {
            reason: "dialog_not_present".into(),
        }
    }
    fn dialog() -> WindowStateProbe {
        WindowStateProbe::Window(MatchedWindow {
            window: WindowInfo {
                target: target(2),
                title: String::new(),
                visible: true,
            },
            kind: WindowKind::Sheet,
            relation: WindowRelation::ChildModal,
        })
    }
    fn setup(
        replies: Vec<Result<WindowStateProbe, Error>>,
        supported: bool,
    ) -> (Runtime<ProbeBackend, ClockState>, ProbeBackend, Instant) {
        let clock = ClockState(Arc::new(Mutex::new(Instant::now())));
        let deadline = clock.now() + Duration::from_secs(10);
        let backend = ProbeBackend {
            replies: Arc::new(Mutex::new(replies.into())),
            calls: Arc::default(),
            clock: clock.clone(),
            supported,
            drop_gate: Arc::default(),
        };
        (
            Runtime::with_clock(
                backend.clone(),
                target(1).app,
                Duration::from_secs(15),
                clock,
            ),
            backend,
            deadline,
        )
    }
    #[test]
    fn window_wait_probes_until_unique_match_without_input() {
        let (runtime, backend, deadline) = setup(vec![Ok(pending()), Ok(dialog())], true);
        let mut seen = Vec::new();
        let result = runtime
            .wait_for_window_until(
                &target(1),
                WindowWaitState::Dialog,
                Duration::from_millis(100),
                deadline,
                |n, _| seen.push(n),
            )
            .unwrap();
        assert_eq!(result.probe, dialog());
        assert_eq!(seen, vec![1, 2]);
        assert_eq!(*backend.calls.lock().unwrap(), 2);
    }
    #[test]
    fn window_wait_zero_is_one_probe_and_condition_timeout_keeps_context() {
        let (runtime, backend, deadline) = setup(vec![Ok(pending()), Ok(dialog())], true);
        assert_eq!(
            runtime
                .wait_for_window_until(
                    &target(1),
                    WindowWaitState::Dialog,
                    Duration::ZERO,
                    deadline,
                    |_, _| {}
                )
                .unwrap()
                .probe,
            pending()
        );
        assert_eq!(*backend.calls.lock().unwrap(), 1);
        assert_eq!(
            runtime
                .wait_for_window_until(
                    &target(1),
                    WindowWaitState::Dialog,
                    Duration::ZERO,
                    deadline,
                    |_, _| {}
                )
                .unwrap()
                .probe,
            dialog()
        );
    }
    #[test]
    fn window_wait_does_not_launch_probe_after_condition_deadline() {
        let (runtime, backend, deadline) = setup(vec![Ok(pending())], true);
        assert_eq!(
            runtime
                .wait_for_window_until(
                    &target(1),
                    WindowWaitState::Dialog,
                    Duration::from_millis(1),
                    deadline,
                    |_, _| {}
                )
                .unwrap()
                .samples,
            1
        );
        assert_eq!(*backend.calls.lock().unwrap(), 1);
    }
    #[test]
    fn window_wait_preserves_native_error_and_rejects_cross_app_or_wrong_state() {
        let failure = Error::NativeFailure {
            code: "permission_denied".into(),
            message: "original".into(),
        };
        let (runtime, _, deadline) = setup(vec![Err(failure.clone())], true);
        assert_eq!(
            runtime.wait_for_window_until(
                &target(1),
                WindowWaitState::Dialog,
                Duration::ZERO,
                deadline,
                |_, _| {}
            ),
            Err(failure)
        );
        assert_eq!(
            validate_probe(&target(1), WindowWaitState::Unblocked, dialog()),
            Err(Error::InvalidObservation)
        );
        assert_eq!(
            validate_probe(
                &target(1),
                WindowWaitState::Dialog,
                WindowStateProbe::Closed
            ),
            Err(Error::InvalidObservation)
        );
        let mut other = match dialog() {
            WindowStateProbe::Window(value) => value,
            _ => unreachable!(),
        };
        other.window.target.app.pid = 43;
        assert_eq!(
            validate_probe(
                &target(1),
                WindowWaitState::Dialog,
                WindowStateProbe::Window(other)
            ),
            Err(Error::WrongApp)
        );
    }
    #[test]
    fn window_wait_original_deadline_and_cancel_remain_terminal_without_more_reads() {
        let (runtime, backend, deadline) = setup(vec![Ok(pending())], true);
        let expired = runtime.clock.now();
        assert_eq!(
            runtime.wait_for_window_until(
                &target(1),
                WindowWaitState::Dialog,
                Duration::ZERO,
                expired,
                |_, _| {}
            ),
            Err(Error::TimedOut)
        );
        assert_eq!(*backend.calls.lock().unwrap(), 0);
        let result = runtime.wait_for_window_until(
            &target(1),
            WindowWaitState::Dialog,
            Duration::from_secs(2),
            deadline,
            |_, _| {
                runtime.cancel().unwrap();
            },
        );
        assert_eq!(result, Err(Error::Cancelled));
        assert_eq!(*backend.calls.lock().unwrap(), 1);
    }
    #[test]
    fn window_wait_holds_admission_through_retired_binding_cleanup_and_rechecks_cancel() {
        let (runtime, backend, deadline) = setup(vec![Ok(WindowStateProbe::Closed)], true);
        let runtime = Arc::new(runtime);
        let (entered, entered_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        *backend.drop_gate.lock().unwrap() = Some((entered, release_rx));
        let observation = runtime.observe(&target(1)).unwrap();
        let worker = runtime.clone();
        let thread = std::thread::spawn(move || {
            worker.wait_for_window_until(
                &target(1),
                WindowWaitState::Closed,
                Duration::ZERO,
                deadline,
                |_, _| {},
            )
        });
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(runtime.windows(), Err(Error::Busy));
        assert!(!runtime.retains_observation(observation.reference));
        runtime.cancel().unwrap();
        release.send(()).unwrap();
        assert_eq!(thread.join().unwrap(), Err(Error::Cancelled));
    }
    #[test]
    fn window_wait_other_platform_is_explicitly_unsupported_without_probe() {
        let (runtime, backend, deadline) = setup(vec![], false);
        assert_eq!(
            runtime.wait_for_window_until(
                &target(1),
                WindowWaitState::Closed,
                Duration::ZERO,
                deadline,
                |_, _| {}
            ),
            Err(Error::UnsupportedCapability)
        );
        assert_eq!(*backend.calls.lock().unwrap(), 0);
    }
}
