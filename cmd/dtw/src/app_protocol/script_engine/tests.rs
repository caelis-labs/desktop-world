use super::*;
use std::thread;

type Record = (String, Value, u32, u32);
fn limits() -> Limits {
    Limits {
        deadline: Instant::now() + Duration::from_secs(3),
        max_calls: 32,
        memory_bytes: 32 * 1024 * 1024,
        stack_bytes: 512 * 1024,
    }
}
fn execute(
    script: &str,
    state: Value,
    limits: Limits,
    mut handle: impl FnMut(&HostCall) -> Result<Value, HostError> + Send + 'static,
) -> (Outcome, Vec<Record>) {
    let (sender, receiver) = mpsc::sync_channel::<HostCall>(1);
    let host = thread::spawn(move || {
        let mut records = Vec::new();
        while let Ok(call) = receiver.recv() {
            records.push((call.op.clone(), call.args.clone(), call.line, call.number));
            let _ = call.reply.send(handle(&call));
        }
        records
    });
    let out = run(
        script,
        state,
        limits,
        Arc::new(AtomicBool::new(false)),
        sender,
    );
    (out, host.join().unwrap())
}
fn pure(script: &str, state: Value) -> Outcome {
    execute(script, state, limits(), |_| {
        panic!("unexpected Core dispatch")
    })
    .0
}
#[test]
fn executes_each_app_operation_through_host_with_exact_refs_and_source_lines() {
    let (out, seen) = execute("const w = await app.windows();\nconst s = await app.observe({window_ref:w.window_ref});\nawait app.capture({window_ref:w.window_ref});\nawait app.press({window_ref:w.window_ref, observation_ref:s.observation_ref, element_ref:s.element_ref});\nawait app.click({window_ref:w.window_ref,observation_ref:'pixels',x:12,y:34});\nstate.count++; return s;",json!({"count":0}), limits(), |call| match call.op.as_str() {
        "windows" => Ok(json!({"window_ref":"w"})),
        "observe" => Ok(json!({"observation_ref":"o","element_ref":"e"})),
        _ => Ok(json!({"receipt":"retained by Core"})),
    });
    let success = out.result.unwrap();
    assert_eq!(success.state, json!({"count":1}));
    assert_eq!(
        success.value,
        json!({"observation_ref":"o","element_ref":"e"})
    );
    assert_eq!(out.calls, 5);
    assert_eq!(
        seen.iter().map(|r| r.2).collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5]
    );
    assert_eq!(
        seen.iter().map(|r| r.3).collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5]
    );
    assert_eq!(
        seen[3].1,
        json!({"window_ref":"w","observation_ref":"o","element_ref":"e"})
    );
}
#[test]
fn only_small_frozen_app_surface_exists_and_native_bridges_are_hidden() {
    let out = pure(
        r#"
      const names=['__host','__done','__log','__settle','__reject_unhandled','__initial_state','__methods','cua','process','require','fetch','std','os','setTimeout'];
      for (const name of names) if (typeof globalThis[name] !== 'undefined') throw new Error(name);
      if (!Object.isFrozen(app) || !Object.isFrozen(app.press) || !Object.isFrozen(console)) throw new Error('mutable API');
      if (Object.getPrototypeOf(app)!==null) throw new Error('app prototype');
      try { app.raw = ()=>1; } catch (_) {}
      if (app.raw || app.pid || app.app_ref || app.launch || app.sleep) throw new Error('privilege');
      return Object.keys(app);
    "#,
        json!({}),
    );
    assert_eq!(
        out.result.unwrap().value,
        json!([
            "windows", "observe", "capture", "press", "click", "typeText", "keypress", "scroll",
            "focus", "setValue"
        ])
    );
}
#[test]
fn constructor_and_dynamic_import_have_no_host_privileges() {
    let out = pure(
        r#"
      const g = app.windows.constructor('return globalThis')();
      if (g.__host || g.__done || g.process || g.require) throw new Error('escape');
      let denied=false; try { await import('std'); } catch (_) { denied=true; }
      return denied;
    "#,
        json!({}),
    );
    assert_eq!(out.result.unwrap().value, json!(true));
}
#[test]
fn extra_app_scope_and_raw_operations_never_dispatch() {
    for source in [
        "return app.observe({window_ref:'w',pid:42});",
        "return app.press({window_ref:'w',observation_ref:'o',element_ref:'e',app_ref:'other'});",
        "return app.windows({});",
        "return app.launch('x');",
        "return app.click({window_ref:'w',observation_ref:'o',x:NaN,y:1});",
    ] {
        let out = pure(source, json!({}));
        assert!(out.result.is_err(), "{source}");
    }
}
#[test]
fn exact_argument_errors_keep_the_parser_message_without_host_dispatch() {
    let out = pure(
        "return await app.observe({window_ref:'w',role:''});",
        json!({}),
    );
    assert_eq!(out.calls, 1);
    let failure = out.result.unwrap_err();
    assert_eq!(failure.code.as_deref(), Some("invalid_args"));
    assert_eq!(failure.call, Some(1));
    assert_eq!(failure.op.as_deref(), Some("observe"));
    assert!(
        failure
            .message
            .contains("command.role must contain 1 to 64 characters"),
        "{}",
        failure.message
    );
}

