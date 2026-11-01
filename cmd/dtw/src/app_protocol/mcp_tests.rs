//! Tests use the real SDK byte transport and tool handler, with a controlled backend.
use super::*;
use std::{
    io,
    pin::Pin,
    sync::atomic::AtomicU8,
    task::{Context, Poll},
};
use tokio::io::{
    AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf,
};

const RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

struct Client {
    input: WriteHalf<DuplexStream>,
    output: BufReader<ReadHalf<DuplexStream>>,
    service: tokio::task::JoinHandle<Result<(), wire::WireError>>,
    serial: u64,
}

impl Client {
    async fn new<F: Factory>(server: Arc<Server<F>>) -> Self {
        Self::with_writer(server, "2025-06-18", None).await
    }

    fn connect<F: Factory>(server: Arc<Server<F>>, mode: Option<Arc<AtomicU8>>) -> Self {
        let (client, remote) = tokio::io::duplex(64 * 1024);
        let (read, write) = tokio::io::split(remote);
        let writer = ControlledWriter {
            inner: write,
            mode: mode.unwrap_or_default(),
        };
        let service = tokio::spawn(mcp::serve_io(server, read, writer));
        let (output, input) = tokio::io::split(client);
        Self {
            input,
            output: BufReader::new(output),
            service,
            serial: 0,
        }
    }

    async fn with_writer<F: Factory>(
        server: Arc<Server<F>>,
        version: &str,
        mode: Option<Arc<AtomicU8>>,
    ) -> Self {
        let mut client = Self::connect(server, mode);
        let initialized = client
            .rpc(
                "initialize",
                json!({"protocolVersion":version,"capabilities":{},"clientInfo":{"name":"dtw-test","version":"1"}}),
            )
            .await;
        assert_eq!(initialized["result"]["protocolVersion"], version);
        assert_eq!(initialized["result"]["serverInfo"]["name"], "desktop-world");
        client
            .send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .await;
        client
    }

    async fn send(&mut self, message: Value) {
        self.raw(serde_json::to_vec(&message).unwrap()).await;
    }

    async fn raw(&mut self, mut frame: Vec<u8>) {
        frame.push(b'\n');
        self.input.write_all(&frame).await.unwrap();
        self.input.flush().await.unwrap();
    }

    async fn receive(&mut self) -> Value {
        let mut line = String::new();
        let bytes = tokio::time::timeout(RESPONSE_TIMEOUT, self.output.read_line(&mut line))
            .await
            .expect("MCP response timed out")
            .expect("MCP output failed");
        assert_ne!(bytes, 0, "MCP output closed");
        serde_json::from_str(&line).expect("response is a JSON frame")
    }

    async fn reply(&mut self, id: Value) -> Value {
        loop {
            let response = self.receive().await;
            if response.get("id") == Some(&id) {
                return response;
            }
            // A cancelled call can race its terminal response. Its authoritative
            // receipt is separately checked with result; it is never replayed.
        }
    }

    async fn begin(&mut self, method: &str, params: Value) -> Value {
        self.serial += 1;
        let id = json!(self.serial);
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await;
        id
    }

    async fn rpc(&mut self, method: &str, params: Value) -> Value {
        let id = self.begin(method, params).await;
        self.reply(id).await
    }

    async fn tool(&mut self, name: &str, arguments: Value) -> Value {
        let response = self
            .rpc("tools/call", json!({"name":name,"arguments":arguments}))
            .await;
        assert!(response.get("error").is_none(), "{response}");
        response["result"].clone()
    }

    async fn close(mut self) {
        self.input.shutdown().await.unwrap();
        tokio::time::timeout(RESPONSE_TIMEOUT, self.service)
            .await
            .expect("MCP service did not close")
            .expect("MCP service panicked")
            .expect("MCP service failed");
    }
}

// Mode 0: ordinary writes; 1: broken pipe; 2: stalled pipe; 3: write-zero.
struct ControlledWriter {
    inner: WriteHalf<DuplexStream>,
    mode: Arc<AtomicU8>,
}
impl AsyncWrite for ControlledWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.mode.load(Ordering::SeqCst) {
            1 => Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, "controlled"))),
            2 => Poll::Pending,
            3 => Poll::Ready(Ok(0)),
            _ => Pin::new(&mut self.inner).poll_write(cx, buf),
        }
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

fn decoded(result: &Value) -> Value {
    let data: Value = serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(data, result["structuredContent"]);
    data
}

#[tokio::test]
async fn sdk_lists_four_tools_and_exec_accepts_only_app_and_source() {
    let (server, catalog) = server();
    let mut client = Client::new(server).await;
    let listed = client.rpc("tools/list", json!({})).await;
    let tools = listed["result"]["tools"].as_array().unwrap();
    assert_eq!(
        tools
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["apps", "api", "exec", "result"]
    );
    let schema = &tools[2]["inputSchema"];
    assert_eq!(schema["required"], json!(["app", "source"]));
    assert_eq!(schema["properties"].as_object().unwrap().len(), 2);
    assert_eq!(schema["properties"]["app"]["type"], "string");
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(tools[2]["annotations"]["readOnlyHint"], false);
    assert_eq!(tools[3]["inputSchema"]["required"], json!(["app"]));
    let apps = decoded(&client.tool("apps", json!({})).await);
    assert_eq!(apps["apps"], json!([{"app":APP,"name":"Controlled"}]));
    let entry = apps["script"]["entry"].as_str().unwrap();
    assert!(entry.contains("exec"));
    assert!(!entry.contains("request_id"), "{entry}");
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 0);
    for field in [
        "request_id",
        "execution_id",
        "file",
        "timeout_ms",
        "max_calls",
    ] {
        let mut args = input("return 1;");
        args[field] = json!(1);
        assert_eq!(client.tool("exec", args).await["isError"], true, "{field}");
    }
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 0);
    client.close().await;
}

