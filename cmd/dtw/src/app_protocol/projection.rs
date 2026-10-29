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
    let mut summary = json!({"steps":steps.len(),"failed":0,"pending":0,"incomplete_reads":0});
    let mut failed = 0;
    let mut pending = 0;
    let mut incomplete_reads = 0;
    let mut recovered_incomplete_reads = 0;
    let mut unrecovered_discoveries = Vec::new();
    let mut dispatch = Map::new();
    let mut evidence = Map::new();
    let mut provider_dispatch = Map::new();
    let mut provider_evidence = Map::new();
    let mut interrupted = 0;
    for step in steps {
        failed += usize::from(!step["error"].is_null());
        pending += usize::from(step["status"] != "completed");
        let inventory_incomplete = incomplete_inventory(&step["inventory"]);
        let discovery_incomplete = incomplete_discovery(&step["discovery"]);
        let incomplete = inventory_incomplete || discovery_incomplete;
        incomplete_reads += usize::from(incomplete);
        let receipt = &step["receipt"];
        if receipt.is_object() || !step["error"].is_null() || step["status"] != "completed" {
            // A later read cannot justify proceeding past an action or failure
            // that happened while earlier discovery was still incomplete.
            unrecovered_discoveries.clear();
        } else if step["operation"] == "observe" {
            let discovery = &step["discovery"];
            if let Some((window, observation)) = discovery_refs(discovery) {
                if discovery_incomplete {
                    unrecovered_discoveries.push((window, observation));
                } else if discovery["complete"] == true && discovery["has_more"] == false {
                    unrecovered_discoveries.retain(|(previous_window, previous_observation)| {
                        let recovered =
                            *previous_window == window && *previous_observation != observation;
                        recovered_incomplete_reads += usize::from(recovered);
                        !recovered
                    });
                }
            }
        }
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
            || incomplete
        {
            // One diagnostic sample, never a replacement for all earlier issues.
            summary["last_issue"] = json!({
                "number":step["number"],"line":step["line"],
                "operation":step["operation"],"status":step["status"],
                "error":step["error"],"receipt":receipt
            });
            if inventory_incomplete {
                // A successful read can still omit an unavailable app member.
                // Preserve that evidence even if JS returned only its windows.
                summary["last_issue"]["inventory"] = step["inventory"].clone();
            }
            if discovery_incomplete {
                summary["last_issue"]["discovery"] = step["discovery"].clone();
            }
        }
    }
    summary["failed"] = json!(failed);
    summary["pending"] = json!(pending);
    summary["incomplete_reads"] = json!(incomplete_reads);
    if recovered_incomplete_reads > 0 {
        summary["recovered_incomplete_reads"] = json!(recovered_incomplete_reads);
    }
    if !dispatch.is_empty() {
        summary["actions"] = json!({"dispatch":dispatch,"evidence":evidence,
            "provider_dispatch":provider_dispatch,"provider_evidence":provider_evidence,
            "interrupted":interrupted});
    }
    summary
}

fn incomplete_inventory(inventory: &Value) -> bool {
    inventory.is_object()
        && (inventory["coverage"]["complete_within_scope"] == false
            || inventory["coverage"]["status"] == "partial"
            || inventory["coverage"]["status"] == "unproven"
            || inventory["diagnostics"]["errors"]
                .as_array()
                .is_some_and(|errors| !errors.is_empty()))
}

fn incomplete_discovery(discovery: &Value) -> bool {
    discovery.is_object() && discovery["complete"] == false && discovery["has_more"] == false
}

