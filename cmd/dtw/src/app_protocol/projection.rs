//! Model-facing execution summary, derived from the original retained receipt.
//! This boundary never changes admission, execution status or recovery records.
use super::Reply;
use serde_json::{json, Map, Value};

pub(super) fn execution(mut reply: Reply) -> Reply {
    // Pre-admission errors have no execution identity or retained ledger.
    if !reply.data["execution_id"].is_string() {
        return reply;
    }
    if let Some(script) = reply.data.get_mut("script").and_then(Value::as_object_mut) {
        if let Some(steps) = script.get("steps").and_then(Value::as_array) {
            let summary = summarize(steps);
            script.remove("steps");
            script.insert("summary".into(), summary);
        }
    }
    let data = reply.data.as_object_mut().expect("execution record object");
    data.remove("app_ref");
    data.remove("operation");
    if data.get("receipt").is_some_and(Value::is_null) {
        data.remove("receipt");
    }
    reply
}

fn summarize(steps: &[Value]) -> Value {
    let mut summary = json!({"steps":steps.len(),"failed":0,"pending":0});
    let mut failed = 0;
    let mut pending = 0;
    let mut dispatch = Map::new();
    let mut evidence = Map::new();
    let mut provider_dispatch = Map::new();
    let mut provider_evidence = Map::new();
    let mut interrupted = 0;
    for step in steps {
        failed += usize::from(!step["error"].is_null());
        pending += usize::from(step["status"] != "completed");
        let receipt = &step["receipt"];
        if receipt.is_object() {
            count(&mut dispatch, &receipt["outcome"]["dispatch"]);
            count(&mut evidence, &receipt["outcome"]["evidence"]["kind"]);
            if receipt["provider_outcome"].is_object() {
                count(
                    &mut provider_dispatch,
                    &receipt["provider_outcome"]["dispatch"],
                );
                count(
                    &mut provider_evidence,
                    &receipt["provider_outcome"]["evidence"]["kind"],
                );
            }
            interrupted += usize::from(!receipt["interruption"].is_null());
        }
        if !step["error"].is_null()
            || step["status"] != "completed"
            || outcome_issue(&receipt["outcome"])
            || outcome_issue(&receipt["provider_outcome"])
            || !receipt["interruption"].is_null()
        {
            // One diagnostic sample, never a replacement for all earlier issues.
            summary["last_issue"] = json!({
                "number":step["number"],"line":step["line"],
                "operation":step["operation"],"status":step["status"],
                "error":step["error"],"receipt":receipt
            });
        }
    }
    summary["failed"] = json!(failed);
    summary["pending"] = json!(pending);
    if !dispatch.is_empty() {
        summary["actions"] = json!({"dispatch":dispatch,"evidence":evidence,
            "provider_dispatch":provider_dispatch,"provider_evidence":provider_evidence,
            "interrupted":interrupted});
    }
    summary
}

fn count(counts: &mut Map<String, Value>, kind: &Value) {
    // Missing evidence is not proof that an operation was never dispatched.
    let entry = counts
        .entry(kind.as_str().unwrap_or("unrecorded"))
        .or_insert(json!(0));
    *entry = json!(entry.as_u64().expect("summary counter") + 1);
}

fn outcome_issue(outcome: &Value) -> bool {
    outcome.is_object()
        && (!outcome["error"].is_null()
            || outcome["dispatch"] == "unknown"
            || outcome["evidence"]["kind"] == "mismatch")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn eviction_failure_and_recovery_metadata_survive_projection() {
        let original = Reply {
            data: json!({"version":1,"app":"example.app","execution_id":"execution_old",
                "status":"completed","value":null,"payload_expired":true,
                "context_reset":true,"state_reset":true,"error":{"code":"script_cancelled"},
                "shutdown_error":{"code":"shutdown_failed"},"shutdown_errors":[{"code":"cleanup_failed"}],
                "script":{"failure":{"kind":"cancelled","line":3},"state_committed":false,
                    "shutdown_error":{"code":"shutdown_failed"},"steps":[]}}),
            image: Some(Arc::new("original image".into())),
            is_error: true,
        };
        let projected = execution(original.clone());
        for key in [
            "app",
            "execution_id",
            "status",
            "value",
            "payload_expired",
            "context_reset",
            "state_reset",
            "error",
            "shutdown_error",
            "shutdown_errors",
        ] {
            assert_eq!(projected.data[key], original.data[key]);
        }
        for key in ["failure", "state_committed", "shutdown_error"] {
            assert_eq!(projected.data["script"][key], original.data["script"][key]);
        }
        assert!(projected.is_error);
        assert!(Arc::ptr_eq(
            projected.image.as_ref().unwrap(),
            original.image.as_ref().unwrap()
        ));
        assert!(original.data["script"]["steps"].is_array());
    }

    #[test]
    fn pre_admission_errors_are_not_given_a_fictitious_execution_summary() {
        let original =
            super::super::problem("invalid_arguments", "Only app and source are accepted");
        let projected = execution(original.clone());
        assert_eq!(projected.data, original.data);
        assert_eq!(projected.is_error, original.is_error);
    }
}