#[tokio::test]
async fn sdk_omits_undefined_host_and_result_object_fields_but_preserves_strict_state() {
    let (server, catalog) = server();
    let mut client = Client::new(server).await;
    let result = client
        .tool(
            "exec",
            input(
                r#"
const window_ref=(await app.windows()).windows[0].window_ref;
async function q(role,text) { return app.observe({window_ref,role,text,limit:12}); }
const optional=await q(undefined,undefined);
const omitted=await app.observe({window_ref,limit:12});
state.saved=7;
return {optional:optional.elements.map(e=>e.name),omitted:omitted.elements.map(e=>e.name),diagnostics:{status:undefined,usable:true},unused:undefined};
"#,
            ),
        )
        .await;
    assert_eq!(result["isError"], false, "{result}");
    assert_eq!(
        decoded(&result)["value"]["result"],
        json!({"optional":["Increment"],"omitted":["Increment"],"diagnostics":{"usable":true}})
    );
    assert_eq!(catalog.backend.0.observes.load(Ordering::SeqCst), 2);
    let rejected = client
        .tool(
            "exec",
            input(
                r#"
const window_ref=(await app.windows()).windows[0].window_ref;
const codes=[];
for(const action of [
  ()=>app.observe({window_ref:undefined,role:undefined,text:undefined}),
  ()=>app.keypress({window_ref,keys:['META',undefined]}),
  ()=>app.observe(undefined),
  ()=>app.observe({window_ref,role:undefined,unexpected:1})
]) { try { await action(); } catch(e) { codes.push(e.code); } }
return codes;
"#,
            ),
        )
        .await;
    assert_eq!(rejected["isError"], false, "{rejected}");
    let rejected = decoded(&rejected);
    assert_eq!(
        rejected["value"]["result"],
        json!([
            "invalid_args",
            "invalid_json",
            "invalid_json",
            "invalid_args"
        ])
    );
    assert_eq!(rejected["script"]["summary"]["steps"], 1);
    for source in [
        "state.saved=8;state.optional=undefined;return true;",
        "state.saved=8;return [undefined];",
    ] {
        let result = client.tool("exec", input(source)).await;
        assert_eq!(result["isError"], true, "{result}");
        let result = decoded(&result);
        assert_eq!(result["script"]["failure"]["code"], "invalid_json");
        assert_eq!(result["script"]["state_committed"], false);
    }
    let final_state = decoded(&client.tool("exec", input("return state;")).await);
    assert_eq!(final_state["value"]["result"], json!({"saved":7}));
    assert_eq!(catalog.backend.0.observes.load(Ordering::SeqCst), 2);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
    client.close().await;
}

#[tokio::test]
async fn sdk_observe_empty_text_matches_omission_and_keeps_exact_argument_errors() {
    let (server, catalog) = server();
    let mut client = Client::new(server).await;
    let api = decoded(&client.tool("api", json!({"methods":["observe"]})).await);
    let fields = &api["methods"]["observe"]["parameters"]["oneOf"][0]["properties"];
    assert_eq!(fields["text"]["minLength"], 0);
    assert_eq!(fields["text"]["maxLength"], 256);
    assert_eq!(fields["role"]["minLength"], 1);
    let result = client
        .tool(
            "exec",
            input(
                r#"
const window_ref=(await app.windows()).windows[0].window_ref;
const absent=await app.observe({window_ref,limit:1});
const empty=await app.observe({window_ref,text:'',limit:1});
return {absent:absent.elements.map(e=>e.name),empty:empty.elements.map(e=>e.name),
        complete:absent.coverage.complete && empty.coverage.complete};
"#,
            ),
        )
        .await;
    assert_eq!(result["isError"], false, "{result}");
    assert_eq!(
        decoded(&result)["value"]["result"],
        json!({"absent":["Increment"],"empty":["Increment"],"complete":true})
    );
    assert_eq!(catalog.backend.0.observes.load(Ordering::SeqCst), 2);
    let rejected = client
        .tool(
            "exec",
            input(
                r#"
const window_ref=(await app.windows()).windows[0].window_ref;
const errors=[];
for(const args of [{window_ref,role:''},{window_ref,text:null},{window_ref,limit:101},
                   {window_ref:'',text:''},{cursor:'c',text:''},{window_ref,pid:42,text:''}]) {
  try { await app.observe(args); throw new Error('unexpectedly accepted'); }
  catch(e) { errors.push({code:e.code,message:e.message}); }
}
return errors;
"#,
            ),
        )
        .await;
    assert_eq!(rejected["isError"], false, "{rejected}");
    let rejected = decoded(&rejected);
    let errors = rejected["value"]["result"].as_array().unwrap();
    assert_eq!(errors.len(), 6);
    for (error, field) in errors.iter().zip([
        "command.role",
        "command.text",
        "command.limit",
        "target.window_ref",
        "command.text",
        "command.pid",
    ]) {
        assert_eq!(error["code"], "invalid_args");
        assert!(
            error["message"].as_str().unwrap().contains(field),
            "{error}"
        );
    }
    assert_eq!(rejected["value"]["calls"], 7);
    assert_eq!(rejected["script"]["summary"]["steps"], 1);
    assert_eq!(catalog.backend.0.observes.load(Ordering::SeqCst), 2);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
    client.close().await;
}

