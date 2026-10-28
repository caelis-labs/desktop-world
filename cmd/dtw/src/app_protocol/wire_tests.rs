use super::*;
use computer::{ActionOutcome, DispatchState, EffectEvidence, Error};

fn submit(command: Value) -> Value {
    let mut target = json!({"app_ref": "app-a"});
    if command["kind"] != "windows" && command["kind"] != "script" {
        target["window_ref"] = json!("window-a");
    }
    json!({"version": 1, "operation": "submit", "request_id": "request-a",
        "target": target, "command": command})
}

fn commands() -> Vec<Value> {
    vec![
        json!({"kind": "windows"}),
        json!({"kind": "observe"}),
        json!({"kind": "capture"}),
        json!({"kind": "press", "observation_ref": "observation-a", "element_ref": "element-a"}),
        json!({"kind": "click", "observation_ref": "observation-a", "x": 0.5, "y": 42}),
    ]
}

#[test]
fn parses_the_complete_public_request_union() {
    for (operation, expected) in [("describe", Request::Describe), ("close", Request::Close)] {
        assert_eq!(
            parse_request(json!({"version": 1, "operation": operation})),
            Ok(expected)
        );
    }
    for (operation, expected) in [
        (
            "result",
            Request::Result {
                request_id: "original-id".into(),
            },
        ),
        (
            "cancel",
            Request::Cancel {
                request_id: "original-id".into(),
            },
        ),
    ] {
        assert_eq!(
            parse_request(
                json!({"version": 1, "operation": operation, "request_id": "original-id"})
            ),
            Ok(expected)
        );
    }
    let expected = [
        Command::Windows,
        Command::Observe,
        Command::Capture,
        Command::Press {
            observation_ref: "observation-a".into(),
            element_ref: "element-a".into(),
        },
        Command::Click {
            observation_ref: "observation-a".into(),
            x: 0.5,
            y: 42.0,
        },
    ];
    for (command, expected) in commands().into_iter().zip(expected) {
        assert_eq!(
            parse_request(submit(command)),
            Ok(Request::Submit(Submit {
                request_id: "request-a".into(),
                target: Target {
                    app_ref: "app-a".into(),
                    window_ref: (expected != Command::Windows).then(|| "window-a".into())
                },
                command: expected,
            }))
        );
    }
}

#[test]
fn versions_are_required_and_never_coerced_from_strings_or_booleans() {
    for value in [
        Value::Null,
        json!([]),
        json!("request"),
        json!({}),
        json!({"operation": "describe"}),
    ] {
        assert_eq!(parse_request(value).unwrap_err().code, "invalid_request");
    }
    for version in [json!(null), json!("1"), json!(true), json!({}), json!([])] {
        assert_eq!(
            parse_request(json!({"version": version, "operation": "describe"}))
                .unwrap_err()
                .code,
            "invalid_request"
        );
    }
    for version in [json!(0), json!(2), json!(-1), json!(1.5), json!(u64::MAX)] {
        assert_eq!(
            parse_request(json!({"version": version, "operation": "describe"}))
                .unwrap_err()
                .code,
            "unsupported_version"
        );
    }
    assert_eq!(
        parse_request(json!({"version": 1.0, "operation": "describe"})),
        Ok(Request::Describe)
    );
}

#[test]
fn unknown_fields_cannot_smuggle_authority_or_native_targets() {
    let mut requests = vec![
        json!({"version":1,"operation":"describe"}),
        json!({"version":1,"operation":"close"}),
        json!({"version":1,"operation":"result","request_id":"r"}),
        json!({"version":1,"operation":"cancel","request_id":"r"}),
    ];
    requests.extend(commands().into_iter().map(submit));
    for request in requests {
        for key in ["approved", "grant", "capabilities", "pid", "wait_ms", "id"] {
            let mut forged = request.clone();
            forged[key] = json!(true);
            assert!(parse_request(forged).is_err(), "accepted top-level {key}");
        }
    }
    for command in commands() {
        for key in ["pid", "native_id", "generation", "provider", "approved"] {
            let mut forged = submit(command.clone());
            forged["target"][key] = json!(7);
            assert!(parse_request(forged).is_err(), "accepted target.{key}");
        }
        for key in ["native_token", "grant", "arguments", "approved", "script"] {
            let mut forged = submit(command.clone());
            forged["command"][key] = json!("forged");
            assert!(parse_request(forged).is_err(), "accepted command.{key}");
        }
    }
}

