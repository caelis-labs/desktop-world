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
fn scoped_discovery_and_text_pages_have_exclusive_bounded_shapes() {
    for args in [
        json!({"window_ref":"w","scope":"menu","text":"Save"}),
        json!({"window_ref":"w","observation_ref":"o","element_ref":"e"}),
        json!({"window_ref":"w","observation_ref":"o","region":{"x":0,"y":1,"width":20,"height":30}}),
    ] {
        assert!(matches!(
            script_submit("a", "r", "observe", &args).unwrap().command,
            Command::ObserveScoped { .. }
        ));
    }
    for args in [
        json!({"window_ref":"w","scope":"window"}),
        json!({"window_ref":"w","scope":"menu","observation_ref":"o"}),
        json!({"window_ref":"w","element_ref":"e"}),
        json!({"window_ref":"w","observation_ref":"o"}),
        json!({"window_ref":"w","observation_ref":"o","element_ref":"e","region":{"x":0,"y":0,"width":1,"height":1}}),
        json!({"window_ref":"w","observation_ref":"o","region":{"x":0,"y":0,"width":0,"height":1}}),
        json!({"cursor":"c","scope":"menu"}),
    ] {
        assert!(script_submit("a", "r", "observe", &args).is_err(), "{args}");
    }
    let args = json!({"window_ref":"w","observation_ref":"o","element_ref":"e"});
    assert!(matches!(
        script_submit("a", "r", "readText", &args).unwrap().command,
        Command::ReadText {
            offset: 0,
            limit: 4096,
            ..
        }
    ));
    for (field, value) in [
        ("offset", json!(-1)),
        ("offset", json!(4294967296u64)),
        ("offset", json!(1.5)),
        ("limit", json!(1)),
        ("limit", json!(16385)),
        ("scope", json!("menu")),
    ] {
        let mut bad = args.clone();
        bad[field] = value;
        assert!(script_submit("a", "r", "readText", &bad).is_err());
    }
}

#[test]
fn observe_query_and_cursor_forms_are_strict_and_bounded() {
    let query = parse_request(submit(
        json!({"kind":"observe","role":"AXButton","text":"提交","limit":1}),
    ))
    .unwrap();
    assert!(matches!(
        query,
        Request::Submit(Submit {
            command: Command::ObserveQuery(computer::ObserveQuery { limit: 1, .. }),
            ..
        })
    ));
    for command in [
        json!({"kind":"observe","limit":0}),
        json!({"kind":"observe","limit":101}),
        json!({"kind":"observe","limit":1.5}),
        json!({"kind":"observe","role":""}),
        json!({"kind":"observe","text":null}),
        json!({"kind":"observe","text":42}),
        json!({"kind":"observe","role":"x".repeat(65)}),
        json!({"kind":"observe","text":"中".repeat(257)}),
        json!({"kind":"observe","region":{}}),
    ] {
        assert!(parse_request(submit(command)).is_err());
    }
    let cursor = json!({"version":1,"operation":"submit","request_id":"r","target":{"app_ref":"app-a"},"command":{"kind":"observe","cursor":"opaque"}});
    assert!(matches!(
        parse_request(cursor.clone()).unwrap(),
        Request::Submit(Submit {
            command: Command::ContinueObservation { .. },
            ..
        })
    ));
    for (key, value) in [
        ("role", json!("AXButton")),
        ("text", json!("x")),
        ("text", json!("")),
        ("limit", json!(1)),
    ] {
        let mut invalid = cursor.clone();
        invalid["command"][key] = value;
        assert!(parse_request(invalid).is_err());
    }
    let mut invalid = cursor;
    invalid["target"]["window_ref"] = json!("window-a");
    assert!(parse_request(invalid).is_err());
}