#[test]
fn ordinary_host_error_can_be_caught_and_later_call_has_its_own_number() {
    let (out, seen)=execute("try { await app.observe({window_ref:'bad'}); } catch(e) { state.failure=e.code; }\nreturn await app.windows();",json!({}),limits(),|call| if call.number==1 {Err(HostError::new("no such window","unknown_window"))} else {Ok(json!(["w"]))});
    let success = out.result.unwrap();
    assert_eq!(success.state, json!({"failure":"unknown_window"}));
    assert_eq!(success.value, json!(["w"]));
    assert_eq!(seen.len(), 2);
}
#[test]
fn omitted_call_quota_allows_more_than_thirty_two_operations() {
    let mut budget = limits();
    budget.max_calls = super::super::scripts::UNLIMITED_CALLS;
    let (out, seen) = execute(
        "for(let i=0;i<80;i++) await app.windows(); return 80;",
        json!({}),
        budget,
        |_| Ok(json!({"windows":[]})),
    );
    assert_eq!(out.result.unwrap().value, 80);
    assert_eq!(seen.len(), 80);
}
#[test]
fn caller_deadline_can_exceed_the_old_five_second_limit() {
    let mut budget = limits();
    budget.deadline = Instant::now() + Duration::from_secs(10);
    let (out, seen) = execute("return await app.windows();", json!({}), budget, |_| {
        thread::sleep(Duration::from_millis(5100));
        Ok(json!({"finished":true}))
    });
    assert_eq!(out.result.unwrap().value, json!({"finished":true}));
    assert_eq!(seen.len(), 1);
}
#[test]
fn receipt_resource_exhaustion_cannot_be_caught_to_report_success() {
    let (out, seen) = execute(
        "try { await app.windows(); } catch(e) {} state.saved=true; return 'ok';",
        json!({}),
        limits(),
        |_| Err(HostError::new("receipt budget exhausted", "receipt_limit")),
    );
    let failure = out.result.unwrap_err();
    assert_eq!(failure.kind, "resource_limit");
    assert_eq!(failure.code.as_deref(), Some("receipt_limit"));
    assert_eq!(seen.len(), 1);
}
#[test]
fn uncaught_host_error_retains_line_operation_code_and_number() {
    let (out,seen)=execute("state.mutated=true;\nawait app.press({window_ref:'w',observation_ref:'o',element_ref:'e'});",json!({}),limits(), |_|Err(HostError::new("reference consumed","stale_observation")));
    let failure = out.result.unwrap_err();
    assert_eq!(failure.kind, "exception");
    assert_eq!(failure.line, Some(2));
    assert_eq!(failure.op.as_deref(), Some("press"));
    assert_eq!(failure.code.as_deref(), Some("stale_observation"));
    assert_eq!(failure.call, Some(1));
    assert_eq!(seen.len(), 1);
}
#[test]
fn call_limit_is_sticky_even_if_script_catches_it() {
    let mut budget = limits();
    budget.max_calls = 1;
    let (out,seen)=execute("await app.windows();\ntry { await app.windows(); } catch (_) {}\nstate.fake=true; return 'success';",json!({}),budget, |_|Ok(json!([])));
    let failure = out.result.unwrap_err();
    assert_eq!(failure.code.as_deref(), Some("call_limit"));
    assert_eq!(failure.line, Some(2));
    assert_eq!(failure.call, Some(2));
    assert_eq!(out.calls, 1);
    assert_eq!(seen.len(), 1);
}
#[test]
fn full_host_queue_is_deadline_bounded_without_dispatch() {
    let (sender, receiver) = mpsc::sync_channel(0);
    let mut budget = limits();
    budget.deadline = Instant::now() + Duration::from_millis(100);
    let out = run(
        "try { app.windows(); } catch(_) {} return true;",
        json!({}),
        budget,
        Arc::new(AtomicBool::new(false)),
        sender,
    );
    assert_eq!(out.result.unwrap_err().kind, "timeout");
    assert!(receiver.try_recv().is_err());
    assert_eq!(out.calls, 1);
}
#[test]
fn cancellation_while_waiting_for_core_reply_is_prompt_and_late_reply_is_not_replayed() {
    let (sender, receiver) = mpsc::sync_channel(1);
    let cancel = Arc::new(AtomicBool::new(false));
    let token = cancel.clone();
    let engine = thread::spawn(move || {
        run(
            "try { await app.windows(); } catch (_) {} return true;",
            json!({}),
            limits(),
            token,
            sender,
        )
    });
    let call = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
    let started = Instant::now();
    cancel.store(true, Ordering::SeqCst);
    let out = engine.join().unwrap();
    assert!(started.elapsed() < Duration::from_secs(1));
    let failure = out.result.unwrap_err();
    assert_eq!(failure.kind, "cancelled");
    assert_eq!(failure.op.as_deref(), Some("windows"));
    assert_eq!(failure.call, Some(1));
    assert!(call.reply.send(Ok(json!({"late":true}))).is_err());
    assert!(receiver.try_recv().is_err());
}
#[test]
fn infinite_loop_and_microtask_loop_obey_deadline_even_with_catch() {
    for source in [
        "try { for(;;){} }catch(_){} return true;",
        "for(;;) await Promise.resolve();",
    ] {
        let mut budget = limits();
        budget.deadline = Instant::now() + Duration::from_millis(100);
        let out = execute(source, json!({}), budget, |_| panic!("unexpected host")).0;
        assert_eq!(out.result.unwrap_err().kind, "timeout");
    }
}
#[test]
fn syntax_source_line_and_unsettled_promise_are_distinct() {
    let failure = pure("const ok=1;\nconst = ;", json!({}))
        .result
        .unwrap_err();
    assert_eq!(failure.kind, "syntax");
    assert_eq!(failure.line, Some(2));
    assert_eq!(
        pure("await new Promise(()=>{});", json!({}))
            .result
            .unwrap_err()
            .kind,
        "stalled"
    );
}
#[test]
fn stack_and_heap_failures_are_bounded_and_classified() {
    let stack = pure(
        "function recurse(){return recurse()+1;} return recurse();",
        json!({}),
    )
    .result
    .unwrap_err();
    assert_eq!(stack.kind, "exception");
    assert!(stack.message.contains("stack"), "{stack:?}");
    let heap = pure(
        "const rows=[]; for(;;) rows.push(new Uint8Array(1024*1024));",
        json!({}),
    )
    .result
    .unwrap_err();
    assert_eq!(heap.kind, "memory", "{heap:?}");
}
#[test]
fn all_json_root_types_and_nested_mutation_commit_only_on_success() {
    for state in [
        json!(null),
        json!(false),
        json!(42),
        json!(2.5),
        json!("hello"),
        json!([1,null,{"x":true}]),
        json!({"nested":[1,2]}),
    ] {
        let out = pure("return state;", state.clone()).result.unwrap();
        assert_eq!(out.state, state);
        assert_eq!(out.value, state);
    }
    let out = pure("state[0].x=2; return undefined;", json!([{"x":1}]))
        .result
        .unwrap();
    assert_eq!(out.state, json!([{"x":2}]));
    assert_eq!(out.value, Value::Null);
    assert_eq!(pure("", json!({})).result.unwrap().value, Value::Null);
    assert!(pure("state.x=2; throw new Error('later');", json!({"x":1}))
        .result
        .is_err());
}
#[test]
fn state_root_replacement_fails_and_catching_it_preserves_original_root() {
    assert_eq!(
        pure("state=undefined;", json!({"x":1}))
            .result
            .unwrap_err()
            .code
            .as_deref(),
        Some("invalid_state")
    );
    let out = pure(
        "try { state={fake:true}; }catch(_){} return state;",
        json!([1]),
    )
    .result
    .unwrap();
    assert_eq!(out.state, json!([1]));
    assert_eq!(out.value, json!([1]));
}
#[test]
fn host_argument_undefined_fields_are_omitted_without_mutating_the_input() {
    let (out, seen) = execute(
        r#"
const args={window_ref:'w',role:undefined,text:undefined,unused:undefined,limit:12};
await app.observe(args);
await app.observe({window_ref:'w',limit:12});
const plain=Object.create(null);
plain.window_ref='w'; plain.role=undefined; plain.text=undefined; plain.limit=12;
await app.observe(plain);
return {roleStillOwned:Object.hasOwn(args,'role'),textStillOwned:Object.hasOwn(plain,'text')};
"#,
        json!({}),
        limits(),
        |_| Ok(json!({})),
    );
    assert_eq!(
        out.result.unwrap().value,
        json!({"roleStillOwned":true,"textStillOwned":true})
    );
    assert_eq!(seen.len(), 3);
    for (_, args, _, _) in seen {
        assert_eq!(args, json!({"window_ref":"w","limit":12}));
    }
}