fn discovery_refs(discovery: &Value) -> Option<(&str, &str)> {
    Some((
        discovery["window_ref"].as_str().filter(|s| !s.is_empty())?,
        discovery["observation_ref"]
            .as_str()
            .filter(|s| !s.is_empty())?,
    ))
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

    fn inventory_step(number: u32, inventory: Value) -> Value {
        json!({"number":number,"line":number + 1,"operation":"windows","status":"completed",
            "error":null,"receipt":null,"inventory":inventory})
    }

    fn successful_execution(steps: Vec<Value>) -> Reply {
        Reply {
            data: json!({"version":1,"app":"example.app","execution_id":"execution_inventory",
                "status":"completed","value":{"result":[{"window_ref":"healthy_window"}]},
                "error":null,"payload_expired":false,
                "script":{"steps":steps,"failure":null,"state_committed":true,"shutdown_error":null}}),
            image: None,
            is_error: false,
        }
    }

    fn partial_inventory(status: &str) -> Value {
        json!({"coverage":{"scope":"application_instances","status":status,
                "complete_within_scope":false,"instance_count":2,
                "available_instances":1,"unavailable_instances":1},
            "diagnostics":{"provider":"macos.surface","counters":{"supported_count":1},
                "error_codes":[-25200],"errors":[{"code":"member_unavailable","message":"original failure",
                    "details":{"coverage":{"status":"unproven"},"diagnostics":{"error_codes":[-25204]}}}]}})
    }

    fn discovery_step(
        number: u32,
        window: &str,
        observation: &str,
        complete: bool,
        has_more: bool,
    ) -> Value {
        json!({"number":number,"line":number,"operation":"observe","status":"completed",
            "error":null,"receipt":null,"discovery":{
                "window_ref":window,"observation_ref":observation,"complete":complete,
                "reason":if complete || has_more {None} else {Some("node_unavailable")},
                "visited":3,"has_more":has_more}})
    }

    #[test]
    fn fresh_complete_discovery_counts_recovery_without_erasing_prior_warnings() {
        let first = discovery_step(1, "window", "old", false, false);
        let second = discovery_step(2, "window", "second", false, false);
        let action = json!({"number":4,"line":4,"operation":"press","status":"completed",
            "error":null,"receipt":{"outcome":{"dispatch":"dispatched","evidence":{"kind":"unverified"},"error":null},
            "provider_outcome":null,"interruption":null}});
        // An action after proven recovery must not retroactively hide it.
        let original = successful_execution(vec![
            first,
            second.clone(),
            discovery_step(3, "window", "fresh", true, false),
            action,
        ]);
        let projected = execution(original.clone());
        let summary = &projected.data["script"]["summary"];
        assert_eq!(summary["incomplete_reads"], 2);
        assert_eq!(summary["recovered_incomplete_reads"], 2);
        assert_eq!(summary["last_issue"]["number"], 2);
        assert_eq!(summary["last_issue"]["discovery"], second["discovery"]);
        assert_eq!(summary["failed"], 0);
        assert_eq!(summary["actions"]["dispatch"]["dispatched"], 1);
        assert_eq!(original.data["script"]["steps"][1], second);
    }

    #[test]
    fn continuation_other_window_same_query_and_missing_refs_do_not_prove_recovery() {
        let mut missing_ref = discovery_step(2, "window", "fresh", true, false);
        missing_ref["discovery"]["observation_ref"] = Value::Null;
        for later in [
            discovery_step(2, "window", "fresh", false, true),
            discovery_step(2, "other_window", "fresh", true, false),
            discovery_step(2, "window", "old", true, false),
            missing_ref,
        ] {
            let projected = execution(successful_execution(vec![
                discovery_step(1, "window", "old", false, false),
                later,
            ]));
            let summary = &projected.data["script"]["summary"];
            assert_eq!(summary["incomplete_reads"], 1);
            assert!(
                summary.get("recovered_incomplete_reads").is_none(),
                "{summary}"
            );
            assert_eq!(summary["last_issue"]["number"], 1);
        }
    }

    #[test]
    fn action_error_and_pending_step_barriers_permanently_retire_prior_recovery_eligibility() {
        let mut barriers = vec![
            json!({"number":2,"line":2,"operation":"observe","status":"completed",
                "error":{"code":"permission_denied","message":"original native failure"},"receipt":null}),
            json!({"number":2,"line":2,"operation":"observe","status":"running","error":null,"receipt":null}),
        ];
        for dispatch in ["not_dispatched", "dispatched", "unknown"] {
            barriers.push(json!({"number":2,"line":2,"operation":"press","status":"completed",
                "error":null,"receipt":{"outcome":{"dispatch":dispatch,"evidence":{"kind":"unverified"},"error":null},
                "provider_outcome":null,"interruption":null}}));
        }
        for barrier in barriers {
            let original = successful_execution(vec![
                discovery_step(1, "window", "old", false, false),
                barrier.clone(),
                discovery_step(3, "window", "fresh", true, false),
                discovery_step(4, "window", "another_fresh", true, false),
            ]);
            let projected = execution(original.clone());
            let summary = &projected.data["script"]["summary"];
            assert_eq!(summary["incomplete_reads"], 1);
            assert!(
                summary.get("recovered_incomplete_reads").is_none(),
                "{summary}"
            );
            assert_eq!(original.data["script"]["steps"][1], barrier);
            if !barrier["error"].is_null() {
                assert_eq!(summary["last_issue"]["error"], barrier["error"]);
                assert_eq!(summary["failed"], 1);
            }
        }
    }

    #[test]
    fn later_read_recovery_cannot_recover_inventory_or_pre_barrier_discovery() {
        let inventory = inventory_step(1, partial_inventory("partial"));
        let before_barrier = discovery_step(2, "window", "old", false, false);
        let barrier = json!({"number":3,"line":3,"operation":"observe","status":"completed",
            "error":{"code":"backend_failed"},"receipt":null});
        let recent = discovery_step(4, "window", "recent", false, false);
        let projected = execution(successful_execution(vec![
            inventory,
            before_barrier,
            barrier,
            recent.clone(),
            discovery_step(5, "window", "fresh", true, false),
        ]));
        let summary = &projected.data["script"]["summary"];
        assert_eq!(summary["incomplete_reads"], 3);
        assert_eq!(summary["recovered_incomplete_reads"], 1);
        assert_eq!(summary["failed"], 1);
        assert_eq!(summary["last_issue"]["discovery"], recent["discovery"]);
    }

    #[test]
    fn mixed_inventory_warning_cannot_be_fully_recovered_by_complete_observation() {
        let projected = execution(successful_execution(vec![
            inventory_step(1, partial_inventory("partial")),
            discovery_step(2, "window", "old", false, false),
            discovery_step(3, "window", "fresh", true, false),
        ]));
        let summary = &projected.data["script"]["summary"];
        assert_eq!(summary["incomplete_reads"], 2);
        assert_eq!(summary["recovered_incomplete_reads"], 1);
        assert_eq!(summary["failed"], 0);
    }

    #[test]
    fn partial_and_unproven_inventory_remain_visible_without_failing_a_successful_script() {
        for status in ["partial", "unproven"] {
            let inventory = partial_inventory(status);
            let original = successful_execution(vec![inventory_step(1, inventory.clone())]);
            let retained = original.data.clone();
            let projected = execution(original.clone());
            let summary = &projected.data["script"]["summary"];
            assert_eq!(summary["steps"], 1);
            assert_eq!(summary["failed"], 0);
            assert_eq!(summary["pending"], 0);
            assert_eq!(summary["incomplete_reads"], 1);
            assert_eq!(summary["last_issue"]["inventory"], inventory);
            assert_eq!(summary["last_issue"]["status"], "completed");
            assert!(summary["last_issue"]["error"].is_null());
            assert!(!projected.is_error);
            assert_eq!(projected.data["status"], "completed");
            assert_eq!(projected.data["script"]["state_committed"], true);
            assert_eq!(projected.data["value"], retained["value"]);
            assert_eq!(
                original.data, retained,
                "the original ledger must not be projected in place"
            );
        }
    }

    #[test]
    fn incomplete_coverage_and_member_errors_each_make_inventory_visible() {
        for inventory in [
            json!({"coverage":{"complete_within_scope":false},"diagnostics":{"errors":[]}}),
            json!({"coverage":{"status":"partial"},"diagnostics":{"errors":[]}}),
            json!({"coverage":{"status":"unproven"},"diagnostics":{"errors":[]}}),
            json!({"coverage":{},"diagnostics":{"errors":[{"code":"native_member_failed"}]}}),
        ] {
            let projected = execution(successful_execution(vec![inventory_step(
                1,
                inventory.clone(),
            )]));
            assert_eq!(projected.data["script"]["summary"]["incomplete_reads"], 1);
            assert_eq!(
                projected.data["script"]["summary"]["last_issue"]["inventory"],
                inventory
            );
        }
    }

    #[test]
    fn complete_and_empty_inventory_do_not_report_incomplete_reads() {
        for status in ["available", "empty_within_scope"] {
            let inventory = json!({"coverage":{"status":status,"complete_within_scope":true},
                "diagnostics":{"counters":{"excluded_count":1},"error_codes":[-25200],"errors":[]}});
            let projected = execution(successful_execution(vec![inventory_step(1, inventory)]));
            let summary = &projected.data["script"]["summary"];
            assert_eq!(summary["incomplete_reads"], 0);
            assert_eq!(summary["failed"], 0);
            assert!(summary.get("last_issue").is_none());
            assert!(!projected.is_error);
        }
    }

    #[test]
    fn latest_issue_changes_without_losing_prior_incomplete_read_counts_or_action_evidence() {
        let inventory = partial_inventory("partial");
        let action = json!({"number":2,"line":3,"operation":"press","status":"completed",
            "error":{"code":"native_outcome_unknown"},"receipt":{
                "outcome":{"dispatch":"unknown","evidence":{"kind":"unverified"},"error":{"code":"native_outcome_unknown"}},
                "provider_outcome":{"dispatch":"unknown","evidence":{"kind":"unverified"},"error":{"code":"native_outcome_unknown"}},
                "interruption":null},"inventory":null});
        let first = inventory_step(1, inventory.clone());
        let projected = execution(successful_execution(vec![first.clone(), action.clone()]));
        let summary = &projected.data["script"]["summary"];
        assert_eq!(summary["incomplete_reads"], 1);
        assert_eq!(summary["failed"], 1);
        assert_eq!(summary["last_issue"]["number"], 2);
        assert_eq!(summary["last_issue"]["receipt"], action["receipt"]);
        assert!(summary["last_issue"].get("inventory").is_none());

        let projected = execution(successful_execution(vec![
            first,
            action,
            inventory_step(3, inventory.clone()),
        ]));
        let summary = &projected.data["script"]["summary"];
        assert_eq!(summary["incomplete_reads"], 2);
        assert_eq!(summary["failed"], 1);
        assert_eq!(summary["last_issue"]["number"], 3);
        assert_eq!(summary["last_issue"]["inventory"], inventory);
        assert_eq!(summary["actions"]["dispatch"]["unknown"], 1);
        assert_eq!(summary["actions"]["provider_dispatch"]["unknown"], 1);
        assert_eq!(summary["actions"]["interrupted"], 0);
    }

    #[test]
    fn incomplete_inventory_evidence_survives_evicted_return_payload() {
        let inventory = partial_inventory("partial");
        let mut original = successful_execution(vec![inventory_step(1, inventory.clone())]);
        original.data["value"] = Value::Null;
        original.data["payload_expired"] = json!(true);
        let projected = execution(original.clone());
        assert!(projected.data["value"].is_null());
        assert_eq!(projected.data["payload_expired"], true);
        assert_eq!(
            projected.data["script"]["summary"]["last_issue"]["inventory"],
            inventory
        );
        assert_eq!(original.data["script"]["steps"][0]["inventory"], inventory);
    }

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