#[tokio::test]
async fn sdk_source_json_round_trip_preserves_javascript_and_result_characters() {
    let source = r#"const text = '中文🙂 "quoted" \'single\' \\';
const path = String.raw`C:\temp\new`;
const lines = "first\nsecond\tend";
const template = `中文🙂
${path}`;
return {text, path, lines, template, boundary: /\bword\b/.test("word"), literal: String.raw`\n`, nul: "\0"};"#;
    let expected = json!({"text":"中文🙂 \"quoted\" 'single' \\","path":"C:\\temp\\new",
        "lines":"first\nsecond\tend","template":"中文🙂\nC:\\temp\\new",
        "boundary":true,"literal":"\\n","nul":"\0"});
    let (server, catalog) = server();
    let mut client = Client::new(server).await;
    for (index, script) in [source.to_owned(), source.replace('\n', "\r\n")]
        .into_iter()
        .enumerate()
    {
        let id = json!(format!("source-{index}"));
        let request = json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"exec","arguments":input(&script)}});
        let mut encoded = serde_json::to_string(&request).unwrap();
        if index == 1 {
            encoded = encoded
                .replace('中', "\\u4e2d")
                .replace('🙂', "\\ud83d\\ude42");
        }
        client.raw(encoded.into_bytes()).await;
        let response = client.reply(id).await;
        assert_eq!(response["result"]["isError"], false, "{response}");
        assert_eq!(decoded(&response["result"])["value"]["result"], expected);
    }
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
    client.close().await;
}

#[tokio::test]
async fn sdk_source_json_underescaping_is_not_silently_repaired() {
    let (server, catalog) = server();
    let mut client = Client::new(server).await;
    // JSON's legal backspace escape changes the JS regex; SDK serialization
    // cannot infer or repair the caller's intended word-boundary expression.
    client.raw(br#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"exec","arguments":{"app":"org.example.Controlled","source":"return /\bword\b/.test(\"word\");"}}}"#.to_vec()).await;
    let response = client.reply(json!(2)).await;
    assert_eq!(response["result"]["isError"], false, "{response}");
    assert_eq!(decoded(&response["result"])["value"]["result"], false);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
    client.close().await;
}

#[tokio::test]
async fn sdk_returns_png_as_content_beside_matching_text_and_structured_result() {
    use base64::Engine;
    let (server, catalog) = server();
    let mut client = Client::new(server).await;
    let result = client.tool("exec", input("const w=(await app.windows()).windows[0].window_ref;await app.capture({window_ref:w});console.log('captured 中文🙂');return {label:'captured',nested:[1,true,null]};")).await;
    assert_eq!(result["isError"], false, "{result}");
    let compact = decoded(&result);
    assert_eq!(
        compact["value"]["result"],
        json!({"label":"captured","nested":[1,true,null]})
    );
    assert_eq!(compact["value"]["calls"], 2);
    assert_eq!(compact["value"]["last_capture"]["step"], 2);
    assert_eq!(compact["value"]["last_capture"]["timing"], "last_call");
    assert_eq!(compact["value"]["last_capture"]["later_calls"], 0);
    assert_eq!(compact["value"]["console_truncated"], false);
    let console = compact["value"]["console"].as_array().unwrap();
    assert_eq!(console.len(), 1);
    assert!(console[0].as_str().unwrap().contains("captured 中文🙂"));
    let image = &result["content"][1];
    assert_eq!(image["type"], "image");
    assert_eq!(image["mimeType"], "image/png");
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(image["data"].as_str().unwrap())
            .unwrap(),
        vec![7; 4096]
    );
    let retained = client
        .tool(
            "result",
            json!({"app":APP,"execution_id":compact["execution_id"]}),
        )
        .await;
    let full = decoded(&retained);
    assert_eq!(retained["isError"], result["isError"]);
    assert_eq!(full["value"], compact["value"]);
    assert_eq!(retained["content"][1], result["content"][1]);
    assert_eq!(full["script"]["steps"].as_array().unwrap().len(), 2);
    assert!(compact["script"].get("steps").is_none());
    assert_eq!(catalog.backend.0.captures.load(Ordering::SeqCst), 1);
    client.close().await;
}

