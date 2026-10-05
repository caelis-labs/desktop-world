# Desktop World Rust SDK

Rust 1.75+, Tokio and Serde; no Node/Go runtime requirement when using a prebuilt native `dtw`. Add the SDK directory from the RC/source archive:

```toml
[dependencies]
caelis-desktop-world = { path = "/trusted/path/clients/rust" }
tokio = { version = "1", features = ["rt", "macros"] }
serde_json = "1"
```

```rust,no_run
use caelis_desktop_world::{HostSession, SessionOptions, Plan};
use serde_json::json;
#[tokio::main(flavor="current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut options = SessionOptions::new("/trusted/bin/dtw");
    options.write_apps.push("Your APP".into());
    let host = HostSession::start(options).await?;
    let inventory = host.desktop.observe(json!({})).await?;
    println!("{:?}", inventory.coverage);
    // Discover exact APP/window/field Refs, check complete unique selection,
    // then call host.grant(&app_ref).await? from trusted owner code.
    // let mut plan = Plan::new(); plan.set(&field_ref,"Hello 中文");
    // let receipt = host.desktop.act(&plan,Some("task-write-1")).await?;
    host.close().await?;
    Ok(())
}
```

`Plan::bind_focus(name,window_ref)` returns a typed `Target` for `press`/`type_text`. Gather focus, bind and input in one plan. `HostSession.declare/grant/revoke/grants/end_turn` manages dynamic authority. `Error.reply` retains full failed/partial receipts; `desktop.reconcile(id)` waits for the original request. Cancellation/drop of a pending request sends `end_turn`; explicit `close()` reports forced cleanup limits. Tokio runtime must remain alive while the session is owned. Use an absolute `dtw.exe` path on Windows.

Fact states, extension fields, coverage and decimal-string versions remain available. Lower-level `call()` supports the seven desktop verbs only. [Shared integration contract](../../docs/agent-integration.md) covers discovery, Windows and no replay rules. No crates.io publication is implied. Run `python scripts/check-sdks.py` from the repository root.