#[test]
fn empty_observe_text_is_only_an_omitted_text_filter() {
    for limit in [1, 40, 100] {
        let empty = json!({"window_ref":"w","role":"button","text":"","limit":limit});
        let absent = json!({"window_ref":"w","role":"button","limit":limit});
        let parsed = script_submit("app", "request", "observe", &empty).unwrap();
        assert_eq!(
            parsed,
            script_submit("app", "request", "observe", &absent).unwrap()
        );
        assert_eq!(
            parsed.command,
            Command::ObserveQuery(computer::ObserveQuery {
                role: Some("button".into()),
                text: None,
                limit,
            })
        );
    }
    for text in ["", " ", "  target  "] {
        let parsed = script_submit(
            "app",
            "request",
            "observe",
            &json!({"window_ref":"w","text":text}),
        )
        .unwrap();
        assert_eq!(
            parsed.command,
            Command::ObserveQuery(computer::ObserveQuery {
                role: None,
                text: (!text.is_empty()).then(|| text.into()),
                limit: 40,
            })
        );
    }
    for invalid in [
        json!({"window_ref":"","text":""}),
        json!({"window_ref":"w","role":"","text":""}),
        json!({"window_ref":"w","text":false}),
        json!({"window_ref":"w","text":[]}),
        json!({"window_ref":"w","text":"","pid":42}),
        json!({"cursor":"","text":""}),
        json!({"cursor":"c","text":""}),
        json!({"window_ref":"w","cursor":"c","text":""}),
    ] {
        assert!(
            script_submit("app", "request", "observe", &invalid).is_err(),
            "{invalid}"
        );
    }
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
            delivery: None,
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
            delivery: None,
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

#[test]
fn delivery_is_optional_and_preserves_typed_route_and_focus_evidence() {
    use crate::computer::{FocusReturn, InputDelivery, InputRoute};
    let mut native = ActionOutcome::unknown(Error::Cancelled);
    let missing = super::outcome(&native);
    assert!(missing.get("delivery").is_none());
    for (status, name) in [
        (FocusReturn::Pending, "pending"),
        (FocusReturn::Restored, "restored"),
        (FocusReturn::ApplicationRestored, "application_restored"),
        (FocusReturn::ChangedExternally, "changed_externally"),
        (FocusReturn::PreviousAppClosed, "previous_app_closed"),
        (FocusReturn::Unchanged, "unchanged"),
        (FocusReturn::Unconfirmed, "unconfirmed"),
    ] {
        native.delivery = Some(InputDelivery {
            route: Some(InputRoute::Foreground),
            reason: Some("background_unavailable".into()),
            foreground_borrowed: true,
            focus_return: Some(status),
        });
        let value = super::outcome(&native);
        assert_eq!(
            value["delivery"],
            json!({"route":"foreground","reason":"background_unavailable",
            "foreground_borrowed":true,"focus_return":name})
        );
        assert_eq!(value["dispatch"], "unknown");
    }
}

#[test]
fn scroll_anchor_forms_are_exclusive_and_keep_wheel_units() {
    let pixel = script_submit(
        "app-a",
        "r",
        "scroll",
        &json!({"window_ref":"w","observation_ref":"o","x":2.5,"y":3,"delta_y":8}),
    )
    .unwrap();
    assert!(matches!(
        pixel.command,
        Command::Scroll {
            anchor: ScrollLocation::Pixels(computer::PixelPoint { x: 2.5, y: 3.0 }),
            delta_x: 0,
            delta_y: 8,
            ..
        }
    ));
    let semantic = script_submit(
        "app-a",
        "r",
        "scroll",
        &json!({"window_ref":"w","observation_ref":"o","element_ref":"e","delta_x":-100}),
    )
    .unwrap();
    assert!(
        matches!(semantic.command, Command::Scroll {anchor:ScrollLocation::Element(ref e),delta_x:-100,delta_y:0,..} if e=="e")
    );
    let center = script_submit(
        "app-a",
        "r",
        "scroll",
        &json!({"window_ref":"w","delta_y":100}),
    )
    .unwrap();
    assert_eq!(
        center.command,
        Command::Input(computer::WindowInput::Scroll {
            delta_x: 0,
            delta_y: 100
        })
    );
    for extra in [
        json!({"x":1,"y":2}),
        json!({"element_ref":"e"}),
        json!({"observation_ref":"o"}),
        json!({"observation_ref":"o","x":1}),
        json!({"observation_ref":"o","y":2}),
        json!({"observation_ref":"o","element_ref":"e","x":1,"y":2}),
        json!({"observation_ref":"o","element_ref":"e","x":1}),
        json!({"observation_ref":"o","element_ref":"e","y":1}),
        json!({"observation_ref":null,"x":1,"y":2}),
        json!({"observation_ref":"o","element_ref":""}),
        json!({"observation_ref":"o","x":-1,"y":2}),
        json!({"observation_ref":"o","x":1,"y":null}),
        json!({"delivery":"background"}),
        json!({"point":{"x":1,"y":2}}),
        json!({"delta_y":0}),
        json!({"delta_y":101}),
        json!({"delta_y":0.5}),
    ] {
        let mut args = json!({"window_ref":"w","delta_y":8});
        args.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        assert!(
            script_submit("app-a", "r", "scroll", &args).is_err(),
            "{args}"
        );
    }
}
