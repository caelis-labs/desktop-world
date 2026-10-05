use caelis_desktop_world::{Fact, HostSession, Plan, SessionOptions};
use serde_json::json;
#[tokio::test(flavor = "current_thread")]
async fn owner_authority_and_original_receipts() {
    let helper = std::env::var("DTW_SDK_TEST_HELPER").expect("run scripts/check-sdks.py");
    let mut options = SessionOptions::new(helper);
    options.input_mode = "shared".into();
    options.write_apps.push("Later".into());
    let host = HostSession::start(options).await.unwrap();
    let dw = &host.desktop;
    let ob = dw
        .observe(json!({"projection":"outline","fields":["name","role","app","window","states"]}))
        .await
        .unwrap();
    assert!(ob.coverage.complete);
    let named = |name: &str| {
        ob.objects
            .iter()
            .find(|o| {
                o.name
                    .as_ref()
                    .and_then(Fact::known)
                    .is_some_and(|v| v == name)
            })
            .unwrap()
            .r#ref
            .clone()
    };
    assert_eq!(host.grants().await.unwrap().grants[0].state, "pending");
    assert!(dw
        .call("grant", json!({"application":named("Fixture")}), None)
        .await
        .is_err());
    let mut p = Plan::new();
    p.set(&named("内容"), "SDK 中文\n🙂");
    assert!(dw.act(&p, Some("denied")).await.is_err());
    host.grant(&named("Fixture")).await.unwrap();
    host.grant(&named("Other")).await.unwrap();
    let receipt = dw.act(&p, Some("original")).await.unwrap();
    assert_eq!(
        dw.act(&p, Some("original")).await.unwrap().run_id,
        receipt.run_id
    );
    let mut different = Plan::new();
    different.set(&named("内容"), "different");
    assert_eq!(
        dw.act(&different, Some("original")).await.unwrap_err().code,
        "request_conflict"
    );
    assert_eq!(
        dw.read(&named("内容")).await.unwrap()["text"]["value"],
        "SDK 中文\n🙂"
    );
    assert_eq!(
        dw.reconcile("original").await.unwrap().result.unwrap()["run_id"],
        receipt.run_id
    );
    let mut input = Plan::new();
    input.focus(&named("内容"));
    let focused = input.bind_focus("input", &named("Desktop World Fixture"));
    input.press(focused, "A", &["primary"]);
    dw.act(&input, None).await.unwrap();
    host.revoke(&named("Fixture")).await.unwrap();
    assert!(dw.act(&p, Some("revoked")).await.is_err());
    let mut other = Plan::new();
    other.set(&named("Other Field"), "");
    dw.act(&other, None).await.unwrap();
    assert_eq!(
        dw.read(&named("Other Field")).await.unwrap()["text"]["value"],
        ""
    );
    host.grant(&named("Fixture")).await.unwrap();
    let mut slow = Plan::new();
    slow.invoke(&named("提交"));
    assert!(tokio::time::timeout(
        std::time::Duration::from_millis(50),
        dw.act(&slow, Some("cancel-original"))
    )
    .await
    .is_err());
    let stopped = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        dw.reconcile("cancel-original"),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(stopped.result.unwrap()["run_id"].is_string());
    assert_eq!(
        dw.get(&receipt.run_id).await.unwrap().run_id,
        receipt.run_id
    );
    assert!(dw.observe(json!({})).await.is_err());
    host.begin_turn("fresh").await.unwrap();
    assert!(host.grants().await.unwrap().grants.is_empty());
    host.close().await.unwrap();
}
#[test]
fn facts_preserve_empty_false_and_unrequested() {
    let empty: Fact<String> = serde_json::from_value(json!({"status":"known","value":""})).unwrap();
    assert_eq!(empty.known().unwrap(), "");
    let no: Fact<bool> = serde_json::from_value(json!({"known":false})).unwrap();
    assert_eq!(no.known(), Some(&false));
    let unrequested: Fact<bool> =
        serde_json::from_value(json!({"status":"unknown","reason":"not_requested"})).unwrap();
    assert!(unrequested.known().is_none());
}
