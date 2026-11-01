//! Windows GDI capture can need an ordinary coordinated foreground preparation.
//! Only the provider's explicit occlusion result permits it; capture reads can
//! repeat, but the single focus action is never replayed.
use super::*;

impl<B: Backend + 'static> Coordinator<B> {
    pub(super) fn capture_with_preparation(
        &self,
        submitted: &Submit,
        target: &WindowTarget,
        window_ref: &str,
        deadline: Instant,
    ) -> Work {
        self.state.lock().unwrap().observations.retain(|snapshot| {
            snapshot.window_ref != window_ref || snapshot.kind != ObservationKind::Pixels
        });
        let first = self.runtime.capture_until(target, deadline);
        let error = match first {
            Ok(observation) => return self.observation(window_ref, observation, None),
            Err(error) if matches!(&error, Error::NativeFailure { code, .. } if code == "windows_capture_occluded") => {
                error
            }
            Err(error) => return Work::read_error(error),
        };
        let mut progress = json!({"kind":"capture", "status":"preparing_focus",
            "focus_preparation":{"attempted":true,"trigger":core_error(&error),"receipt":null}});
        // Atomic diagnostic reads have no retained Host Call ledger. Keep
        // them read-only rather than silently dropping a focus action receipt.
        if !self.try_record_resolution(&submitted.request_id, progress.clone()) {
            return Work::read_error(error);
        }
        // This goes through the normal input admission, service policy, lease,
        // target verification and observation retirement. Never focus inside a
        // native read, where that mutation would have no action evidence.
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
            let error = focus.completed.error.unwrap_or_else(|| WireError {
                code: "focus_preparation_unproven".into(),
                message:
                    "Capture focus preparation was not proven; no second capture was attempted"
                        .into(),
                details: None,
            });
            progress["status"] = json!("focus_failed");
            progress["error"] = error_json(&error);
            self.record_resolution(&submitted.request_id, progress);
            let mut work = Work::read_error(Error::NativeFailure {
                code: error.code.clone(),
                message: error.message.clone(),
            });
            work.completed.error = Some(error);
            return work;
        }
        progress["status"] = json!("capturing");
        self.record_resolution(&submitted.request_id, progress.clone());
        // Fresh bounds and image binding are computed by the native provider.
        // Preserve the original target and deadline; no recursion or retries.
        match self.runtime.capture_until(target, deadline) {
            Ok(observation) => {
                progress["status"] = json!("captured");
                self.record_resolution(&submitted.request_id, progress);
                self.observation(window_ref, observation, None)
            }
            Err(error) => {
                progress["status"] = json!("capture_failed");
                progress["error"] = core_error(&error);
                self.record_resolution(&submitted.request_id, progress);
                Work::read_error(error)
            }
        }
    }
}
