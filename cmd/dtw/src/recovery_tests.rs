//! Opt-in integration tests against a built supervisor and the real pinned worker.
//! Only JSON scripts/invalid browser bindings are used; no GUI input is sent.
use super::*;

fn server(manifest: Option<Value>) -> Arc<Server> {
    let worker = env::var("DTW_TEST_WORKER").expect("DTW_TEST_WORKER");
    let lifetime = lifecycle::Lifetime::new(&worker).unwrap();
    let manifest = manifest.map(|value| {
        let path = lifetime.root.join("approved.yaml");
        fs::write(&path, value.to_string()).unwrap();
        path.to_string_lossy().into_owned()
    });
    let driver = new_driver_at(
        worker,
        &lifetime,
        &manifest,
        env::var("DTW_TEST_BINARY").expect("DTW_TEST_BINARY"),
    )
    .unwrap();
    Arc::new(Server {
        driver,
        lifetime,
        capability_manifest: manifest.clone(),
        discovery: authorization::discovery(manifest.as_deref()).unwrap(),
        closing: std::sync::atomic::AtomicBool::new(false),
        session: Mutex::new(None),
        app_view: app_view::Runtime::default(),
        jobs: Mutex::new(HashMap::new()),
        pending_requests: Mutex::new(HashMap::new()),
        stdout: AsyncMutex::new(io::stdout()),
    })
}

async fn call(server: &Arc<Server>, id: &str, args: Value) -> Value {
    server
        .pending_requests
        .lock()
        .unwrap()
        .insert(id.into(), None);
    let reply = server.exec(args, id).await;
    server.pending_requests.lock().unwrap().remove(id);
    reply
}

fn text(reply: &Value) -> &str {
    reply["content"][0]["text"].as_str().unwrap()
}

#[tokio::test]
#[ignore = "requires DTW_TEST_BINARY and DTW_TEST_WORKER; run with --test-threads=1"]
async fn idle_and_cancel_recovery_preserve_runtime_and_native_expiry() {
    let runtime = server(None);
    let first = call(
        &runtime,
        "one",
        json!({"script":"state.n=7; return state.n;"}),
    )
    .await;
    assert!(text(&first).contains("return value: 7"), "{first}");
    runtime.session.lock().unwrap().as_mut().unwrap().last_used -= Duration::from_secs(1501);
    let reset = call(
        &runtime,
        "two",
        json!({"script":"state.n=999; return state.n;"}),
    )
    .await;
    assert!(text(&reset).contains("runtime_context_reset"), "{reset}");
    let fresh = call(&runtime, "three", json!({"script":"return state.n || 0;"})).await;
    assert!(text(&fresh).contains("return value: 0"), "{fresh}");
    runtime
        .pending_requests
        .lock()
        .unwrap()
        .insert("cancelled".into(), None);
    runtime.cancel_request("cancelled");
    let cancelled = runtime
        .exec(json!({"script":"state.n=999;"}), "cancelled")
        .await;
    assert!(
        text(&cancelled).contains("cancelled_before_action"),
        "{cancelled}"
    );
    assert!(!runtime.lifetime.stopped());
    let after = call(&runtime, "four", json!({"script":"return state.n || 0;"})).await;
    assert!(text(&after).contains("return value: 0"), "{after}");
    // Cancelling an outstanding discovery request must not stop a different job.
    let running = call(&runtime, "long", json!({"script":"const until=Date.now()+500; while(Date.now()<until){}; return 42;", "wait_ms":0})).await;
    assert!(text(&running).contains("status=running"), "{running}");
    runtime
        .pending_requests
        .lock()
        .unwrap()
        .insert("discovery".into(), None);
    runtime.cancel_request("discovery");
    assert!(!runtime.lifetime.stopped());
    for _ in 0..200 {
        if runtime
            .jobs
            .lock()
            .unwrap()
            .values()
            .all(|j| j.phase != "running")
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(runtime
        .jobs
        .lock()
        .unwrap()
        .values()
        .all(|j| j.phase != "running"));
    let after = call(&runtime, "final", json!({"script":"return 42;"})).await;
    assert!(text(&after).contains("return value: 42"), "{after}");
    runtime.lifetime.stop().unwrap();
    drop(runtime);

    let bounded = server(Some(
        json!({"version":2,"mode":"bounded","expires_after":"3s","idle_timeout":"3s",
        "allow":{"tools":["start_session","end_session","get_browser_state"]},
        "resources":{"browser":{"origins":["http://127.0.0.1:1"]}}}),
    ));
    let args =
        json!({"operation":"get_browser_state","arguments":{"pid":2147483647,"window_id":1}});
    let before = call(&bounded, "before", args.clone()).await;
    assert!(!text(&before).contains("expired"), "{before}");
    tokio::time::sleep(Duration::from_millis(3200)).await;
    bounded.session.lock().unwrap().as_mut().unwrap().last_used -= Duration::from_secs(1501);
    let reset = call(&bounded, "reset", args.clone()).await;
    assert!(text(&reset).contains("runtime_context_reset"), "{reset}");
    let expired = call(&bounded, "expired", args).await;
    assert!(text(&expired).contains("expired"), "{expired}");
    bounded.lifetime.stop().unwrap();
}
