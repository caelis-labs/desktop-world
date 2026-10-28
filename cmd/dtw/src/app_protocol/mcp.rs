//! MCP messages only. Application execution and cleanup live in Server.
use super::{app_id, wire};
use serde_json::{json, Value};
use std::io::BufRead;
pub const MAX_INPUT_BYTES: usize = 512 * 1024;
const CURRENT_VERSION: &str = "2025-06-18";
#[derive(Default)]
pub struct Session {
    initialized: bool,
    ready: bool,
    structured: bool,
}
pub enum Event {
    None,
    Reply(Value),
    Cancel(Value),
    Call {
        id: Value,
        name: String,
        args: Value,
        structured: bool,
    },
}
impl Session {
    pub fn handle(&mut self, input: Value) -> Event {
        let id = input.get("id").and_then(rpc_id);
        if !input.is_object()
            || input["jsonrpc"] != "2.0"
            || !input["method"].is_string()
            || (input.get("id").is_some() && id.is_none())
        {
            return Event::Reply(rpc_error(Value::Null, -32600, "Invalid JSON-RPC request"));
        }
        let method = input["method"].as_str().unwrap();
        let params = &input["params"];
        let Some(id) = id else {
            if method == "notifications/initialized" && self.initialized {
                self.ready = true;
            }
            if method == "notifications/cancelled" && self.ready {
                if let Some(id) = params.get("requestId").and_then(rpc_id) {
                    return Event::Cancel(id);
                }
            }
            return Event::None;
        };
        let result = match method {
            "initialize" => {
                if self.initialized {
                    return Event::Reply(rpc_error(id, -32600, "Already initialized"));
                }
                let Some(requested) = params["protocolVersion"].as_str() else {
                    return Event::Reply(rpc_error(id, -32602, "protocolVersion is required"));
                };
                let version = match requested {
                    "2024-11-05" | CURRENT_VERSION => requested,
                    _ => CURRENT_VERSION,
                };
                self.initialized = true;
                self.structured = version == CURRENT_VERSION;
                json!({"protocolVersion":version,"capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"desktop-world","version":env!("CARGO_PKG_VERSION")},
                    "instructions":"Call apps for application identifiers and script API. Call exec with app and source; DTW assigns execution_id and waits for completion. Standard MCP cancellation stops the matching call. result with app alone reads this session's latest accepted execution; optional execution_id selects an older receipt without replay. Repeated MCP request IDs reuse the original execution only within this connection; new request IDs execute new work. DTW validates targets and confines execution."})
            }
            "ping" => json!({}),
            _ if !self.ready => {
                return Event::Reply(rpc_error(id, -32002, "Initialize the session first"))
            }
            "tools/list" => json!({"tools":tools()}),
            "tools/call" => {
                let Some(name) = params["name"]
                    .as_str()
                    .filter(|n| ["apps", "exec", "result"].contains(n))
                else {
                    return Event::Reply(rpc_error(id, -32602, "Unknown tool"));
                };
                return Event::Call {
                    id,
                    name: name.into(),
                    args: params
                        .get("arguments")
                        .cloned()
                        .unwrap_or_else(|| json!({})),
                    structured: self.structured,
                };
            }
            _ => return Event::Reply(rpc_error(id, -32601, "Method not found")),
        };
        Event::Reply(json!({"jsonrpc":"2.0","id":id,"result":result}))
    }
}
// JSON-RPC integer IDs can arrive as `1` or `1.0`; they must identify the
// same call for both retransmission and cancellation. Do not round a floating
// point value outside the exact integer range into a different request ID.
fn rpc_id(value: &Value) -> Option<Value> {
    if value.is_string() || value.is_i64() || value.is_u64() {
        return Some(value.clone());
    }
    let number = value.as_f64()?;
    (number.fract() == 0.0 && number.abs() <= 9_007_199_254_740_991.0).then(|| json!(number as i64))
}
pub fn tools() -> Value {
    let id = json!({"type":"string","minLength":1,"maxLength":128,"description":"Optional DTW-generated execution ID from an earlier response in this MCP session. Omit to read this app's latest accepted execution."});
    let read = json!({"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true});
    json!([
        {"name":"apps","description":"List running applications with stable identifiers and discover the supported app script API, schemas and limits. No window contents or screenshots are read.","inputSchema":{"type":"object","properties":{},"additionalProperties":false},"annotations":read},
        {"name":"exec","description":"Run JavaScript in exactly the app identified by the required app identifier. Use app.windows(), app.observe({window_ref}), app.capture({window_ref}), app.press({window_ref,observation_ref,element_ref}), app.click({window_ref,observation_ref,x,y}); await methods sequentially. apps provides full schemas. The call waits for completion, returns original per-step receipts and the last PNG outside JS. state persists separately for each app in this MCP session. DTW generates execution_id; callers supply only app and source plus optional budgets. A new MCP request ID executes new work even when source is identical. Inspect effects independently; cancellation is not undo.",
            "inputSchema":{"type":"object","additionalProperties":false,"properties":{"app":app_id::schema(),"source":{"type":"string","minLength":1,"maxLength":65536,"description":"JavaScript async function body, at most 64 KiB UTF-8; supports await and return. No imports, filesystem, network or process APIs."},"timeout_ms":{"type":"integer","minimum":1,"maximum":super::scripts::MAX_TIMEOUT_MS,"default":super::scripts::DEFAULT_TIMEOUT_MS,"description":"Optional caller deadline; overrides the default in either direction. Standard MCP cancellation remains available."},"max_calls":{"type":"integer","minimum":1,"maximum":super::scripts::MAX_CALLS,"description":"Optional caller-selected bridge attempt budget. Omit for no call-count quota; deadline, VM memory and receipt byte budgets still apply."}},"required":["app","source"]},
            "annotations":{"readOnlyHint":false,"destructiveHint":true,"idempotentHint":false,"openWorldHint":true}},
        {"name":"result","description":"Read this MCP session's latest accepted execution for the app, including a running or failed execution, cancellation and app exit. Optionally pass a DTW-generated execution_id to retrieve an older record. This is read-only and never replays work. Records do not cross connections; missing records never prove absence of effects.","inputSchema":{"type":"object","additionalProperties":false,"properties":{"app":app_id::schema(),"execution_id":id},"required":["app"]},"annotations":read}
    ])
}
pub fn response(id: Value, reply: super::Reply, structured: bool) -> Value {
    let mut value = reply.mcp();
    if !structured {
        value.as_object_mut().unwrap().remove("structuredContent");
    }
    json!({"jsonrpc":"2.0","id":id,"result":value})
}
pub fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
pub fn read_frame<R: BufRead>(reader: &mut R) -> Result<Option<Vec<u8>>, wire::WireError> {
    let mut line = Vec::new();
    std::io::Read::take(&mut *reader, (MAX_INPUT_BYTES + 1) as u64)
        .read_until(b'\n', &mut line)
        .map_err(|_| wire::WireError {
            code: "transport_error".into(),
            message: "Cannot read MCP input".into(),
            details: None,
        })?;
    if line.is_empty() {
        return Ok(None);
    }
    if line.len() > MAX_INPUT_BYTES || line.last() != Some(&b'\n') {
        return Err(wire::WireError {
            code: "invalid_frame".into(),
            message: "MCP input must be a complete line of at most 512 KiB".into(),
            details: None,
        });
    }
    Ok(Some(line))
}
