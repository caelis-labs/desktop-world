use super::*;

impl<B: Backend, C: Clock> Runtime<B, C> {
    /// Retire only positively exited incarnations after a failed final read.
    /// This never launches, quits, adopts a replacement or repeats any input.
    pub fn retire_exited_instances_until(&self, deadline: Instant) -> Result<usize, Error> {
        let deadline = self
            .bounded_deadline(Some(deadline))
            .expect("bounded deadline");
        let (flight, apps, provider) = {
            let mut state = self.state();
            self.live_context(&state)?;
            if state.busy {
                return Err(Error::Busy);
            }
            self.check_deadline(Some(deadline))?;
            state.busy = true;
            (
                Flight::new(&self.state),
                state.apps.clone(),
                self.backend.stamp(),
            )
        };
        let poll = || self.remaining(deadline, Some(deadline));
        let call = CallContext { poll: &poll };
        let mut exited = Vec::new();
        for app in apps {
            call.remaining()?;
            if self.backend.application_exited(&app, &call)? {
                exited.push(app);
            }
        }
        let retired = {
            let mut state = self.state();
            self.live_context(&state)?;
            self.check_deadline(Some(deadline))?;
            if self.backend.stamp() != provider {
                return Err(Error::WrongProvider);
            }
            state.apps.retain(|app| !exited.contains(app));
            take_observations(&mut state, |observation| {
                exited.contains(&observation.target.app)
            })
        };
        drop(retired);
        let state = flight.finish();
        self.live_context(&state)?;
        self.backend.cleanup_error().map_or(Ok(exited.len()), Err)
    }
    pub fn launch_until(&self, deadline: Instant) -> ActionReceipt {
        let Some(app_id) = &self.application_id else {
            return ActionReceipt::rejected(Error::UnsupportedCapability);
        };
        let deadline = self.bounded_deadline(Some(deadline));
        let prepared = (|| {
            let mut state = self.state();
            self.live_context(&state)?;
            if state.busy {
                return Err(Error::Busy);
            }
            if !self.backend.supports(Capability::Launch) {
                return Err(Error::UnsupportedCapability);
            }
            self.check_deadline(deadline)?;
            state.busy = true;
            Ok((Flight::new(&self.state), self.backend.stamp()))
        })();
        let (flight, provider) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => return ActionReceipt::rejected(error),
        };
        let poll = || self.remaining(deadline.unwrap(), deadline);
        let launched = match poll() {
            Ok(_) => self
                .backend
                .launch_application(app_id, &CallContext { poll: &poll }),
            Err(error) => ApplicationLaunch {
                outcome: ActionOutcome::rejected(error),
                instances: vec![],
            },
        };
        // Dispatch can precede observable startup. An unverified launch with no
        // identities keeps its original receipt and cannot erase proven members.
        // Only a claim of confirmed startup requires an observable identity.
        let mut outcome = launched.outcome;
        if outcome.error.is_none()
            && launched.instances.is_empty()
            && matches!(outcome.evidence, EffectEvidence::Confirmed(_))
        {
            outcome = ActionOutcome::unknown(Error::InvalidObservation);
        }
        let retired = if outcome.error.is_none() && !launched.instances.is_empty() {
            match normalized_instances(launched.instances) {
                Ok(instances) => {
                    let mut state = self.state();
                    let retired =
                        take_observations(&mut state, |o| !instances.contains(&o.target.app));
                    state.apps = instances;
                    retired
                }
                Err(error) => {
                    outcome = ActionOutcome::unknown(error);
                    vec![]
                }
            }
        } else {
            vec![]
        };
        drop(retired);
        let state = flight.finish();
        let interruption = self
            .live_context(&state)
            .err()
            .or_else(|| self.check_deadline(deadline).err())
            .or_else(|| (self.backend.stamp() != provider).then_some(Error::WrongProvider));
        ActionReceipt::from_provider(outcome, interruption)
    }

    /// Menu shortcuts for a proven app with no addressable window. The backend
    /// must independently prove foreground ownership before every key down.
    pub fn app_keypress_until(&self, keys: &[String], deadline: Instant) -> ActionReceipt {
        self.application_input_until(Some(keys), deadline)
    }

    pub fn app_focus_until(&self, deadline: Instant) -> ActionReceipt {
        self.application_input_until(None, deadline)
    }

    fn application_input_until(&self, keys: Option<&[String]>, deadline: Instant) -> ActionReceipt {
        let deadline = self.bounded_deadline(Some(deadline));
        let prepared = (|| {
            let mut state = self.state();
            self.live_context(&state)?;
            if state.busy {
                return Err(Error::Busy);
            }
            if state.apps.len() != 1 {
                return Err(Error::NativeFailure {
                    code: "ambiguous_app".into(),
                    message: "Application shortcut requires one verified running instance".into(),
                });
            }
            let app = state.apps[0].clone();
            self.validate_target(
                &state,
                &app,
                if keys.is_some() {
                    Capability::Keypress
                } else {
                    Capability::Focus
                },
            )?;
            self.check_deadline(deadline)?;
            let retired = take_observations(&mut state, |o| o.target.app == app);
            state.busy = true;
            Ok((app, Flight::new(&self.state), self.backend.stamp(), retired))
        })();
        let (app, flight, provider, retired) = match prepared {
            Ok(v) => v,
            Err(e) => return ActionReceipt::rejected(e),
        };
        drop(retired);
        let poll = || self.remaining(deadline.unwrap(), deadline);
        let outcome = match poll() {
            Ok(_) if self.backend.stamp() != provider => {
                ActionOutcome::rejected(Error::WrongProvider)
            }
            Ok(_) => match keys {
                Some(keys) => {
                    self.backend
                        .keypress_application(&app, keys, &CallContext { poll: &poll })
                }
                None => self
                    .backend
                    .focus_application(&app, &CallContext { poll: &poll }),
            },
            Err(e) => ActionOutcome::rejected(e),
        };
        let state = flight.finish();
        let interruption = self
            .live_context(&state)
            .err()
            .or_else(|| self.check_deadline(deadline).err())
            .or_else(|| (self.backend.stamp() != provider).then_some(Error::WrongProvider));
        ActionReceipt::from_provider(outcome, interruption)
    }

    pub fn quit_until(&self, target: Option<&WindowTarget>, deadline: Instant) -> ActionReceipt {
        let app = {
            let state = self.state();
            if let Err(error) = self.live_context(&state) {
                return ActionReceipt::rejected(error);
            }
            if let Err(error) = self.check_deadline(self.bounded_deadline(Some(deadline))) {
                return ActionReceipt::rejected(error);
            }
            if !self.backend.supports(Capability::Quit) {
                return ActionReceipt::rejected(Error::UnsupportedCapability);
            }
            match target {
                Some(target) => target.app.clone(),
                None if state.apps.len() == 1 => state.apps[0].clone(),
                None if state.apps.is_empty() => {
                    return ActionReceipt {
                        outcome: ActionOutcome {
                            delivery: None,
                            dispatch: DispatchState::NotDispatched,
                            evidence: EffectEvidence::Confirmed("application_not_running".into()),
                            error: None,
                        },
                        provider_outcome: None,
                        interruption: None,
                    }
                }
                None => {
                    return ActionReceipt::rejected(Error::NativeFailure {
                        code: "ambiguous_app".into(),
                        message: "Multiple instances are running; select one through window_ref"
                            .into(),
                    })
                }
            }
        };
        self.lifecycle_action(&app, target, Capability::Quit, deadline)
    }

    pub fn close_window_until(&self, target: &WindowTarget, deadline: Instant) -> ActionReceipt {
        self.lifecycle_action(&target.app, Some(target), Capability::CloseWindow, deadline)
    }

    fn lifecycle_action(
        &self,
        app: &AppInstance,
        target: Option<&WindowTarget>,
        capability: Capability,
        deadline: Instant,
    ) -> ActionReceipt {
        let deadline = self.bounded_deadline(Some(deadline));
        let prepared = (|| {
            let mut state = self.state();
            if state.busy {
                return Err(Error::Busy);
            }
            self.validate_target(&state, app, capability)?;
            self.check_deadline(deadline)?;
            let retired = take_observations(&mut state, |o| {
                o.target.app == *app
                    && (capability == Capability::Quit || target == Some(&o.target))
            });
            state.busy = true;
            Ok((Flight::new(&self.state), self.backend.stamp(), retired))
        })();
        let (flight, provider, retired) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => return ActionReceipt::rejected(error),
        };
        drop(retired);
        let poll = || self.remaining(deadline.unwrap(), deadline);
        let call = CallContext { poll: &poll };
        let outcome = match poll() {
            Err(error) => ActionOutcome::rejected(error),
            Ok(_) if capability == Capability::Quit => self.backend.quit_application(app, &call),
            Ok(_) => self.backend.close_window(target.unwrap(), &call),
        };
        if capability == Capability::Quit
            && outcome.error.is_none()
            && outcome.evidence == EffectEvidence::Confirmed("application_exited".into())
        {
            self.state().apps.retain(|member| member != app);
        }
        let state = flight.finish();
        let interruption = self
            .live_context(&state)
            .err()
            .or_else(|| self.check_deadline(deadline).err())
            .or_else(|| (self.backend.stamp() != provider).then_some(Error::WrongProvider));
        ActionReceipt::from_provider(outcome, interruption)
    }
}