#[tokio::test]
async fn sdk_exec_compacts_repeated_reads_while_result_retains_the_original_ledger() {
    let (server, catalog) = server();
    let mut client = Client::new(server.clone()).await;
    let response = client
        .tool(
            "exec",
            input(
                "const w=(await app.windows()).windows[0].window_ref;\n\
                 let observed;\n\
                 for(let i=0;i<24;i++) observed=await app.observe({window_ref:w});\n\
                 return {label:observed.elements[0].label,reads:24};",
            ),
        )
        .await;
    assert_eq!(response["isError"], false);
    let compact = decoded(&response);
    assert_eq!(compact["app"], APP);
    assert_eq!(compact["status"], "completed");
    assert!(compact["execution_id"].is_string());
    assert_eq!(
        compact["value"]["result"],
        json!({"label":"Increment","reads":24})
    );
    assert_eq!(compact["value"]["calls"], 25);
    assert_eq!(
        compact["script"]["summary"],
        json!({"steps":25,"failed":0,"pending":0,"incomplete_reads":0})
    );
    assert_eq!(compact["script"]["state_committed"], true);
    assert!(compact["script"]["failure"].is_null());
    assert!(compact["script"].get("steps").is_none());
    for field in ["app_ref", "operation", "receipt"] {
        assert!(compact.get(field).is_none(), "{field}: {compact}");
    }

    let result_args = json!({"app":APP,"execution_id":compact["execution_id"]});
    let retained = client.tool("result", result_args.clone()).await;
    let full = decoded(&retained);
    assert_eq!(retained["isError"], response["isError"]);
    assert_eq!(full, call(&server, "result", result_args.clone()).data);
    let steps = full["script"]["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 25);
    assert_eq!(steps[0]["operation"], "windows");
    assert!(steps[0]["inventory"].is_object());
    assert!(steps[1..].iter().all(|step| step["operation"] == "observe"));
    assert!(steps[1..].iter().all(|step| {
        let discovery = &step["discovery"];
        discovery["complete"] == true
            && discovery["reason"].is_null()
            && discovery["visited"] == 1
            && discovery["has_more"] == false
            && discovery["window_ref"].is_string()
            && discovery["window_ref"] == steps[1]["discovery"]["window_ref"]
            && discovery["observation_ref"].is_string()
    }));
    assert_eq!(
        steps[1..]
            .iter()
            .map(|step| step["discovery"]["observation_ref"].as_str().unwrap())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        24
    );
    assert!(full["script"].get("summary").is_none());
    for field in [
        "app",
        "execution_id",
        "status",
        "error",
        "value",
        "context_reset",
        "state_reset",
        "payload_expired",
    ] {
        assert_eq!(compact.get(field), full.get(field), "{field}");
    }
    for field in ["failure", "state_committed", "shutdown_error"] {
        assert_eq!(compact["script"].get(field), full["script"].get(field));
    }
    assert_eq!(client.tool("result", result_args).await, retained);
    let mut selected = decoded(&client.tool("result", json!({"app":APP})).await);
    assert!(selected
        .as_object_mut()
        .unwrap()
        .remove("selection")
        .is_some());
    assert_eq!(selected, full);

    let compact_bytes = serde_json::to_vec(&compact).unwrap().len();
    let full_bytes = serde_json::to_vec(&full).unwrap().len();
    eprintln!(
        "25 read steps, same execution: exec={compact_bytes} bytes, result={full_bytes} bytes, ratio={:.3}",
        compact_bytes as f64 / full_bytes as f64
    );
    assert!(compact_bytes * 2 < full_bytes);
    assert_eq!(catalog.backend.0.windows.load(Ordering::SeqCst), 1);
    assert_eq!(catalog.backend.0.observes.load(Ordering::SeqCst), 24);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
    client.close().await;
}

#[tokio::test]
async fn sdk_exec_keeps_caught_unknown_and_mismatch_evidence_in_the_summary() {
    for (provider, failed, caught) in [
        (
            ActionOutcome::unknown(Error::BackendFailed("controlled unknown action".into())),
            1,
            json!("backend_failed"),
        ),
        (
            ActionOutcome {
                delivery: None,
                dispatch: DispatchState::Unknown,
                evidence: EffectEvidence::Unverified,
                error: None,
            },
            0,
            Value::Null,
        ),
        (
            ActionOutcome {
                delivery: None,
                dispatch: DispatchState::Dispatched,
                evidence: EffectEvidence::Mismatch {
                    expected: "counter=1".into(),
                    observed: "counter=2".into(),
                },
                error: None,
            },
            0,
            Value::Null,
        ),
    ] {
        let (server, catalog) = server();
        catalog.backend.0.model.lock().unwrap().outcome = provider.clone();
        let mut client = Client::new(server).await;
        let response = client
            .tool(
                "exec",
                input(
                    "const w=(await app.windows()).windows[0].window_ref;\n\
                     const o=await app.observe({window_ref:w});\n\
                     let caught=null;\n\
                     try {await app.press({window_ref:w,observation_ref:o.observation_ref,element_ref:o.elements[0].element_ref});}\n\
                     catch(e) {caught=e.code;}\n\
                     await app.windows();\n\
                     return {caught};",
                ),
            )
            .await;
        assert_eq!(response["isError"], false, "{response}");
        let compact = decoded(&response);
        assert_eq!(compact["value"]["result"]["caught"], caught);
        assert_eq!(compact["script"]["state_committed"], true);
        assert!(compact["script"]["failure"].is_null());
        let summary = &compact["script"]["summary"];
        assert_eq!(summary["steps"], 4);
        assert_eq!(summary["failed"], failed);
        assert_eq!(summary["pending"], 0);
        let full = decoded(
            &client
                .tool(
                    "result",
                    json!({"app":APP,"execution_id":compact["execution_id"]}),
                )
                .await,
        );
        let issue = &summary["last_issue"];
        let step = &full["script"]["steps"][2];
        assert_eq!(issue["number"], 3);
        assert_eq!(issue["operation"], "press");
        for field in ["number", "line", "operation", "status", "error", "receipt"] {
            assert_eq!(issue.get(field), step.get(field), "{field}");
        }
        assert_eq!(
            issue["receipt"],
            wire::receipt(&crate::runtime::ActionReceipt {
                outcome: provider.clone(),
                provider_outcome: Some(provider),
                interruption: None,
            })
        );
        let dispatch = issue["receipt"]["outcome"]["dispatch"].as_str().unwrap();
        assert_eq!(summary["actions"]["dispatch"][dispatch], 1);
        assert_eq!(summary["actions"]["provider_dispatch"][dispatch], 1);
        let evidence = issue["receipt"]["outcome"]["evidence"]["kind"]
            .as_str()
            .unwrap();
        assert_eq!(summary["actions"]["evidence"][evidence], 1);
        assert_eq!(summary["actions"]["provider_evidence"][evidence], 1);
        assert_eq!(full["script"]["steps"][3]["operation"], "windows");
        assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
        assert_eq!(catalog.backend.0.windows.load(Ordering::SeqCst), 2);
        client.close().await;
    }
}

#[tokio::test]
async fn sdk_later_caught_read_error_does_not_hide_an_earlier_unknown_action() {
    let (server, catalog) = server();
    catalog.backend.0.model.lock().unwrap().outcome =
        ActionOutcome::unknown(Error::BackendFailed("original action failure".into()));
    let mut client = Client::new(server).await;
    let response = client
        .tool(
            "exec",
            input(
                "const w=(await app.windows()).windows[0].window_ref;\n\
                 const o=await app.observe({window_ref:w});\n\
                 try {await app.press({window_ref:w,observation_ref:o.observation_ref,element_ref:o.elements[0].element_ref});} catch(e) {}\n\
                 try {await app.observe({window_ref:'not-an-issued-window'});} catch(e) {}\n\
                 return 'caught both';",
            ),
        )
        .await;
    assert_eq!(response["isError"], false);
    let compact = decoded(&response);
    assert_eq!(compact["value"]["result"], "caught both");
    let summary = &compact["script"]["summary"];
    assert_eq!(summary["steps"], 4);
    assert_eq!(summary["failed"], 2);
    assert_eq!(summary["actions"]["dispatch"]["unknown"], 1);
    assert_eq!(summary["actions"]["provider_dispatch"]["unknown"], 1);
    assert_eq!(summary["last_issue"]["number"], 4);
    assert_eq!(summary["last_issue"]["operation"], "observe");
    assert_eq!(summary["last_issue"]["error"]["code"], "target_changed");
    assert!(summary["last_issue"]["receipt"].is_null());
    let full = decoded(
        &client
            .tool(
                "result",
                json!({"app":APP,"execution_id":compact["execution_id"]}),
            )
            .await,
    );
    let original_action = &full["script"]["steps"][2];
    assert_eq!(original_action["receipt"]["outcome"]["dispatch"], "unknown");
    assert_eq!(
        original_action["receipt"]["provider_outcome"]["error"]["message"],
        "original action failure"
    );
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    // The invalid read fails before native dispatch; reading result adds no work.
    assert_eq!(catalog.backend.0.observes.load(Ordering::SeqCst), 1);
    client.close().await;
}

#[tokio::test]
async fn sdk_completed_exec_retransmission_returns_the_same_summary_without_dispatch() {
    let (server, catalog) = server();
    let mut client = Client::new(server).await;
    let params = json!({"name":"exec","arguments":input(PRESS)});
    let id = client.begin("tools/call", params.clone()).await;
    let original = client.reply(id.clone()).await;
    let compact = decoded(&original["result"]);
    assert_eq!(original["result"]["isError"], false);
    assert_eq!(
        compact["script"]["summary"]["actions"]["dispatch"]["dispatched"],
        1
    );
    assert!(compact["script"]["summary"].get("last_issue").is_none());
    for _ in 0..3 {
        client
            .send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":params}))
            .await;
        assert_eq!(client.reply(id.clone()).await, original);
    }
    let full = decoded(
        &client
            .tool(
                "result",
                json!({"app":APP,"execution_id":compact["execution_id"]}),
            )
            .await,
    );
    assert_eq!(full["value"], compact["value"]);
    assert_eq!(full["script"]["steps"].as_array().unwrap().len(), 3);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(catalog.backend.0.windows.load(Ordering::SeqCst), 1);
    assert_eq!(catalog.backend.0.observes.load(Ordering::SeqCst), 1);
    client.close().await;
}

#[tokio::test]
async fn sdk_negotiates_versions_and_keeps_plain_text_results_readable() {
    for version in ["2024-11-05", "2025-06-18", "2025-11-25"] {
        let (server, _) = server();
        let mut client = Client::with_writer(server, version, None).await;
        let result = client.tool("apps", json!({})).await;
        let text: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text["apps"][0]["app"], APP);
        client.close().await;
    }
}

