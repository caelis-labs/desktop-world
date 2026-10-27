//! Internal acceptance harness. Not installed or a public protocol.
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("Native checkpoint GUI harness currently requires macOS");
    std::process::exit(2);
}
#[cfg(target_os = "macos")]
fn main() {
    if desktop_world::backends::macos::worker_entry() {
        return;
    }
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
#[cfg(target_os = "macos")]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    use desktop_world::{
        backends::macos::NativeBackend,
        computer::*,
        runtime::{ActionReceipt, Grant, Runtime},
    };
    use serde_json::{json, Value};
    use std::{
        collections::HashMap,
        io::{BufRead, Write},
        sync::Arc,
        time::{Duration, Instant},
    };
    let args: Vec<_> = std::env::args().collect();
    let backend = NativeBackend::spawn().map_err(|e| format!("{e:?}"))?;
    let catalog = backend
        .catalog(Duration::from_secs(8))
        .map_err(|e| format!("{e:?}"))?;
    if args.get(1).map(String::as_str) == Some("--catalog") {
        println!(
            "{}",
            json!({"apps":catalog.iter().map(|a|json!({"pid":a.app.pid,"generation":a.app.generation,"identity":a.app.identity.0,"name":a.name})).collect::<Vec<_>>()})
        );
        return Ok(());
    }
    let pid: u32 = args
        .get(1)
        .ok_or("usage: native_checkpoint PID OUTPUT_DIRECTORY")?
        .parse()?;
    let output = std::path::PathBuf::from(args.get(2).ok_or("missing output directory")?);
    std::fs::create_dir_all(&output)?;
    let app = catalog
        .into_iter()
        .find(|a| a.app.pid == pid)
        .ok_or("PID absent from native app catalog")?;
    let runtime = Arc::new(Runtime::new(
        backend,
        Grant::new(
            app.app,
            vec![
                Capability::Observe,
                Capability::Press,
                Capability::Capture,
                Capability::Click,
            ],
            Instant::now() + Duration::from_secs(180),
        ),
        Duration::from_secs(15),
    ));
    let mut targets = HashMap::new();
    let mut observations = Vec::new();
    fn receipt(r: ActionReceipt) -> Value {
        json!({"dispatch":format!("{:?}",r.outcome.dispatch),"evidence":format!("{:?}",r.outcome.evidence),"error":r.outcome.error.map(|e|format!("{e:?}")),"interruption":r.interruption.map(|e|format!("{e:?}")),"provider_outcome":r.provider_outcome.map(|e|format!("{e:?}"))})
    }
    fn emit(v: Value) {
        println!("{v}");
        std::io::stdout().flush().unwrap();
    }
    emit(
        json!({"ready":true,"pid":pid,"coverage":"same-PID standard layer-0 AX windows; no sheets, menus, helpers or automatic focus"}),
    );
    for line in std::io::stdin().lock().lines() {
        let command: Value = serde_json::from_str(&line?)?;
        let operation = command["operation"].as_str().ok_or("missing operation")?;
        if operation == "exit" {
            break;
        }
        let timer = command["cancel_after_ms"].as_u64().map(|ms| {
            let runtime = runtime.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(ms));
                runtime.cancel().map_err(|e| format!("{e:?}"))
            })
        });
        let started = Instant::now();
        let result = (|| -> Result<Value, String> {
            if operation == "windows" {
                let list = runtime.windows().map_err(|e| format!("{e:?}"))?;
                let rows=list.iter().map(|w|json!({"id":w.target.native_id,"generation":w.target.generation,"title":w.title,"visible":w.visible})).collect::<Vec<_>>();
                for w in list {
                    targets.insert(w.target.native_id, w.target);
                }
                return Ok(json!({"windows":rows}));
            }
            if operation == "cancel" {
                runtime.cancel().map_err(|e| format!("{e:?}"))?;
                return Ok(json!({"cancelled":true}));
            }
            if operation == "close" {
                runtime.close().map_err(|e| format!("{e:?}"))?;
                return Ok(json!({"closed":true}));
            }
            let target = targets
                .get(&command["window"].as_u64().ok_or("missing window")?)
                .ok_or("unknown window")?;
            if operation == "observe" || operation == "capture" {
                let observed = if operation == "observe" {
                    runtime.observe(target)
                } else {
                    runtime.capture(target)
                }
                .map_err(|e| format!("{e:?}"))?;
                let id = observations.len();
                observations.push(observed.reference);
                let image = observed.image.map(|image| {
                    let path = output.join(format!("observation-{id}.png"));
                    std::fs::write(&path, image.png).unwrap();
                    json!({"path":path,"width":image.width,"height":image.height})
                });
                return Ok(
                    json!({"observation":id,"kind":format!("{:?}",observed.kind),"elements":observed.elements.iter().map(|e|json!({"id":e.id.0,"label":e.label,"can_press":e.can_press})).collect::<Vec<_>>(),"image":image}),
                );
            }
            let reference = *observations
                .get(
                    command["observation"]
                        .as_u64()
                        .ok_or("missing observation")? as usize,
                )
                .ok_or("unknown observation")?;
            match operation {
                "press" => Ok(receipt(runtime.press(
                    target,
                    reference,
                    ElementId(command["element"].as_u64().ok_or("missing element")?),
                ))),
                "click" => Ok(receipt(runtime.click(
                    target,
                    reference,
                    PixelPoint {
                        x: command["x"].as_f64().ok_or("missing x")?,
                        y: command["y"].as_f64().ok_or("missing y")?,
                    },
                ))),
                _ => Err("unknown internal harness operation".into()),
            }
        })();
        if let Some(timer) = timer {
            timer.join().unwrap()?;
        }
        let mut reply = result.unwrap_or_else(|error| json!({"error":error}));
        reply["elapsed_ms"] = json!(started.elapsed().as_millis() as u64);
        emit(reply);
    }
    runtime.close().map_err(|e| format!("{e:?}"))?;
    Ok(())
}
