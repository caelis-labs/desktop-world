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
        input_policy: InputPolicy::Auto,
        focus_pending: AtomicBool::new(false),
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
restores=0
for line in sys.stdin:
 r=json.loads(line); op=r['operation']
 if op=='begin_execution_focus':
  assert not active
  active=True; begins+=1
  v={'dispatch':'dispatched','evidence':'unverified','scope_active':True,'borrowed':True}
 elif op=='validate_execution_focus':
  v={'error':'focus_changed','message':'User selected another app'} if lost else {'scope_active':active,'borrowed':True}
 elif op=='invalidate':
  lost=True; v={}
 elif op=='release_focus':
  restores+=int(active and not lost)
  v={'focus':'changed_externally' if lost else 'restored' if active else 'unchanged'}
  active=False; lost=False
 elif op=='stats':
  v={'begins':begins,'payloads':payloads,'background':background,'restores':restores}
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
fn borrow_starts_only_at_capability_limit_and_survives_until_script_finish() {
    let backend = backend(CONTROL);
    let poll = || Ok(Duration::from_secs(3));
    let call = CallContext { poll: &poll };
    for _ in 0..3 {
        assert_eq!(
            backend.action(input(false), &call).dispatch,
            DispatchState::Dispatched
        );
        assert!(!backend.focus_pending.load(Ordering::Acquire));
    }
    for need in [true, false, true, false] {
        let outcome = backend.action(input(need), &call);
        assert_eq!(outcome.dispatch, DispatchState::Dispatched);
        assert_eq!(
            outcome.delivery.as_ref().unwrap().focus_return,
            Some(FocusReturn::Pending)
        );
    }
    let stats = backend
        .read_rpc(json!({"operation":"stats"}), &call)
        .unwrap();
    assert_eq!(
        stats,
        json!({"begins":1,"payloads":7,"background":3,"restores":0})
    );
    assert_eq!(backend.finish_execution(), Ok(Some(FocusReturn::Restored)));
    assert!(!backend.focus_pending.load(Ordering::Acquire));
    let stats = backend
        .read_rpc(json!({"operation":"stats"}), &call)
        .unwrap();
    assert_eq!(stats["restores"], 1);
    assert_eq!(backend.finish_execution(), Ok(None));
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
    assert_eq!(
        backend.finish_execution(),
        Ok(Some(FocusReturn::ChangedExternally))
    );
    assert_eq!(
        backend
            .read_rpc(json!({"operation":"stats"}), &call)
            .unwrap()["restores"],
        0
    );
    backend.shutdown().unwrap();
}

#[test]
fn unknown_or_malformed_preflight_never_starts_a_borrow_or_resends_input() {
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
        assert!(!backend.focus_pending.load(Ordering::Acquire));
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
    assert!(!backend.focus_pending.load(Ordering::Acquire));
    backend.shutdown().unwrap();
}

#[test]
fn uncertain_focus_preparation_prevents_the_original_input_and_is_cleaned_once() {
    let backend = backend(
        r#"import json,sys
for line in sys.stdin:
 r=json.loads(line)
 if r['operation']=='release_focus': v={'focus':'restored'}
 else: v={'dispatch':'unknown','evidence':'unverified','error':'focus_failed','message':'Activation did not settle'}
 print(json.dumps({'request_id':r['request_id'],'result':v}),flush=True)
"#,
    );
    let poll = || Ok(Duration::from_secs(3));
    let call = CallContext { poll: &poll };
    let mut calls = 0;
    let reply = backend
        .execution_input_rpc(input(true), &call, |_| {
            calls += 1;
            Ok(json!({"dispatch":"not_dispatched","evidence":"unverified",
            "foreground_plan":{"reason":"background_key_window_absent","visual":false}}))
        })
        .unwrap();
    assert_eq!(action_reply(&reply).dispatch, DispatchState::Unknown);
    assert_eq!(calls, 1);
    assert!(backend.focus_pending.load(Ordering::Acquire));
    assert_eq!(backend.finish_execution(), Ok(Some(FocusReturn::Restored)));
    backend.shutdown().unwrap();
}
