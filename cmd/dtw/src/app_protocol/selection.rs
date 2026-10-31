//! Convenience resolution uses the existing observation owner and action path.
//! A complete fresh traversal proves uniqueness; it is not a UI snapshot lock.
use super::*;
use wire::ElementSelector;

const POLL: Duration = Duration::from_millis(50);

impl<B: Backend + 'static> Coordinator<B> {
    pub(super) fn select_element(
        &self,
        submitted: &Submit,
        selector: &ElementSelector,
        timeout_ms: u64,
        deadline: Instant,
    ) -> Work {
        let wait_until = (Instant::now() + Duration::from_millis(timeout_ms)).min(deadline);
        self.select_element_until(submitted, selector, timeout_ms, deadline, wait_until)
    }

    // Keep the polling window separate from the enclosing execution deadline:
    // a completed native read can legitimately outlive the polling window.
    pub(super) fn select_element_until(
        &self,
        submitted: &Submit,
        selector: &ElementSelector,
        timeout_ms: u64,
        deadline: Instant,
        wait_until: Instant,
    ) -> Work {
        let mut progress =
            json!({"kind":"element","attempts":0,"pages":0,"matches":0,"status":"reading"});
        self.record_resolution(&submitted.request_id, progress.clone());
        let mut attempts = 0_u32;
        let mut pages = 0_u32;
        let mut focus_attempted = false;
        loop {
            attempts += 1;
            let mut matches = Vec::<Value>::new();
            let mut command = Command::ObserveQuery(ObserveQuery {
                role: selector.role.clone(),
                text: selector.text.clone(),
                limit: 2,
            });
            let final_page = loop {
                if let Err(error) = self.selection_live(deadline) {
                    progress["status"] = json!("interrupted");
                    progress["error"] = core_error(&error);
                    self.record_resolution(&submitted.request_id, progress.clone());
                    return selection_error(submitted, wire::core_error(&error));
                }
                // Keep the original execution deadline. The wait window stops
                // new probes; it must not turn a healthy final read into a
                // millisecond transport timeout and retire its provider.
                let mut read = self.execute_atomic(
                    &Submit {
                        command,
                        ..submitted.clone()
                    },
                    deadline,
                );
                pages += 1;
                progress["attempts"] = json!(attempts);
                progress["pages"] = json!(pages);
                if let Some(error) = read.completed.error {
                    progress["status"] = json!("read_failed");
                    progress["error"] = error_json(&error);
                    self.record_resolution(&submitted.request_id, progress.clone());
                    return selection_error(submitted, error);
                }
                let page = read.completed.value.take().expect("observation result");
                // The next cursor and eventual action must resolve through the
                // same public/Core binding maps as an explicit observe call.
                apply_publication(&mut self.state.lock().unwrap(), read.publication);
                matches.extend(
                    page["elements"]
                        .as_array()
                        .expect("observation elements")
                        .iter()
                        .take(2 - matches.len())
                        .cloned(),
                );
                progress["matches"] = json!(matches.len());
                progress["coverage"] = page["coverage"].clone();
                progress["candidates"] = json!(matches.iter().map(candidate).collect::<Vec<_>>());
                progress["status"] = json!("reading");
                self.record_resolution(&submitted.request_id, progress.clone());
                if matches.len() >= 2 {
                    progress["status"] = json!("ambiguous");
                    self.record_resolution(&submitted.request_id, progress.clone());
                    return selection_error(submitted, resolution_error("selector_ambiguous",
                        "The selector matches multiple elements; narrow role or text. The selected action was not dispatched", &progress));
                }
                if let Some(cursor) = page["next"].as_str() {
                    command = Command::ContinueObservation {
                        cursor: cursor.into(),
                    };
                } else {
                    break page;
                }
            };
            let complete = final_page["coverage"]["complete"] == true;
            let disabled = complete
                && matches.len() == 1
                && matches[0]["enabled"] == false
                && !matches!(submitted.command, Command::Find { .. });
            if complete && matches.len() == 1 && !disabled {
                progress["status"] = json!("resolved");
                self.record_resolution(&submitted.request_id, progress);
                let observation_ref = final_page["observation_ref"]
                    .as_str()
                    .expect("observation ref")
                    .to_owned();
                let element_ref = matches[0]["element_ref"]
                    .as_str()
                    .expect("element ref")
                    .to_owned();
                let command = match &submitted.command {
                    Command::Find { .. } => {
                        return Work {
                            completed: Completed {
                                value: Some(json!({"window_ref":submitted.target.window_ref,
                                "observation_ref":observation_ref,"element_ref":element_ref,
                                "element":{"role":matches[0]["role"],"name":matches[0]["name"],
                                    "value":matches[0]["value"],"label":matches[0]["label"],
                                    "enabled":matches[0]["enabled"],"actions":matches[0]["actions"],
                                    "can_press":matches[0]["can_press"]}})),
                                image: None,
                                receipt: None,
                                error: None,
                                payload_bytes: 0,
                                payload_expired: false,
                            },
                            publication: Publication::None,
                            state_update: None,
                        }
                    }
                    Command::PressSelected { .. } => Command::Press {
                        observation_ref,
                        element_ref,
                    },
                    Command::SetValueSelected { value, .. } => Command::SetValue {
                        observation_ref,
                        element_ref,
                        value: value.clone(),
                    },
                    _ => unreachable!("selection command"),
                };
                // Exactly one original action path, including rejection and
                // unknown outcomes. Never return to discovery after dispatch.
                return self.execute_atomic(
                    &Submit {
                        command,
                        ..submitted.clone()
                    },
                    deadline,
                );
            }
            if !focus_attempted
                && timeout_ms != 0
                && matches!(
                    submitted.command,
                    Command::PressSelected { .. } | Command::SetValueSelected { .. }
                )
                && !complete
                && final_page["coverage"]["reason"] == "content_unproven"
            {
                focus_attempted = true;
                progress["status"] = json!("preparing_focus");
                progress["focus_preparation"] = json!({"attempted":true,
                    "trigger":final_page["coverage"],"receipt":null});
                self.record_resolution(&submitted.request_id, progress.clone());
                // Use the ordinary action admission, policy, binding retirement
                // and foreground-return owner. Never focus inside a read RPC.
                let focus = self.execute_atomic(
                    &Submit {
                        command: Command::Input(WindowInput::Focus),
                        ..submitted.clone()
                    },
                    deadline,
                );
                let ready = focus.completed.receipt.as_ref().is_some_and(|receipt| {
                    receipt.outcome.dispatch == DispatchState::Dispatched
                        && receipt.outcome.error.is_none()
                        && receipt.interruption.is_none()
                        && !matches!(receipt.outcome.evidence, EffectEvidence::Mismatch { .. })
                });
                progress["focus_preparation"]["receipt"] = focus
                    .completed
                    .receipt
                    .as_ref()
                    .map(wire::receipt)
                    .unwrap_or(Value::Null);
                if !ready {
                    let error = focus.completed.error.unwrap_or_else(|| resolution_error(
                        "focus_preparation_unproven", "Focus preparation was not proven; the selected action was not dispatched", &progress));
                    progress["status"] = json!("focus_failed");
                    progress["error"] = error_json(&error);
                    self.record_resolution(&submitted.request_id, progress);
                    return selection_error(submitted, error);
                }
                progress["status"] = json!("reading");
                self.record_resolution(&submitted.request_id, progress.clone());
                // One fresh traversal, even if the first native read consumed
                // the polling window. Do not restart that window or repeat the
                // preparation; the original execution deadline still applies.
                continue;
            }
            if !focus_attempted && timeout_ms != 0 && Instant::now() < wait_until {
                progress["status"] = json!("waiting");
                self.record_resolution(&submitted.request_id, progress.clone());
                std::thread::sleep(POLL.min(wait_until.saturating_duration_since(Instant::now())));
            }
            if focus_attempted || timeout_ms == 0 || Instant::now() >= wait_until {
                if let Err(error) = self.selection_live(deadline) {
                    progress["status"] = json!("interrupted");
                    progress["error"] = core_error(&error);
                    self.record_resolution(&submitted.request_id, progress.clone());
                    return selection_error(submitted, wire::core_error(&error));
                }
                let (code, status, message) = if disabled {
                    ("selector_not_ready", "not_ready", "The unique element remained disabled; the selected action was not dispatched")
                } else if complete {
                    (
                        "selector_not_found",
                        "not_found",
                        "No element matched the selector; the selected action was not dispatched",
                    )
                } else {
                    ("selector_unproven", "unproven", "Discovery remained incomplete; uniqueness is unproven and the selected action was not dispatched")
                };
                progress["status"] = json!(status);
                self.record_resolution(&submitted.request_id, progress.clone());
                return selection_error(submitted, resolution_error(code, message, &progress));
            }
        }
    }

    fn selection_live(&self, deadline: Instant) -> Result<(), Error> {
        if let Some(error) = &self.state.lock().unwrap().terminal {
            return Err(error.clone());
        }
        if Instant::now() >= deadline {
            return Err(Error::TimedOut);
        }
        Ok(())
    }
}

fn candidate(row: &Value) -> Value {
    json!({"role":row["role"],"name":row["name"],"label":row["label"],"enabled":row["enabled"],"actions":row["actions"]})
}

fn resolution_error(code: &str, message: &str, progress: &Value) -> WireError {
    WireError {
        code: code.into(),
        message: message.into(),
        details: Some(progress.clone()),
    }
}

fn selection_error(submitted: &Submit, error: WireError) -> Work {
    let mut work = Work::rejected(
        &submitted.command,
        Error::NativeFailure {
            code: error.code.clone(),
            message: error.message.clone(),
        },
    );
    // Retain structured provider/read evidence too; the action receipt remains
    // explicitly not_dispatched when resolution failed before that action.
    work.completed.error = Some(error);
    work
}