#[test]
fn windows_requires_no_window_and_other_commands_require_one() {
    for value in [json!(null), json!("window-a"), json!(0)] {
        let mut request = submit(json!({"kind": "windows"}));
        request["target"]["window_ref"] = value;
        assert!(parse_request(request).is_err());
    }
    for command in commands().into_iter().skip(1) {
        let mut request = submit(command);
        request["target"]
            .as_object_mut()
            .unwrap()
            .remove("window_ref");
        assert!(parse_request(request.clone()).is_err());
        request["target"]["window_ref"] = Value::Null;
        assert!(parse_request(request).is_err());
    }
}

#[test]
fn identifiers_have_uniform_unicode_length_and_type_limits() {
    let press = submit(json!({"kind":"press","observation_ref":"o","element_ref":"e"}));
    for path in [
        "/request_id",
        "/target/app_ref",
        "/target/window_ref",
        "/command/observation_ref",
        "/command/element_ref",
    ] {
        for length in [1, 128] {
            let mut request = press.clone();
            *request.pointer_mut(path).unwrap() = json!("界".repeat(length));
            assert!(
                parse_request(request).is_ok(),
                "rejected valid {path} length {length}"
            );
        }
        for value in [
            json!(""),
            json!("界".repeat(129)),
            json!(1),
            json!(null),
            json!(false),
        ] {
            let mut request = press.clone();
            *request.pointer_mut(path).unwrap() = value;
            assert!(parse_request(request).is_err(), "accepted invalid {path}");
        }
    }
    for operation in ["result", "cancel"] {
        for request_id in [json!(""), json!("r".repeat(129)), json!(null), json!(1)] {
            assert!(parse_request(
                json!({"version":1,"operation":operation,"request_id":request_id})
            )
            .is_err());
        }
    }
}

#[test]
fn required_fields_and_unknown_commands_are_rejected() {
    for command in commands() {
        let request = submit(command);
        for key in ["request_id", "target", "command"] {
            let mut missing = request.clone();
            missing.as_object_mut().unwrap().remove(key);
            assert!(parse_request(missing).is_err());
        }
        let mut missing = request.clone();
        missing["target"].as_object_mut().unwrap().remove("app_ref");
        assert!(parse_request(missing).is_err());
        for key in request["command"].as_object().unwrap().keys() {
            let mut missing = request.clone();
            missing["command"].as_object_mut().unwrap().remove(key);
            assert!(
                parse_request(missing).is_err(),
                "accepted missing command.{key}"
            );
        }
    }
    for kind in ["run", "launch", "invoke", "", "Observe"] {
        assert!(parse_request(submit(json!({"kind":kind}))).is_err());
    }
    for operation in ["open", "approve", "status", "", "Submit"] {
        assert!(parse_request(json!({"version":1,"operation":operation})).is_err());
    }
}

#[test]
fn coordinate_validation_does_not_replace_core_geometry_checks() {
    for value in [json!(-1.0), json!(0), json!(-0.0), json!(f64::MAX)] {
        let request = submit(json!({"kind":"click","observation_ref":"o","x":value,"y":1}));
        assert!(parse_request(request).is_ok());
    }
    for coordinate in ["x", "y"] {
        for value in [
            json!(null),
            json!("1"),
            json!(true),
            json!([]),
            json!({}),
            json!(f64::INFINITY),
            json!(f64::NAN),
        ] {
            let mut request = submit(json!({"kind":"click","observation_ref":"o","x":1,"y":2}));
            request["command"][coordinate] = value;
            assert!(parse_request(request).is_err());
        }
    }
}

