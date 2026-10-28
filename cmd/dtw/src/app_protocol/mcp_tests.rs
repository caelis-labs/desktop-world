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
    async fn new(server: Arc<Server<Catalog>>) -> Self {
        Self::with_writer(server, "2025-06-18", None).await
    }

    fn connect(server: Arc<Server<Catalog>>, mode: Option<Arc<AtomicU8>>) -> Self {
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

    async fn with_writer(
        server: Arc<Server<Catalog>>,
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
async fn sdk_lists_three_tools_and_exec_accepts_only_app_and_source() {
    let (server, catalog) = server();
    let mut client = Client::new(server).await;
    let listed = client.rpc("tools/list", json!({})).await;
    let tools = listed["result"]["tools"].as_array().unwrap();
    assert_eq!(
        tools
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["apps", "exec", "result"]
    );
    let schema = &tools[1]["inputSchema"];
    assert_eq!(schema["required"], json!(["app", "source"]));
    assert_eq!(schema["properties"].as_object().unwrap().len(), 2);
    assert_eq!(schema["properties"]["app"]["type"], "string");
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(tools[1]["annotations"]["readOnlyHint"], false);
    assert_eq!(tools[2]["inputSchema"]["required"], json!(["app"]));
    let apps = decoded(&client.tool("apps", json!({})).await);
    assert_eq!(apps["apps"], json!([{"app":APP,"name":"Controlled"}]));
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
    let result = client.tool("exec", input("const w=(await app.windows()).windows[0].window_ref;await app.capture({window_ref:w});return 'captured';")).await;
    assert_eq!(result["isError"], false, "{result}");
    assert_eq!(decoded(&result)["value"]["result"], "captured");
    let image = &result["content"][1];
    assert_eq!(image["type"], "image");
    assert_eq!(image["mimeType"], "image/png");
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(image["data"].as_str().unwrap())
            .unwrap(),
        vec![7; 4096]
    );
    assert_eq!(catalog.backend.0.captures.load(Ordering::SeqCst), 1);
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
    assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), 3);
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
    assert_eq!(recovered["value"]["result"], json!(["before"]));
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
