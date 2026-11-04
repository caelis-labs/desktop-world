//! Convenience resolution uses the existing observation owner and action path.
//! A complete fresh traversal proves uniqueness; it is not a UI snapshot lock.
use super::*;
use wire::{ElementSelector, ElementWaitState, SelectorScope};

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
        self.select_element_in_scene_until(
            submitted, selector, timeout_ms, deadline, wait_until, None,
        )
    }

    pub(super) fn select_scene_element(
        &self,
        submitted: &Submit,
        selector: &ElementSelector,
        timeout_ms: u64,
        deadline: Instant,
    ) -> Work {
        let instance = {
            let state = self.state.lock().unwrap();
            submitted
                .target
                .window_ref
                .as_ref()
                .and_then(|reference| state.windows.get(reference))
                .map(|target| target.app.clone())
        };
        let Some(instance) = instance else {
            return selection_error(submitted, wire::core_error(&Error::TargetChanged));
        };
        let wait_until = (Instant::now() + Duration::from_millis(timeout_ms)).min(deadline);
        self.select_element_in_scene_until(
            submitted,
            selector,
            timeout_ms,
            deadline,
            wait_until,
            Some(instance),
        )
    }

    fn select_element_in_scene_until(
        &self,
        submitted: &Submit,
        selector: &ElementSelector,
        timeout_ms: u64,
        deadline: Instant,
        wait_until: Instant,
        instance: Option<AppInstance>,
    ) -> Work {
        let condition = match submitted.command {
            Command::Wait { state, .. } => Some(state),
            _ => None,
        };
        let read_only = matches!(
            submitted.command,
            Command::Find { .. } | Command::Wait { .. }
        );
        let absent = condition == Some(ElementWaitState::Absent);
        let mut progress = json!({"kind":"element","attempts":0,"pages":0,"matches":0,"status":"reading",
                "scope":match selector.scope { SelectorScope::Window => "window", SelectorScope::Menu => "menu" }});
        if let Some(condition) = condition {
            progress["kind"] = json!("element_condition");
            progress["state"] = json!(condition.as_str());
        }
        self.record_resolution(&submitted.request_id, progress.clone());
        let mut attempts = 0_u32;
        let mut pages = 0_u32;
        loop {
            // The public app selector follows its current top surface while it
            // is only reading. A popup may arrive after the preceding input.
            // Atomic window selectors and app menus keep their explicit target.
            let mut routed = submitted.clone();
            if let Some(instance) = &instance {
                let inventory =
                    self.scene_atomic(&submitted.request_id, "windows", json!({}), deadline);
                if let Some(error) = inventory.completed.error {
                    return selection_error(submitted, error);
                }
                let rows = super::scene::ordered(
                    inventory.completed.value.unwrap()["windows"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default(),
                );
                let mut state = self.state.lock().unwrap();
                let window = rows.iter().find(|row| {
                    row["visible"] == true
                        && row["scene"]["semantic_candidate"] != false
                        && row["window_ref"]
                            .as_str()
                            .and_then(|reference| state.windows.get(reference))
                            .is_some_and(|target| &target.app == instance)
                });
                let Some(window) = window.and_then(|row| row["window_ref"].as_str()) else {
                    return selection_error(submitted, wire::core_error(&Error::TargetChanged));
                };
                routed.target.window_ref = Some(window.to_owned());
                state.scene.active = routed.target.window_ref.clone();
            }
            let submitted = &routed;
            attempts += 1;
            let mut matches = Vec::<Value>::new();
            let query = ObserveQuery {
                role: selector.role.clone(),
                text: selector.text.clone(),
                limit: 2,
            };
            let mut command = match selector.scope {
                SelectorScope::Window => Command::ObserveQuery(query),
                SelectorScope::Menu => Command::ObserveScoped {
                    query,
                    scope: wire::DiscoveryScope::Menu,
                },
            };
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
                    // Two matches already disprove absence for this sample.
                    // No element is being selected; wait for a fresh traversal
                    // rather than treating these matches as an ambiguity error.
                    if absent {
                        break page;
                    }
                    progress["status"] = json!("ambiguous");
                    self.record_resolution(&submitted.request_id, progress.clone());
                    return selection_error(
                        submitted,
                        selection_resolution_error(
                            &submitted.command,
                            "selector_ambiguous",
                            "The selector matches multiple elements; narrow role or text",
                            &progress,
                        ),
                    );
                }
                // Ownership gaps are sticky within this retained traversal.
                // Stop paging it; a fresh query may still recover within the
                // existing polling window. Ordinary page limits keep paging.
                if page["coverage"]["reason"] == "ownership_unproven" {
                    break page;
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
            let disabled =
                complete && matches.len() == 1 && matches[0]["enabled"] == false && !read_only;
            let condition_met = match condition {
                Some(ElementWaitState::Absent) => matches.is_empty(),
                Some(ElementWaitState::Enabled) => {
                    matches.len() == 1 && matches[0]["enabled"] == true
                }
                Some(ElementWaitState::Disabled) => {
                    matches.len() == 1 && matches[0]["enabled"] == false
                }
                _ => matches.len() == 1 && !disabled,
            };
            if complete && condition_met {
                progress["status"] = json!("resolved");
                self.record_resolution(&submitted.request_id, progress);
                if absent {
                    return Work {
                        completed: Completed {
                            value: Some(
                                json!({"window_ref":submitted.target.window_ref,"state":"absent"}),
                            ),
                            image: None,
                            receipt: None,
                            error: None,
                            payload_bytes: 0,
                            payload_expired: false,
                        },
                        publication: Publication::None,
                        state_update: None,
                    };
                }
                let observation_ref = final_page["observation_ref"]
                    .as_str()
                    .expect("observation ref")
                    .to_owned();
                let element_ref = matches[0]["element_ref"]
                    .as_str()
                    .expect("element ref")
                    .to_owned();
                let command = match &submitted.command {
                    Command::Find { .. } | Command::Wait { .. } => {
                        let mut value = json!({"window_ref":submitted.target.window_ref,
                            "observation_ref":observation_ref,"element_ref":element_ref,
                            "element":{"role":matches[0]["role"],"name":matches[0]["name"],
                                "value":matches[0]["value"],"label":matches[0]["label"],
                                "enabled":matches[0]["enabled"],"actions":matches[0]["actions"],
                                "can_press":matches[0]["can_press"]}});
                        if let Some(condition) = condition {
                            value["state"] = json!(condition.as_str());
                        }
                        return Work {
                            completed: Completed {
                                value: Some(value),
                                image: None,
                                receipt: None,
                                error: None,
                                payload_bytes: 0,
                                payload_expired: false,
                            },
                            publication: Publication::None,
                            state_update: None,
                        };
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
            // Incomplete semantics and menu scope never grant foreground focus.
            // Readiness may settle through reads; the native action still owns
            // exact destination admission and any clean foreground_required error.
            if timeout_ms != 0 && Instant::now() < wait_until {
                progress["status"] = json!("waiting");
                self.record_resolution(&submitted.request_id, progress.clone());
                std::thread::sleep(POLL.min(wait_until.saturating_duration_since(Instant::now())));
            }
            if timeout_ms == 0 || Instant::now() >= wait_until {
                if let Err(error) = self.selection_live(deadline) {
                    progress["status"] = json!("interrupted");
                    progress["error"] = core_error(&error);
                    self.record_resolution(&submitted.request_id, progress.clone());
                    return selection_error(submitted, wire::core_error(&error));
                }
                if let Some(condition) = condition {
                    progress["status"] = json!("timeout");
                    self.record_resolution(&submitted.request_id, progress.clone());
                    return selection_error(submitted, resolution_error(
                        "wait_timeout",
                        &format!("The selector condition '{}' was not observed before the wait ended; only reads were performed. Inspect the original action receipt and current UI instead of repeating input", condition.as_str()),
                        &progress,
                    ));
                }
                let (code, status, message) = if disabled {
                    (
                        "selector_not_ready",
                        "not_ready",
                        "The unique element remained disabled",
                    )
                } else if complete
                    && selector.scope == SelectorScope::Window
                    && selector.role.as_deref() == Some("menu_item")
                {
                    ("selector_not_found", "not_found", "No menu item matched in window scope; for the application's menu use selector:{scope:'menu',role:'menu_item',text:...}")
                } else if complete {
                    (
                        "selector_not_found",
                        "not_found",
                        "No element matched the selector",
                    )
                } else {
                    (
                        "selector_unproven",
                        "unproven",
                        "Discovery remained incomplete; uniqueness is unproven",
                    )
                };
                progress["status"] = json!(status);
                self.record_resolution(&submitted.request_id, progress.clone());
                return selection_error(
                    submitted,
                    selection_resolution_error(&submitted.command, code, message, &progress),
                );
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

fn selection_resolution_error(
    command: &Command,
    code: &str,
    message: &str,
    progress: &Value,
) -> WireError {
    let conclusion = if matches!(command, Command::Find { .. } | Command::Wait { .. }) {
        "No unique element reference was returned; inspect candidates and coverage"
    } else {
        "The selected action was not dispatched"
    };
    resolution_error(code, &format!("{message}. {conclusion}"), progress)
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