#[test]
fn errors_have_stable_codes_and_preserve_backend_diagnostics_as_data() {
    let errors = [
        (Error::Closed, "closed"),
        (Error::Cancelled, "cancelled"),
        (Error::Busy, "busy"),
        (Error::WrongApp, "wrong_app"),
        (Error::UnsupportedCapability, "unsupported_capability"),
        (Error::StaleObservation, "stale_observation"),
        (Error::WrongProvider, "wrong_provider"),
        (Error::TargetChanged, "target_changed"),
        (Error::InvalidObservation, "invalid_observation"),
        (Error::InvalidPoint, "invalid_point"),
        (Error::ElementNotFound, "element_not_found"),
        (Error::ElementNotPressable, "element_not_pressable"),
        (Error::TimedOut, "timed_out"),
    ];
    for (error, expected) in errors {
        assert_eq!(core_error(&error).code, expected);
        assert!(!core_error(&error).message.is_empty());
    }
    let diagnostic = "native transport: Shutdown(\"kill failed\")\n完整原始诊断";
    let error = core_error(&Error::BackendFailed(diagnostic.into()));
    assert_eq!(error.code, "backend_failed");
    assert_eq!(error.message, diagnostic);
}

#[test]
fn cancellation_receipts_preserve_distinct_context_and_provider_evidence() {
    let receipt = runtime::ActionReceipt {
        outcome: ActionOutcome::unknown(Error::Cancelled),
        provider_outcome: Some(ActionOutcome {
            dispatch: DispatchState::Dispatched,
            evidence: EffectEvidence::Confirmed("counter changed from 0 to 1".into()),
            error: None,
        }),
        interruption: Some(Error::Cancelled),
    };
    let value = super::receipt(&receipt);
    assert_eq!(value["outcome"]["dispatch"], "unknown");
    assert_eq!(value["outcome"]["evidence"], json!({"kind":"unverified"}));
    assert_eq!(value["outcome"]["error"]["code"], "cancelled");
    assert_eq!(value["provider_outcome"]["dispatch"], "dispatched");
    assert_eq!(
        value["provider_outcome"]["evidence"],
        json!({"kind":"confirmed","postcondition":"counter changed from 0 to 1"})
    );
    assert_eq!(value["provider_outcome"]["error"], Value::Null);
    assert_eq!(value["interruption"]["code"], "cancelled");
    assert_eq!(
        serde_json::from_str::<Value>(&value.to_string()).unwrap(),
        value
    );
}

#[test]
fn known_rejection_and_mismatch_are_not_reclassified() {
    let rejected = ActionOutcome::rejected(Error::TargetChanged);
    let value = receipt(&runtime::ActionReceipt {
        outcome: rejected.clone(),
        provider_outcome: Some(rejected),
        interruption: Some(Error::Closed),
    });
    assert_eq!(value["outcome"]["dispatch"], "not_dispatched");
    assert_eq!(value["provider_outcome"]["dispatch"], "not_dispatched");
    assert_eq!(value["interruption"]["code"], "closed");
    let value = receipt(&runtime::ActionReceipt {
        outcome: ActionOutcome {
            dispatch: DispatchState::Dispatched,
            evidence: EffectEvidence::Mismatch {
                expected: "Count: 1".into(),
                observed: "Count: 0".into(),
            },
            error: None,
        },
        provider_outcome: None,
        interruption: None,
    });
    assert_eq!(
        value["outcome"]["evidence"],
        json!({"kind":"mismatch","expected":"Count: 1","observed":"Count: 0"})
    );
    assert_eq!(value["outcome"]["error"], Value::Null);
    assert_eq!(value["provider_outcome"], Value::Null);
    assert_eq!(value["interruption"], Value::Null);
}

