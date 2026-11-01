//! Scoped condition publication never replaces another instance's inventory.
use super::*;

fn wait_progress(desired: WindowWaitState, samples: u32, probe: &WindowStateProbe) -> Value {
    match probe {
        WindowStateProbe::Pending { reason } => {
            json!({"kind":"window_condition", "state":desired.as_str(), "samples":samples, "status":"pending", "reason":reason})
        }
        WindowStateProbe::Closed => {
            json!({"kind":"window_condition", "state":desired.as_str(), "samples":samples, "status":"closed"})
        }
        WindowStateProbe::Window(found) => {
            json!({"kind":"window_condition", "state":desired.as_str(), "samples":samples, "status":"resolved", "window_kind":found.kind.as_str(), "relation":found.relation.as_str()})
        }
    }
}

impl<B: Backend + 'static> Coordinator<B> {
    pub(super) fn wait_for_window(
        &self,
        submitted: &Submit,
        target: &WindowTarget,
        desired: WindowWaitState,
        timeout_ms: u64,
        deadline: Instant,
    ) -> Work {
        let source_ref = submitted
            .target
            .window_ref
            .as_deref()
            .expect("resolved window reference");
        self.record_resolution(&submitted.request_id, json!({"kind":"window_condition", "state":desired.as_str(), "samples":0, "status":"reading"}));
        let result = self.runtime.wait_for_window_until(
            target,
            desired,
            Duration::from_millis(timeout_ms),
            deadline,
            |samples, probe| {
                self.record_resolution(
                    &submitted.request_id,
                    wait_progress(desired, samples, probe),
                )
            },
        );
        let result = match result {
            Ok(result) => result,
            Err(error) => return Work::read_error(error),
        };
        if let WindowStateProbe::Pending { reason } = &result.probe {
            let details = json!({"state":desired.as_str(), "window_ref":source_ref, "samples":result.samples, "reason":reason});
            let hint = match desired {
                WindowWaitState::Closed | WindowWaitState::Unblocked =>
                    " A confirmation may have opened another modal. Inspect the current window or use app.waitForWindow({window_ref,state:'dialog',timeout_ms:0}) on the same anchor before deciding on new input; do not repeat the prior close or confirmation.",
                WindowWaitState::Dialog =>
                    " Refresh app.windows() and inspect the current UI; do not repeat the action that was expected to open a dialog.",
            };
            return Work {
                completed: Completed { value: None, image: None, receipt: None,
                    error: Some(WireError { code:"window_wait_timeout".into(),
                        message:format!("The scoped window condition was not proven before the wait ended ({reason}); no input was dispatched.{hint}"),
                        details:Some(details) }), payload_bytes:0, payload_expired:false },
                publication:Publication::None, state_update:None,
            };
        }
        let state = self.state.lock().unwrap();
        let (value, bound) =
            match publish_wait_result(&state.windows, source_ref, target, desired, result.probe) {
                Ok(publication) => publication,
                Err(error) => return Work::read_error(error),
            };
        Work {
            completed: Completed {
                value: Some(value),
                image: None,
                receipt: None,
                error: None,
                payload_bytes: 0,
                payload_expired: false,
            },
            publication: Publication::Windows(bound),
            state_update: None,
        }
    }
}

fn publish_wait_result(
    current: &HashMap<String, WindowTarget>,
    source_ref: &str,
    target: &WindowTarget,
    desired: WindowWaitState,
    probe: WindowStateProbe,
) -> Result<(Value, HashMap<String, WindowTarget>), Error> {
    let mut bound = current.clone();
    match probe {
        WindowStateProbe::Closed => {
            bound.retain(|_, value| value != target);
            Ok((json!({"state":"closed", "window_ref":source_ref}), bound))
        }
        WindowStateProbe::Window(found) => {
            let key = match current
                .iter()
                .find(|(_, target)| **target == found.window.target)
                .map(|(key, _)| key.clone())
            {
                Some(key) => key,
                None => reference("win")?,
            };
            // A new generation of this one surface retires only that slot.
            // Unavailable siblings and other process instances remain bound.
            bound.retain(|_, old| {
                old.app != found.window.target.app
                    || old.native_id != found.window.target.native_id
                    || *old == found.window.target
            });
            bound.insert(key.clone(), found.window.target);
            Ok((
                json!({"state":desired.as_str(), "window_ref":key, "kind":found.kind.as_str(), "relation":found.relation.as_str()}),
                bound,
            ))
        }
        WindowStateProbe::Pending { .. } => unreachable!("pending cannot publish window identity"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn target(pid: u32, id: u64, generation: u64) -> WindowTarget {
        WindowTarget {
            app: AppInstance {
                identity: AppIdentity("wait.app".into()),
                pid,
                generation: 1,
            },
            native_id: id,
            generation,
        }
    }
    #[test]
    fn window_wait_publication_is_scoped_and_preserves_other_instances() {
        let anchor = target(4, 10, 1);
        let old = target(4, 11, 1);
        let next = target(4, 11, 2);
        let sibling = target(5, 11, 1);
        let current = HashMap::from([
            ("anchor".into(), anchor.clone()),
            ("old_dialog".into(), old),
            ("sibling".into(), sibling.clone()),
        ]);
        let (value, bound) = publish_wait_result(
            &current,
            "anchor",
            &anchor,
            WindowWaitState::Dialog,
            WindowStateProbe::Window(MatchedWindow {
                window: WindowInfo {
                    target: next.clone(),
                    title: String::new(),
                    visible: true,
                },
                kind: WindowKind::Sheet,
                relation: WindowRelation::ChildModal,
            }),
        )
        .unwrap();
        assert_eq!(value["state"], "dialog");
        assert_eq!(value["kind"], "sheet");
        assert_eq!(bound["anchor"], anchor);
        assert_eq!(bound["sibling"], sibling);
        assert!(!bound.contains_key("old_dialog"));
        super::super::tests::assert_reference(value["window_ref"].as_str().unwrap(), "win");
        assert_eq!(bound[value["window_ref"].as_str().unwrap()], next);
    }
    #[test]
    fn window_wait_closed_keeps_identifier_but_retires_only_exact_target() {
        let anchor = target(4, 10, 1);
        let sibling = target(5, 10, 1);
        let current = HashMap::from([
            ("anchor".into(), anchor.clone()),
            ("sibling".into(), sibling.clone()),
        ]);
        let (value, bound) = publish_wait_result(
            &current,
            "anchor",
            &anchor,
            WindowWaitState::Closed,
            WindowStateProbe::Closed,
        )
        .unwrap();
        assert_eq!(value, json!({"state":"closed","window_ref":"anchor"}));
        assert!(!bound.contains_key("anchor"));
        assert_eq!(bound["sibling"], sibling);
    }
}