#[test]
fn host_argument_omission_preserves_schema_and_strict_json_guards() {
    for args in [
        "{window_ref:undefined,role:undefined}",
        "{window_ref:'w',role:''}",
        "{window_ref:'w',text:null}",
        "{window_ref:'w',unexpected:true}",
        "{cursor:'c',window_ref:'w',role:undefined}",
    ] {
        let out = pure(&format!("return await app.observe({args});"), json!({}));
        assert_eq!(
            out.result.unwrap_err().code.as_deref(),
            Some("invalid_args"),
            "{args}"
        );
        assert_eq!(out.calls, 1);
    }
    for args in [
        "undefined",
        "[undefined]",
        "[,1]",
        "{window_ref:'w',role:()=>1}",
        "{window_ref:'w',role:Symbol('role')}",
        "{window_ref:'w',role:1n}",
        "{window_ref:'w',role:NaN}",
        "{window_ref:'w',role:new Date()}",
        "(()=>{const a={window_ref:'w'};a.self=a;return a;})()",
        "(()=>{const a={window_ref:'w'};a[Symbol('extra')]=undefined;return a;})()",
        "Object.defineProperty({window_ref:'w'},'hidden',{value:undefined})",
        "Object.assign([],{extra:undefined})",
    ] {
        let out = pure(&format!("return await app.observe({args});"), json!({}));
        assert_eq!(
            out.result.unwrap_err().code.as_deref(),
            Some("invalid_json"),
            "{args}"
        );
        assert_eq!(out.calls, 0);
    }
    let out = pure(
        "return await app.keypress({window_ref:'w',keys:['META',undefined]});",
        json!({}),
    );
    assert_eq!(
        out.result.unwrap_err().code.as_deref(),
        Some("invalid_json")
    );
    assert_eq!(out.calls, 0);
    for definition in [
        "Object.defineProperty(args,'role',{get(){touched++;return undefined},enumerable:true});",
        "args.toJSON=()=>{touched++;return {window_ref:'w'}};",
    ] {
        let out = pure(&format!("let touched=0;const args={{window_ref:'w'}};{definition}try{{await app.observe(args)}}catch(e){{return {{code:e.code,touched}}}}"),json!({}));
        assert_eq!(
            out.result.unwrap().value,
            json!({"code":"invalid_json","touched":0})
        );
        assert_eq!(out.calls, 0);
    }
}

