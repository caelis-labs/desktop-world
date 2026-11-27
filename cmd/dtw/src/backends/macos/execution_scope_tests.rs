//! Native transport controls; these processes never call desktop APIs.
use super::*;

fn backend(code: &str) -> NativeBackend {
    NativeBackend {
        worker: Arc::new(
            Worker::spawn(std::path::Path::new("python3"), &["-u", "-c", code]).unwrap(),
        ),
        scene: SceneOwner::default(),
        stamp: ProviderStamp {
            id: "execution-focus-control".into(),
            generation: 1,
        },
        cleanup_failure: Mutex::new(None),
        foreground_active: AtomicBool::new(false),
        permissions: Mutex::new(None),
        cursor: Arc::new(CursorSession::default()),
        cursor_operation: AtomicU64::new(0),
    }
}

const CONTROL: &str = r#"import json,sys
active=False
lost=False
begins=0
payloads=0
background=0
ends=0
for line in sys.stdin:
 r=json.loads(line); op=r['operation']
 if op=='begin_execution_focus':
  assert not active
  active=True; begins+=1
  v={'dispatch':'dispatched','evidence':'unverified','scope_active':True}
 elif op=='validate_execution_focus':
  v={'error':'focus_changed','message':'User selected another app'} if lost else {'scope_active':active}
 elif op=='invalidate':
  lost=True; v={}
 elif op=='end_execution':
  ends+=1
  v={'ended':True}
  active=False; lost=False
 elif op=='stats':
  v={'begins':begins,'payloads':payloads,'background':background,'ends':ends}
 elif r.get('need_foreground') and not active:
  assert r.get('_defer_foreground') is True
  v={'dispatch':'not_dispatched','evidence':'unverified','foreground_plan':{'reason':'background_key_window_absent','visual':False}}
 else:
  assert r.get('_execution_foreground')==active
  payloads+=1; background+=int(not active)
  v={'dispatch':'dispatched','evidence':'unverified'}
 print(json.dumps({'request_id':r['request_id'],'result':v}),flush=True)
"#;

fn input(need_foreground: bool) -> Value {
    json!({"operation":"keypress", "target":{"app":{"pid":7,"generation":8},"native_id":9},
        "need_foreground":need_foreground})
}

#[test]
fn background_settlement_keeps_input_usable() {
    let backend = backend(CONTROL);
    let poll = || Ok(Duration::from_secs(3));
    let call = CallContext { poll: &poll };
    for _ in 0..2 {
        assert_eq!(
            backend.action(input(false), &call).dispatch,
            DispatchState::Dispatched
        );
        assert!(!backend.foreground_active.load(Ordering::Acquire));
        assert_eq!(backend.finish_execution(), Ok(()));
        assert_eq!(backend.cleanup_error(), None);
        assert!(backend.is_available());
    }
    backend.shutdown().unwrap();
}

#[test]
fn foreground_starts_only_at_capability_limit_and_survives_until_script_finish() {
    let backend = backend(CONTROL);
    let poll = || Ok(Duration::from_secs(3));
    let call = CallContext { poll: &poll };
    for _ in 0..3 {
        assert_eq!(
            backend.action(input(false), &call).dispatch,
            DispatchState::Dispatched
        );
        assert!(!backend.foreground_active.load(Ordering::Acquire));
    }
    for need in [true, false, true, false] {
        let outcome = backend.action(input(need), &call);
        assert_eq!(outcome.dispatch, DispatchState::Dispatched);
        assert_eq!(
            outcome.delivery.as_ref().unwrap().route,
            Some(InputRoute::Foreground)
        );
    }
    let stats = backend
        .read_rpc(json!({"operation":"stats"}), &call)
        .unwrap();
    assert_eq!(
        stats,
        json!({"begins":1,"payloads":7,"background":3,"ends":0})
    );
    assert_eq!(backend.finish_execution(), Ok(()));
    assert!(!backend.foreground_active.load(Ordering::Acquire));
    let stats = backend
        .read_rpc(json!({"operation":"stats"}), &call)
        .unwrap();
    assert_eq!(stats["ends"], 1);
    assert_eq!(backend.finish_execution(), Ok(()));
    backend.shutdown().unwrap();
}

