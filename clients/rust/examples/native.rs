use caelis_desktop_world::{Fact, HostSession, Observation, Plan, SessionOptions};
use serde_json::json;
fn one(ob: &Observation, name: &str) -> Result<String, Box<dyn std::error::Error>> {
    if !ob.coverage.complete
        || ob.coverage.dirty
        || ob.coverage.truncated
        || !ob.coverage.unavailable_sources.is_empty()
    {
        return Err("incomplete native coverage".into());
    }
    let objects: Vec<_> = ob
        .objects
        .iter()
        .filter(|o| {
            o.name
                .as_ref()
                .and_then(Fact::known)
                .is_some_and(|v| v == name)
        })
        .collect();
    if objects.len() != 1 {
        return Err("native match is not unique".into());
    }
    Ok(objects[0].r#ref.clone())
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let title = std::env::var("DTW_NATIVE_TITLE")?;
    let token = std::env::var("DTW_NATIVE_TOKEN")?;
    let mut options = SessionOptions::new(std::env::var("DTW_NATIVE_HELPER")?);
    options.write_app_windows.push(title.clone());
    let host = HostSession::start(options).await?;
    let dw = &host.desktop;
    let inv = dw
        .observe(json!({"budget":{"max_results":256,"max_output_bytes":65536}}))
        .await?;
    let window = one(&inv, &title)?;
    let field = one(
        &dw.find(&window, json!({"role":"text_field","name_equals":"内容"}))
            .await?,
        "内容",
    )?;
    let submit = one(
        &dw.find(&window, json!({"role":"button","name_equals":"提交"}))
            .await?,
        "提交",
    )?;
    let mut plan = Plan::new();
    plan.focus(&field);
    let focused = plan.bind_focus("input", &window);
    plan.press(focused.clone(), "A", &["primary"])
        .type_text(focused, &token)
        .invoke(&submit);
    let receipt = dw.act(&plan, Some("native-rust-original")).await?;
    assert_eq!(
        dw.act(&plan, Some("native-rust-original")).await?.run_id,
        receipt.run_id
    );
    assert_eq!(dw.read(&field).await?["text"]["value"], token);
    println!(
        "{}",
        json!({"language":"rust","run_id":receipt.run_id,"input":receipt.extra.get("input"),"verified":true})
    );
    host.close().await?;
    Ok(())
}