#[test]
fn omitted_host_fields_still_count_toward_json_traversal_limits() {
    let out = pure("const args={window_ref:'w'};for(let i=0;i<32768;i++)args['unused'+i]=undefined;return await app.observe(args);", json!({}));
    let failure = out.result.unwrap_err();
    assert_eq!(failure.code.as_deref(), Some("invalid_json"));
    assert!(
        failure.message.contains("JSON traversal limit"),
        "{failure:?}"
    );
    assert_eq!(out.calls, 0);
}

#[test]
fn invalid_json_state_or_result_is_never_silently_coerced() {
    for mutation in ["state.x=undefined;", "state.x=()=>1;", "state.x=Symbol('x');", "state.x=NaN;", "state.x=Infinity;", "state.x=1n;", "state.x=state;", "state.x=new Date();", "state.x=[,1];", "state[Symbol('x')]=1;", "Object.defineProperty(state,'x',{get(){throw new Error('accessor executed')},enumerable:true});"] {
        let failure=pure(mutation,json!({})).result.unwrap_err(); assert_eq!(failure.code.as_deref(),Some("invalid_json"),"{mutation}: {failure:?}");
    }
    for result in [
        "()=>1",
        "{x:undefined}",
        "Symbol('x')",
        "Infinity",
        "1n",
        "new Date()",
        "[,1]",
    ] {
        assert!(
            pure(&format!("return {result};"), json!({}))
                .result
                .is_err(),
            "{result}"
        );
    }
}
#[test]
fn serializer_cannot_be_replaced_or_trigger_host_calls_during_settlement() {
    let out=pure(r#"JSON.stringify=()=>'{"ok":true}'; Object.setPrototypeOf=()=>{}; Object.prototype.toJSON=()=>({fake:true}); Array.prototype.toJSON=()=>({fake:true}); state.real=[1,2]; return {real:true};"#,json!({})).result.unwrap();
    assert_eq!(out.state, json!({"real":[1,2]}));
    assert_eq!(out.value, json!({"real":true}));
    let out = pure(
        "return new Proxy({}, {ownKeys(){app.windows(); return []}});",
        json!({}),
    );
    assert_eq!(out.calls, 0);
    assert_eq!(
        out.result.unwrap_err().code.as_deref(),
        Some("invalid_phase")
    );
}
#[test]
fn result_state_and_host_arguments_have_utf8_byte_limits() {
    for source in [
        "return 'x'.repeat(65536);",
        "state.big='界'.repeat(22000);",
        "return app.observe({window_ref:'x'.repeat(65536)});",
    ] {
        let out = pure(source, json!({}));
        assert_eq!(
            out.result.unwrap_err().code.as_deref(),
            Some("output_limit")
        );
        assert_eq!(out.calls, 0);
    }
    let (out, seen) = execute("return app.windows();", json!({}), limits(), |_| {
        Ok(json!({"large":"x".repeat(HOST_REPLY_LIMIT)}))
    });
    assert_eq!(
        out.result.unwrap_err().code.as_deref(),
        Some("host_result_limit")
    );
    assert_eq!(seen.len(), 1);
}
#[test]
fn initial_state_budget_and_unrepresentable_integers_fail_before_js() {
    assert_eq!(
        pure("return 1;", json!({"large":"x".repeat(JSON_LIMIT)}))
            .result
            .unwrap_err()
            .code
            .as_deref(),
        Some("state_limit")
    );
    assert_eq!(
        pure("return state;", json!(9007199254740993u64))
            .result
            .unwrap_err()
            .code
            .as_deref(),
        Some("invalid_state")
    );
    assert_eq!(
        pure("return state;", json!(9007199254740992u64))
            .result
            .unwrap()
            .state,
        json!(9007199254740992u64)
    );
}
#[test]
fn console_and_exception_evidence_are_bounded_and_mark_truncation() {
    let out = pure(
        "for(let i=0;i<300;i++)console.warn('界'.repeat(3000)); return 1;",
        json!({}),
    );
    assert!(out.result.is_ok());
    assert!(out.console_truncated);
    assert!(out.console.len() <= CONSOLE_MAX_LINES);
    assert!(out.console.iter().map(String::len).sum::<usize>() <= CONSOLE_MAX_BYTES);
    let failure = pure("throw new Error('界'.repeat(20000));", json!({}))
        .result
        .unwrap_err();
    assert!(failure.message.len() <= 8192);
    assert!(failure.message.contains("[truncated]"));
}

#[test]
fn cancelled_full_send_does_not_wait_for_receiver_or_dispatch() {
    let (sender, receiver) = mpsc::sync_channel(0);
    let cancel = Arc::new(AtomicBool::new(false));
    let token = cancel.clone();
    let used = Arc::new(AtomicU32::new(0));
    let count = used.clone();
    let terminal = Arc::new(Mutex::new(None));
    let stopped = terminal.clone();
    let deadline = Instant::now() + Duration::from_secs(3);
    let engine = thread::spawn(move || {
        host_call(
            "windows", "{}", 7, deadline, 32, &token, &count, &sender, &stopped,
        )
    });
    while used.load(Ordering::SeqCst) == 0 {
        assert!(Instant::now() < deadline, "host call never started");
        thread::yield_now();
    }
    let started = Instant::now();
    cancel.store(true, Ordering::SeqCst);
    let reply: Value = serde_json::from_str(&engine.join().unwrap()).unwrap();
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(reply["code"], "cancelled");
    assert!(receiver.try_recv().is_err());
    assert_eq!(terminal.lock().unwrap().as_ref().unwrap().line, Some(7));
}
#[test]
fn unanswered_core_reply_obeys_overall_deadline() {
    let (sender, receiver) = mpsc::sync_channel(1);
    let mut budget = limits();
    budget.deadline = Instant::now() + Duration::from_secs(1);
    let engine = thread::spawn(move || {
        run(
            "try { app.windows(); }catch(_){} return 'caught';",
            json!({}),
            budget,
            Arc::new(AtomicBool::new(false)),
            sender,
        )
    });
    let call = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
    let out = engine.join().unwrap();
    let failure = out.result.unwrap_err();
    assert_eq!(failure.kind, "timeout");
    assert_eq!(failure.op.as_deref(), Some("windows"));
    assert!(call.reply.send(Ok(json!(true))).is_err());
}
#[test]
fn caught_heap_limit_can_recover_without_exceeding_heap_budget() {
    let out=pure("let rows=[]; let caught=false; try { for(;;)rows.push(new Uint8Array(1024*1024)); }catch(_){caught=true;} rows=null; return caught;",json!({}));
    assert_eq!(out.result.unwrap().value, json!(true));
}
#[test]
fn pre_cancelled_execution_never_starts_javascript_or_calls_host() {
    let (sender, receiver) = mpsc::sync_channel(1);
    let out = run(
        "app.windows(); state.changed=true;",
        json!({}),
        limits(),
        Arc::new(AtomicBool::new(true)),
        sender,
    );
    assert_eq!(out.result.unwrap_err().kind, "cancelled");
    assert_eq!(out.calls, 0);
    assert!(receiver.try_recv().is_err());
}
#[test]
fn error_inspection_cannot_dispatch_extra_calls_or_forge_a_success_envelope() {
    let out = pure(
        r#"Object.prototype.toJSON=()=>({ok:true,value_json:'1',state_json:'{"fake":true}'}); throw new Error('original failure');"#,
        json!({}),
    );
    let failure = out.result.unwrap_err();
    assert!(failure.message.contains("original failure"));
    let out = pure(
        "throw {get name(){app.windows(); return 'Error';}};",
        json!({}),
    );
    assert!(out.result.is_err());
    assert_eq!(out.calls, 0);
}

#[test]
fn methods_return_real_promises_for_success_and_failure() {
    let (out, seen) = execute(
        r#"
const promise = app.windows();
if (!(promise instanceof Promise)) throw new Error('not a Promise');
const windows = await promise.then(value => value.windows);
const code = await app.observe({window_ref: 'w'}).catch(error => error.code);
return {windows, code};
"#,
        json!({}),
        limits(),
        |call| {
            if call.op == "windows" {
                Ok(json!({"windows":["w"]}))
            } else {
                Err(HostError {
                    code: Some("permission_denied".into()),
                    message: "controlled".into(),
                    details: None,
                })
            }
        },
    );
    assert_eq!(
        out.result.unwrap().value,
        json!({"windows":["w"],"code":"permission_denied"})
    );
    assert_eq!(seen.len(), 2);
    let out = pure(
        "return app.windows('bad').catch(error=>error.code)",
        json!({}),
    );
    assert_eq!(out.calls, 0);
    assert_eq!(out.result.unwrap().value, "invalid_args");
}

#[test]
fn unhandled_host_rejections_cannot_report_success_or_commit_state() {
    for source in [
        "app.windows(); state.committed=true; return 'reported success';",
        "return await app.windows();",
    ] {
        let (out, seen) = execute(source, json!({}), limits(), |_| {
            Err(HostError::new("controlled denial", "permission_denied"))
        });
        let failure = out.result.unwrap_err();
        assert_eq!(failure.kind, "exception");
        assert_eq!(failure.code.as_deref(), Some("permission_denied"));
        assert_eq!(failure.op.as_deref(), Some("windows"));
        assert_eq!(failure.call, Some(1));
        assert_eq!(failure.line, Some(1));
        assert_eq!(seen.len(), 1);
    }
    let (out, seen) = execute(
        "const code=await app.windows().catch(e=>e.code); state.recovered=true; return code;",
        json!({}),
        limits(),
        |_| Err(HostError::new("controlled denial", "permission_denied")),
    );
    let success = out.result.unwrap();
    assert_eq!(success.value, "permission_denied");
    assert_eq!(success.state, json!({"recovered":true}));
    assert_eq!(seen.len(), 1);
}

#[test]
fn late_catch_handlers_and_microtasks_can_handle_an_initial_rejection() {
    for source in [
        "const p=app.windows(); await Promise.resolve(); await p.catch(()=>null); return 'handled';",
        "const p=app.windows(); Promise.resolve().then(()=>Promise.resolve().then(()=>p.catch(()=>null))); return 'handled';",
        "const p=app.windows().then(v=>v); await Promise.resolve(); await p.catch(()=>null); return 'handled';",
    ] {
        let (out, seen) = execute(source, json!({}), limits(), |_| {
            Err(HostError::new("controlled denial", "permission_denied"))
        });
        assert_eq!(out.result.unwrap().value, "handled", "{source}");
        assert_eq!(seen.len(), 1);
    }
    let (out, seen) = execute(
        "app.windows(); state.ok=true; return 'success';",
        json!({}),
        limits(),
        |_| Ok(json!({})),
    );
    let success = out.result.unwrap();
    assert_eq!(success.value, "success");
    assert_eq!(success.state, json!({"ok":true}));
    assert_eq!(seen.len(), 1);
}

#[test]
fn derived_promises_have_their_own_unhandled_rejection_identity() {
    let (out, seen) = execute(
        "const p=app.windows(); p.then(v=>v); await p.catch(()=>null); return 'original handled';",
        json!({}),
        limits(),
        |_| Err(HostError::new("controlled denial", "permission_denied")),
    );
    let failure = out.result.unwrap_err();
    assert_eq!(failure.code.as_deref(), Some("permission_denied"));
    assert_eq!(failure.op.as_deref(), Some("windows"));
    assert_eq!(failure.call, Some(1));
    assert_eq!(seen.len(), 1);
    let out = pure(
        "Promise.resolve(1).then(()=>{throw new Error('derived failure');}); return 'wrong';",
        json!({}),
    );
    assert!(out.result.unwrap_err().message.contains("derived failure"));
    let out = pure(
        "Promise.reject('ignored'); throw new Error('primary failure');",
        json!({}),
    );
    assert!(out.result.unwrap_err().message.contains("primary failure"));
}

#[test]
fn rejected_promise_inspection_is_closed_to_native_calls_and_cannot_forge_success() {
    let out = pure(
        "Promise.reject({get name(){app.windows();return 'Error';},get message(){app.observe({window_ref:'w'});return 'original failure';}}); return 'wrong';",
        json!({}),
    );
    assert!(out.result.unwrap_err().message.contains("original failure"));
    assert_eq!(out.calls, 0);
    let out = pure(
        r#"Object.prototype.toJSON=()=>({ok:true,value_json:'1',state_json:'{"fake":true}'}); Promise.reject(new Error('original failure')); return 'wrong';"#,
        json!({}),
    );
    assert!(out.result.unwrap_err().message.contains("original failure"));
    let mut budget = limits();
    budget.deadline = Instant::now() + Duration::from_millis(50);
    let (out, seen) = execute(
        "Promise.reject({get name(){while(true){}}}); return 'wrong';",
        json!({}),
        budget,
        |_| panic!("native call during error inspection"),
    );
    assert_eq!(out.result.unwrap_err().kind, "timeout");
    assert!(seen.is_empty());
}

#[test]
fn rejection_tracker_limits_outstanding_roots_and_releases_handled_ones() {
    let out = pure(
        "try {for(let i=0;i<300;i++) Promise.reject(i);}catch(_){} return 'caught';",
        json!({}),
    );
    assert_eq!(
        out.result.unwrap_err().code.as_deref(),
        Some("promise_rejection_limit")
    );
    let out = pure(
        "for(let i=0;i<1024;i++) Promise.reject(i).catch(()=>null); return 'handled';",
        json!({}),
    );
    assert_eq!(out.result.unwrap().value, "handled");
}