#[test]
fn a_foreground_scope_preserves_each_actions_actual_delivery_route() {
    let backend = backend(CONTROL);
    let poll = || Ok(Duration::from_secs(3));
    let call = CallContext { poll: &poll };
    assert_eq!(
        backend.action(input(true), &call).dispatch,
        DispatchState::Dispatched
    );
    for route in ["background", "semantic", "foreground"] {
        let mut sends = 0;
        let reply = backend
            .execution_input_rpc(input(false), &call, |request| {
                assert_eq!(request["_execution_foreground"], true);
                sends += 1;
                Ok(json!({"dispatch":"dispatched","evidence":"unverified",
                "delivery":{"route":route,"reason":"actual_transport"}}))
            })
            .unwrap();
        assert_eq!(sends, 1);
        let outcome = action_reply(&reply);
        assert_eq!(outcome.dispatch, DispatchState::Dispatched);
        let delivery = outcome.delivery.unwrap();
        assert_eq!(delivery.route.unwrap().as_str(), route);
        assert_eq!(delivery.reason.as_deref(), Some("actual_transport"));
    }
    assert_eq!(
        backend
            .read_rpc(json!({"operation":"stats"}), &call)
            .unwrap()["begins"],
        1
    );
    assert_eq!(backend.finish_execution(), Ok(()));
    backend.shutdown().unwrap();
}

#[test]
fn external_focus_choice_stops_following_inputs_without_reactivation() {
    let backend = backend(CONTROL);
    let poll = || Ok(Duration::from_secs(3));
    let call = CallContext { poll: &poll };
    assert_eq!(
        backend.action(input(true), &call).dispatch,
        DispatchState::Dispatched
    );
    backend
        .read_rpc(json!({"operation":"invalidate"}), &call)
        .unwrap();
    for _ in 0..2 {
        let refused = backend.action(input(true), &call);
        assert_eq!(refused.dispatch, DispatchState::NotDispatched);
        assert!(
            matches!(refused.error, Some(Error::NativeFailure { ref code, .. }) if code=="focus_changed")
        );
    }
    let stats = backend
        .read_rpc(json!({"operation":"stats"}), &call)
        .unwrap();
    assert_eq!(stats["begins"], 1);
    assert_eq!(stats["payloads"], 1);
    assert_eq!(backend.finish_execution(), Ok(()));
    assert_eq!(
        backend
            .read_rpc(json!({"operation":"stats"}), &call)
            .unwrap()["ends"],
        1
    );
    backend.shutdown().unwrap();
}

#[test]
fn unknown_or_malformed_preflight_never_starts_foreground_or_resends_input() {
    for dispatch in ["unknown", "dispatched", "not_dispatched"] {
        let backend = backend(CONTROL);
        let poll = || Ok(Duration::from_secs(3));
        let call = CallContext { poll: &poll };
        let mut calls = 0;
        let result = backend.execution_input_rpc(input(true), &call, |_| {
            calls += 1;
            Ok(json!({"dispatch":dispatch,"evidence":"unverified",
                "foreground_plan":{"reason":"native_keyboard_semantics","visual":false}}))
        });
        assert!(matches!(result, Err((Error::InvalidObservation, true))));
        assert_eq!(calls, 1);
        assert!(!backend.foreground_active.load(Ordering::Acquire));
        assert_eq!(
            backend
                .read_rpc(json!({"operation":"stats"}), &call)
                .unwrap()["begins"],
            0
        );
        backend.shutdown().unwrap();
    }
}

#[test]
fn ordinary_unknown_action_is_not_an_unsupported_capability() {
    let backend = backend(CONTROL);
    let poll = || Ok(Duration::from_secs(3));
    let call = CallContext { poll: &poll };
    let mut calls = 0;
    let reply = backend.execution_input_rpc(input(true), &call, |_| {
        calls += 1;
        Ok(json!({"dispatch":"unknown","evidence":"unverified","error":"input_failed","message":"Original failure"}))
    }).unwrap();
    assert_eq!(action_reply(&reply).dispatch, DispatchState::Unknown);
    assert_eq!(calls, 1);
    assert!(!backend.foreground_active.load(Ordering::Acquire));
    backend.shutdown().unwrap();
}