#[tokio::test]
async fn sdk_serves_current_protocol_with_per_request_metadata() {
    let (server, catalog) = server();
    let mut client = Client::connect(server, None);
    let meta = json!({"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}});
    let listed = client.rpc("tools/list", json!({"_meta":meta})).await;
    assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), 4);
    let response = client
        .rpc(
            "tools/call",
            json!({"_meta":meta,"name":"exec","arguments":input("return 'current';")}),
        )
        .await;
    assert_eq!(decoded(&response["result"])["value"]["result"], "current");
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
    client.close().await;
}

async fn reached(gate: TestGate) -> TestGate {
    tokio::task::spawn_blocking(move || {
        gate.entered();
        gate
    })
    .await
    .unwrap()
}

#[derive(Clone)]
struct CleanupCatalog {
    inner: Catalog,
    close_gate: Arc<Mutex<Option<WorkerGate>>>,
}
impl Factory for CleanupCatalog {
    type Provider = MockBackend;
    fn catalog(&self) -> Result<Vec<Application>, Error> {
        self.inner.catalog()
    }
    fn connect(&self) -> Result<MockBackend, Error> {
        self.inner.connect()
    }
    fn close_catalog(&self) -> Result<(), Error> {
        let gate = self.close_gate.lock().unwrap().take();
        if let Some(gate) = gate {
            gate.wait();
        }
        self.inner.close_catalog()
    }
}