#[test]
fn shutdown_failure_remains_separate_from_the_cancellation_reason() {
    let value = receipt(&runtime::ActionReceipt {
        outcome: ActionOutcome::unknown(Error::Cancelled),
        provider_outcome: Some(ActionOutcome::unknown(Error::BackendFailed(
            "native transport: Shutdown(\"kill failed\")".into(),
        ))),
        interruption: Some(Error::Cancelled),
    });
    assert_eq!(value["provider_outcome"]["dispatch"], "unknown");
    assert_eq!(value["provider_outcome"]["error"]["code"], "backend_failed");
    assert_eq!(
        value["provider_outcome"]["error"]["message"],
        "native transport: Shutdown(\"kill failed\")"
    );
    assert_eq!(value["interruption"]["code"], "cancelled");
}

#[test]
fn scripts_are_app_bound_with_caller_selected_execution_budgets() {
    let request = submit(json!({"kind":"script","source":"return 42;"}));
    assert_eq!(
        parse_request(request.clone()),
        Ok(Request::Submit(Submit {
            request_id: "request-a".into(),
            target: Target {
                app_ref: "app-a".into(),
                window_ref: None
            },
            command: Command::Script {
                source: "return 42;".into(),
                timeout_ms: crate::app_protocol::scripts::DEFAULT_TIMEOUT_MS,
                max_calls: crate::app_protocol::scripts::UNLIMITED_CALLS
            }
        }))
    );
    for (key, values) in [
        (
            "timeout_ms",
            vec![
                json!(0),
                json!(-1),
                json!(u32::MAX as u64 + 1),
                json!(1.5),
                json!("1"),
                json!(null),
            ],
        ),
        (
            "max_calls",
            vec![
                json!(0),
                json!(u32::MAX as u64 + 1),
                json!(1.5),
                json!(true),
                json!(null),
            ],
        ),
    ] {
        for value in values {
            let mut bad = request.clone();
            bad["command"][key] = value;
            assert!(parse_request(bad).is_err());
        }
    }
    for (time, calls) in [
        (json!(1), json!(1)),
        (json!(5000.0), json!(32.0)),
        (json!(120000), json!(1000)),
    ] {
        let mut valid = request.clone();
        valid["command"]["timeout_ms"] = time;
        valid["command"]["max_calls"] = calls;
        assert!(parse_request(valid).is_ok());
    }
    for field in ["app_ref", "pid", "approved", "memory_bytes", "native_call"] {
        let mut bad = request.clone();
        bad["command"][field] = json!("forged");
        assert!(parse_request(bad).is_err());
    }
    let mut window = request.clone();
    window["target"]["window_ref"] = json!("window-a");
    assert!(parse_request(window).is_err());
    for source in [
        json!(null),
        json!(42),
        json!("x".repeat(65537)),
        json!("界".repeat(21846)),
    ] {
        let mut bad = request.clone();
        bad["command"]["source"] = source;
        assert!(parse_request(bad).is_err());
    }
    let mut boundary = request;
    boundary["command"]["source"] = json!("x".repeat(65536));
    assert!(parse_request(boundary).is_ok());
    let schema = input_schema();
    let scripts: Vec<_> = schema["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["properties"]["command"]["properties"]["kind"]["const"] == "script")
        .collect();
    assert_eq!(scripts.len(), 1);
    let schema = scripts[0];
    assert_eq!(
        schema["properties"]["target"]["required"],
        json!(["app_ref"])
    );
    assert_eq!(
        schema["properties"]["command"]["properties"]["timeout_ms"]["maximum"],
        crate::app_protocol::scripts::MAX_TIMEOUT_MS
    );
    assert_eq!(
        schema["properties"]["command"]["properties"]["max_calls"]["maximum"],
        crate::app_protocol::scripts::MAX_CALLS
    );
}