#[test]
fn uncertain_focus_preparation_prevents_the_original_input_and_is_cleaned_once() {
    for explicit_focus in [false, true] {
        let backend = backend(
            r#"import json,sys
for line in sys.stdin:
 r=json.loads(line)
 if r['operation']=='end_execution': v={'ended':True}
 else: v={'dispatch':'unknown','evidence':'unverified','error':'focus_failed','message':'Activation did not settle','scope_active':True}
 print(json.dumps({'request_id':r['request_id'],'result':v}),flush=True)
"#,
        );
        let poll = || Ok(Duration::from_secs(3));
        let call = CallContext { poll: &poll };
        let mut calls = 0;
        let mut request = input(true);
        if explicit_focus {
            request["operation"] = json!("focus");
        }
        let reply = backend
        .execution_input_rpc(request, &call, |_| {
            calls += 1;
            Ok(json!({"dispatch":"not_dispatched","evidence":"unverified",
            "foreground_plan":{"reason":if explicit_focus {"explicit_focus"} else {"background_key_window_absent"},"visual":false}}))
        })
        .unwrap();
        let outcome = action_reply(&reply);
        assert_eq!(
            outcome.dispatch,
            if explicit_focus {
                DispatchState::Unknown
            } else {
                DispatchState::NotDispatched
            }
        );
        assert!(
            matches!(outcome.error, Some(Error::NativeFailure { ref code, .. }) if code == "focus_failed")
        );
        if !explicit_focus {
            assert!(reply["message"]
                .as_str()
                .unwrap()
                .contains("no payload was sent"));
            assert_eq!(
                outcome.delivery.unwrap().route,
                Some(InputRoute::Foreground)
            );
        }
        assert_eq!(calls, 1);
        assert!(backend.foreground_active.load(Ordering::Acquire));
        assert_eq!(backend.finish_execution(), Ok(()));
        backend.shutdown().unwrap();
    }
}

#[test]
fn losing_the_preparation_reply_does_not_claim_a_key_payload_was_sent() {
    for explicit_focus in [false, true] {
        let backend = backend("import sys,os\nsys.stdin.readline()\nos._exit(17)\n");
        let poll = || Ok(Duration::from_secs(3));
        let call = CallContext { poll: &poll };
        let mut request = input(true);
        if explicit_focus {
            request["operation"] = json!("focus");
        }
        let mut calls = 0;
        let error = backend.execution_input_rpc(request, &call, |_| {
            calls += 1;
            Ok(json!({"dispatch":"not_dispatched","evidence":"unverified",
                "foreground_plan":{"reason":if explicit_focus {"explicit_focus"} else {"background_key_window_absent"},"visual":false}}))
        }).unwrap_err();
        assert_eq!(error.1, explicit_focus);
        assert_eq!(calls, 1);
        assert!(backend.shutdown().is_err());
    }
}

#[test]
fn native_pointer_receiver_preparation_keeps_the_real_cursor_out_of_foreground_preparation() {
    let code = CONTROL
        .replace("background_key_window_absent", "native_pointer_key_window")
        .replace(
            "active=True; begins+=1",
            "assert r['visual'] is False\n  active=True; begins+=1",
        );
    let backend = backend(&code);
    let poll = || Ok(Duration::from_secs(3));
    let call = CallContext { poll: &poll };
    let mut request = input(true);
    request["operation"] = json!("drag");
    let reply = backend.execution_input_rpc(request, &call, |request| backend.rpc(request, &call));
    assert_eq!(reply.unwrap()["dispatch"], "dispatched");
    let stats = backend
        .read_rpc(json!({"operation":"stats"}), &call)
        .unwrap();
    assert_eq!(stats["begins"], 1);
    assert_eq!(stats["payloads"], 1);
    assert_eq!(stats["background"], 0);
    assert_eq!(backend.finish_execution(), Ok(()));
    assert_eq!(backend.cleanup_error(), None);
    backend.shutdown().unwrap();
}