#[tokio::test]
async fn sdk_completed_receipt_waits_for_outer_cancellation_cleanup() {
    let (_, catalog) = server();
    catalog.backend.0.model.lock().unwrap().poll_action = true;
    let factory = CleanupCatalog {
        inner: catalog.clone(),
        close_gate: Arc::default(),
    };
    let mut client = Client::new(Server::new(factory.clone())).await;
    let initial = decoded(
        &client
            .tool("exec", input("state.before=true;return 1;"))
            .await,
    );
    assert_eq!(initial["value"]["result"], 1, "{initial}");
    let action = catalog.backend.action_gate();
    let id = client
        .begin(
            "tools/call",
            json!({"name":"exec","arguments":input(PRESS)}),
        )
        .await;
    let action = reached(action).await;
    let running = decoded(&client.tool("result", json!({"app":APP})).await);
    let (gate, cleanup) = TestGate::pair();
    *factory.close_gate.lock().unwrap() = Some(gate);
    client
        .send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id}}))
        .await;
    let cleanup = reached(cleanup).await;
    // Core has produced its terminal receipt, but the outer run/cancel owner
    // still holds shared catalog resources. Public completion must cover both.
    let unsettled = tokio::time::timeout(RESPONSE_TIMEOUT, async {
        loop {
            let current = decoded(&client.tool("result", json!({"app":APP})).await);
            if current["error"].is_object() && current["script"]["steps"][2]["receipt"].is_object()
            {
                break current;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("Core did not publish its original terminal receipt");
    assert_eq!(
        unsettled["execution_id"], running["execution_id"],
        "{unsettled}"
    );
    assert_eq!(
        unsettled["script"]["steps"][2]["receipt"]["outcome"]["dispatch"], "unknown",
        "{unsettled}"
    );
    let blocked = decoded(
        &client
            .tool("exec", input("return Object.keys(state);"))
            .await,
    );
    assert_eq!(blocked["error"]["code"], "busy", "{blocked}");
    assert!(blocked.get("execution_id").is_none(), "{blocked}");
    assert_eq!(
        unsettled["status"], "running",
        "public record before cleanup: {unsettled}; admission: {blocked}"
    );
    cleanup.release();
    action.release();
    let settled = tokio::time::timeout(RESPONSE_TIMEOUT, async {
        loop {
            let current = decoded(&client.tool("result", json!({"app":APP})).await);
            if current["status"] == "completed" {
                break current;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("outer cleanup did not settle");
    assert_eq!(settled["execution_id"], running["execution_id"]);
    assert_eq!(settled["script"], unsettled["script"]);
    let recovered = decoded(
        &client
            .tool("exec", input("return Object.keys(state);"))
            .await,
    );
    assert_eq!(
        recovered["value"]["result"],
        json!(["before"]),
        "{recovered}"
    );
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    client.close().await;
}

#[tokio::test]
async fn sdk_cancellation_preserves_original_receipt_and_allows_later_work() {
    let (server, catalog) = server();
    catalog.backend.0.model.lock().unwrap().poll_action = true;
    let gate = catalog.backend.action_gate();
    let mut client = Client::new(server).await;
    client
        .tool("exec", input("state.before=true;return 1;"))
        .await;
    let id = client
        .begin(
            "tools/call",
            json!({"name":"exec","arguments":input(&format!("state.uncommitted=true;{PRESS}"))}),
        )
        .await;
    let gate = reached(gate).await;
    let running = decoded(&client.tool("result", json!({"app":APP})).await);
    assert_eq!(running["status"], "running");
    client
        .send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id}}))
        .await;
    let original = tokio::time::timeout(RESPONSE_TIMEOUT, async {
        loop {
            let current = decoded(&client.tool("result", json!({"app":APP})).await);
            if current["status"] == "completed" {
                break current;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("cancel did not settle");
    assert_eq!(original["execution_id"], running["execution_id"]);
    assert!(original["error"].is_object(), "{original}");
    let steps = original["script"]["steps"].as_array().unwrap();
    assert_eq!(
        steps.last().unwrap()["receipt"]["outcome"]["dispatch"],
        "unknown"
    );
    gate.release();
    let recovered = decoded(
        &client
            .tool("exec", input("return Object.keys(state);"))
            .await,
    );
    assert_eq!(
        recovered["value"]["result"],
        json!(["before"]),
        "recovery exec: {recovered}; original execution: {original}"
    );
    let retained = decoded(
        &client
            .tool(
                "result",
                json!({"app":APP,"execution_id":original["execution_id"]}),
            )
            .await,
    );
    // latest selection adds one presentation field; the stored receipt itself is unchanged.
    let mut original = original;
    original.as_object_mut().unwrap().remove("selection");
    assert_eq!(retained, original);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    client.close().await;
}

#[tokio::test]
async fn sdk_cancelled_exec_retransmission_preserves_provider_evidence_and_interruption() {
    let (server, catalog) = server();
    let provider = ActionOutcome {
        delivery: None,
        dispatch: DispatchState::Dispatched,
        evidence: EffectEvidence::Mismatch {
            expected: "counter=1".into(),
            observed: "counter=2".into(),
        },
        error: Some(Error::BackendFailed("late provider diagnostic".into())),
    };
    catalog.backend.0.model.lock().unwrap().outcome = provider.clone();
    let gate = catalog.backend.action_gate();
    let mut client = Client::new(server).await;
    let params = json!({"name":"exec","arguments":input(PRESS)});
    let id = client.begin("tools/call", params.clone()).await;
    let gate = reached(gate).await;
    client
        .send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id}}))
        .await;
    // Hold the provider reply until the real MCP cancellation has reached Core.
    // Its eventual diagnostic must coexist with the terminal interruption.
    tokio::time::timeout(RESPONSE_TIMEOUT, async {
        while catalog.backend.0.shutdowns.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("cancellation did not reach the native owner");
    gate.release();
    // The SDK may suppress a cancelled response. Settle through the original
    // record, then retransmit the same RPC frame to inspect its exec projection.
    let original = tokio::time::timeout(RESPONSE_TIMEOUT, async {
        loop {
            let current = decoded(&client.tool("result", json!({"app":APP})).await);
            if current["status"] == "completed" {
                break current;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("cancelled execution did not settle");
    client
        .send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":params}))
        .await;
    let response = client.reply(id).await;
    assert_eq!(response["result"]["isError"], true, "{response}");
    let compact = decoded(&response["result"]);
    assert_eq!(compact["execution_id"], original["execution_id"]);
    assert_eq!(compact["status"], "completed");
    assert_eq!(compact["script"]["state_committed"], false);
    let summary = &compact["script"]["summary"];
    assert_eq!(summary["steps"], 3);
    assert_eq!(summary["failed"], 1);
    assert_eq!(summary["pending"], 0);
    assert_eq!(summary["actions"]["dispatch"]["unknown"], 1);
    assert_eq!(summary["actions"]["provider_dispatch"]["dispatched"], 1);
    assert_eq!(summary["actions"]["provider_evidence"]["mismatch"], 1);
    assert_eq!(summary["actions"]["interrupted"], 1);
    assert_eq!(
        summary["last_issue"]["receipt"],
        wire::receipt(&crate::runtime::ActionReceipt {
            outcome: ActionOutcome::unknown(Error::Cancelled),
            provider_outcome: Some(provider),
            interruption: Some(Error::Cancelled),
        })
    );
    let retained = client
        .tool(
            "result",
            json!({"app":APP,"execution_id":compact["execution_id"]}),
        )
        .await;
    assert_eq!(retained["isError"], response["result"]["isError"]);
    let full = decoded(&retained);
    assert_eq!(full["error"], compact["error"]);
    assert_eq!(full["script"]["failure"], compact["script"]["failure"]);
    assert_eq!(
        full["script"]["steps"][2]["receipt"],
        summary["last_issue"]["receipt"]
    );
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    client.close().await;
}

#[tokio::test]
async fn sdk_eof_stops_native_work_without_waiting_for_sdk_drain_timeout() {
    let (server, catalog) = server();
    catalog.backend.0.model.lock().unwrap().poll_action = true;
    let gate = catalog.backend.action_gate();
    let mut client = Client::new(server.clone()).await;
    client
        .begin(
            "tools/call",
            json!({"name":"exec","arguments":input(PRESS)}),
        )
        .await;
    let gate = reached(gate).await;
    client.input.shutdown().await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), client.service)
        .await
        .expect("EOF waited for the SDK's five-second drain")
        .unwrap()
        .unwrap();
    assert!(catalog.backend.0.shutdowns.load(Ordering::SeqCst) > 0);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    assert_eq!(latest(&server, APP).data["status"], "completed");
    gate.release();
}

#[tokio::test]
async fn sdk_output_failure_stops_active_work_while_input_remains_open() {
    for mode in [1, 2, 3] {
        let (server, catalog) = server();
        catalog.backend.0.model.lock().unwrap().poll_action = true;
        let gate = catalog.backend.action_gate();
        let writer_mode = Arc::new(AtomicU8::new(0));
        let mut client =
            Client::with_writer(server.clone(), "2025-06-18", Some(writer_mode.clone())).await;
        client
            .begin(
                "tools/call",
                json!({"name":"exec","arguments":input(PRESS)}),
            )
            .await;
        let gate = reached(gate).await;
        writer_mode.store(mode, Ordering::SeqCst);
        client.begin("ping", json!({})).await;
        let result = tokio::time::timeout(Duration::from_secs(6), client.service)
            .await
            .expect("broken output did not close service")
            .unwrap();
        let error = result.expect_err("output failure was reported as a clean exit");
        assert_eq!(
            error.code,
            if mode == 2 {
                "output_backpressure"
            } else {
                "transport_error"
            },
            "mode {mode}"
        );
        assert!(catalog.backend.0.shutdowns.load(Ordering::SeqCst) > 0);
        assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
        assert_eq!(latest(&server, APP).data["status"], "completed");
        gate.release();
    }
}

#[tokio::test]
async fn sdk_duplicate_active_rpc_does_not_replay_or_detach_original_cancellation() {
    let (server, catalog) = server();
    catalog.backend.0.model.lock().unwrap().poll_action = true;
    let gate = catalog.backend.action_gate();
    let mut client = Client::new(server).await;
    let params = json!({"name":"exec","arguments":input(PRESS)});
    let id = client.begin("tools/call", params.clone()).await;
    let gate = reached(gate).await;
    let running = decoded(&client.tool("result", json!({"app":APP})).await);
    for _ in 0..3 {
        client
            .send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":params}))
            .await;
    }
    // This read lets the SDK dispatch the retransmitted frame before cancelling
    // and also verifies that the duplicate retained the original execution.
    let duplicate = decoded(&client.tool("result", json!({"app":APP})).await);
    assert_eq!(duplicate["execution_id"], running["execution_id"]);
    assert_eq!(duplicate["status"], "running");
    client
        .send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id}}))
        .await;
    let original = tokio::time::timeout(RESPONSE_TIMEOUT, async {
        loop {
            let current = decoded(&client.tool("result", json!({"app":APP})).await);
            if current["status"] == "completed" {
                break current;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("original execution did not cancel after duplicate frame");
    assert_eq!(original["execution_id"], running["execution_id"]);
    assert!(original["error"].is_object(), "{original}");
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    gate.release();
    client.close().await;
}

#[tokio::test]
async fn sdk_frame_limit_discards_oversized_input_without_dispatching_it() {
    let (server, catalog) = server();
    let mut client = Client::new(server).await;
    let mut exact =
        serde_json::to_vec(&json!({"jsonrpc":"2.0","id":"exact","method":"ping"})).unwrap();
    exact.resize(mcp::MAX_INPUT_BYTES - 1, b' ');
    client.raw(exact).await;
    assert_eq!(client.reply(json!("exact")).await["result"], json!({}));
    let mut oversized = serde_json::to_vec(&json!({"jsonrpc":"2.0","id":"oversized","method":"tools/call","params":{"name":"exec","arguments":input(PRESS)}})).unwrap();
    oversized.resize(mcp::MAX_INPUT_BYTES, b' ');
    client.raw(oversized).await;
    let result = client.rpc("ping", json!({})).await;
    assert_eq!(result["result"], json!({}));
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 0);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
    client.close().await;
}

#[tokio::test]
async fn sdk_eof_before_initialize_is_a_clean_exit_without_native_work() {
    let (server, catalog) = server();
    Client::connect(server, None).close().await;
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 0);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn sdk_incomplete_final_frame_never_dispatches_work() {
    let (server, catalog) = server();
    let mut client = Client::new(server).await;
    let bytes = serde_json::to_vec(&json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"exec","arguments":input(PRESS)}})).unwrap();
    client.input.write_all(&bytes).await.unwrap();
    client.close().await;
    assert_eq!(catalog.connects.load(Ordering::SeqCst), 0);
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn sdk_integer_and_string_rpc_ids_do_not_share_execution_identity() {
    let (server, catalog) = server();
    let mut client = Client::new(server).await;
    for (id, expected) in [(json!(2), 1), (json!("2"), 2)] {
        client.send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"exec","arguments":input("state.count=(state.count||0)+1;return state.count;")}})).await;
        assert_eq!(
            decoded(&client.reply(id).await["result"])["value"]["result"],
            expected
        );
    }
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 0);
    client.close().await;
}

#[tokio::test]
async fn sdk_multiple_active_duplicates_return_one_original_effect_and_response() {
    let (server, catalog) = server();
    let gate = catalog.backend.action_gate();
    let mut client = Client::new(server).await;
    let params = json!({"name":"exec","arguments":input(PRESS)});
    let id = client.begin("tools/call", params.clone()).await;
    let gate = reached(gate).await;
    for _ in 0..3 {
        client
            .send(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":params}))
            .await;
    }
    let running = decoded(&client.tool("result", json!({"app":APP})).await);
    assert_eq!(running["status"], "running");
    gate.release();
    let original = decoded(&client.reply(id).await["result"]);
    assert_eq!(original["execution_id"], running["execution_id"]);
    assert_eq!(original["status"], "completed");
    assert_eq!(catalog.backend.0.actions.load(Ordering::SeqCst), 1);
    let ping = client.begin("ping", json!({})).await;
    assert_eq!(
        client.receive().await["id"],
        ping,
        "duplicate response escaped SDK routing"
    );
    client.close().await;
}