#[test]
fn native_hover_prepares_focus_without_receiving_pointer_coordinates() {
    let code = CONTROL
        .replace("background_key_window_absent", "native_hover_tracking")
        .replace("'visual':False", "'visual':True")
        .replace(
            "active=True; begins+=1",
            "assert r['visual'] is True and not {'hover_point','screen_x','screen_y','path'}.intersection(r)\n  active=True; begins+=1",
        );
    let backend = backend(&code);
    let poll = || Ok(Duration::from_secs(3));
    let call = CallContext { poll: &poll };
    let mut request = input(true);
    request["operation"] = json!("move");
    request["screen_x"] = json!(-56.0);
    request["screen_y"] = json!(72.0);
    request["path"] = json!([{"x":-56.0,"y":72.0}]);
    let reply = backend.execution_input_rpc(request, &call, |request| backend.rpc(request, &call));
    assert_eq!(reply.unwrap()["dispatch"], "dispatched");
    let stats = backend
        .read_rpc(json!({"operation":"stats"}), &call)
        .unwrap();
    assert_eq!(stats["begins"], 1);
    assert_eq!(stats["payloads"], 1);
    assert_eq!(backend.finish_execution(), Ok(()));
    backend.shutdown().unwrap();
}

#[test]
fn chrome_hover_focus_preparation_resubmits_only_on_the_directed_route() {
    let code = CONTROL
        .replace(
            "background_key_window_absent",
            input_route::CHROME_HOVER_FOCUS,
        )
        .replace(
            "active=True; begins+=1",
            "assert r['visual'] is False and 'screen_x' not in r\n  active=True; begins+=1",
        );
    let backend = backend(&code);
    let poll = || Ok(Duration::from_secs(3));
    let call = CallContext { poll: &poll };
    let mut request = input(true);
    request["operation"] = json!("move");
    request["screen_x"] = json!(56.0);
    let mut attempts = 0;
    let reply = backend.execution_input_rpc(request, &call, |request| {
        assert_eq!(
            request["_require_directed_chrome_hover"] == true,
            attempts == 1
        );
        attempts += 1;
        backend.rpc(request, &call)
    });
    assert_eq!(reply.unwrap()["dispatch"], "dispatched");
    assert_eq!(attempts, 2);
    let stats = backend
        .read_rpc(json!({"operation":"stats"}), &call)
        .unwrap();
    assert_eq!(
        stats,
        json!({"begins":1,"payloads":1,"background":0,"ends":0})
    );
    assert_eq!(backend.finish_execution(), Ok(()));
    backend.shutdown().unwrap();
}

#[test]
fn press_accepts_only_selected_or_focused_confirmation() {
    for postcondition in [
        "element_selected",
        "element_focused",
        "element_value_set",
        "window_focused",
        "other",
    ] {
        let backend = backend(&format!(
            r#"import json,sys
for line in sys.stdin:
 r=json.loads(line)
 assert r['operation']=='press'
 v={{'dispatch':'dispatched','evidence':'confirmed','postcondition':{}}}
 print(json.dumps({{'request_id':r['request_id'],'result':v}}),flush=True)
"#,
            json!(postcondition)
        ));
        let poll = || Ok(Duration::from_secs(3));
        let outcome = backend.action(json!({"operation":"press"}), &CallContext { poll: &poll });
        if ["element_selected", "element_focused"].contains(&postcondition) {
            assert_eq!(outcome.dispatch, DispatchState::Dispatched);
            assert_eq!(
                outcome.evidence,
                EffectEvidence::Confirmed(postcondition.into())
            );
        } else {
            assert_eq!(outcome.dispatch, DispatchState::Unknown);
            assert_eq!(outcome.error, Some(Error::InvalidObservation));
        }
    }
}

#[test]
fn ending_execution_requires_an_explicit_ended_acknowledgement() {
    for response in [
        json!({}),
        json!({"ended":false}),
        json!({"ended":"true"}),
        json!({"ended":true}),
    ] {
        let backend = backend(&format!(
            r#"import json,sys
for line in sys.stdin:
 r=json.loads(line)
 assert r['operation']=='end_execution'
 assert 'target' not in r
 print(json.dumps({{'request_id':r['request_id'],'result':json.loads({})}}),flush=True)
"#,
            json!(response.to_string())
        ));
        backend.foreground_active.store(true, Ordering::Release);
        let result = backend.finish_execution();
        assert_eq!(
            result,
            if response["ended"] == true {
                Ok(())
            } else {
                Err(Error::InvalidObservation)
            }
        );
        assert_eq!(
            backend.foreground_active.load(Ordering::Acquire),
            result.is_err()
        );
    }
}
