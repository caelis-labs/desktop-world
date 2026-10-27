//! Small MCP stdio contract. Native jobs have their own IDs and cancellation.
use super::{wire, Coordinator, Reply};
use crate::computer::Backend;
use serde_json::{json, Value};
use std::{io::BufRead, sync::Arc};

pub const MAX_INPUT_BYTES: usize = 64 * 1024;
const CURRENT_VERSION: &str = "2025-06-18";

#[derive(Default)]
pub struct Session {
    initialized: bool,
    ready: bool,
    structured: bool,
}
impl Session {
    pub fn handle<B: Backend + 'static>(
        &mut self,
        owner: &Arc<Coordinator<B>>,
        input: Value,
    ) -> Option<Value> {
        let id = input.get("id").cloned();
        if !input.is_object()
            || input["jsonrpc"] != "2.0"
            || !input["method"].is_string()
            || id.as_ref().is_some_and(|id| {
                !(id.is_string() || id.as_f64().is_some_and(|number| number.fract() == 0.0))
            })
        {
            return Some(rpc_error(Value::Null, -32600, "Invalid JSON-RPC request"));
        }
        let method = input["method"].as_str().unwrap();
        let Some(id) = id else {
            if method == "notifications/initialized" && self.initialized {
                self.ready = true;
            }
            // MCP cancellation addresses a completed transport request, not a
            // native job. Only exec.cancel can cancel an accepted request_id.
            return None;
        };
        let params = &input["params"];
        let result = match method {
            "initialize" => {
                if self.initialized {
                    return Some(rpc_error(id, -32600, "Already initialized"));
                }
                let Some(requested) = params["protocolVersion"].as_str() else {
                    return Some(rpc_error(id, -32602, "protocolVersion is required"));
                };
                let version = match requested {
                    "2024-11-05" | "2025-03-26" | CURRENT_VERSION => requested,
                    _ => CURRENT_VERSION,
                };
                self.initialized = true;
                self.structured = version == CURRENT_VERSION;
                json!({"protocolVersion":version,"capabilities":{"tools":{"listChanged":false}},
                    "serverInfo":{"name":"dtw-app","version":env!("CARGO_PKG_VERSION")},
                    "instructions":"This connection has one immutable Host app grant. Start with exec describe. Native jobs use explicit request_id; poll result and preserve the receipt. Cancellation never proves that an effect was undone."})
            }
            "ping" => json!({}),
            _ if !self.ready => return Some(rpc_error(id, -32002, "Initialize the session first")),
            "tools/list" => json!({"tools":[{"name":"exec",
                "description":"Execute the versioned single-app protocol. describe reports Host authority and limits. submit starts one typed command; result reads its original record; cancel terminates the context for the active request; close releases resources. Reuse an accepted request_id only with the identical command. No scripts or raw native tools.",
                "inputSchema":wire::input_schema(),
                "annotations":{"readOnlyHint":false,"destructiveHint":true,"idempotentHint":false,"openWorldHint":true}}]}),
            "tools/call" => {
                if params["name"] != "exec" {
                    return Some(rpc_error(id, -32602, "Unknown tool"));
                }
                let reply = match wire::parse_request(params["arguments"].clone()) {
                    Ok(request) => owner.handle(request),
                    Err(error) => Reply::error(error),
                };
                let mut value = reply.mcp();
                if !self.structured {
                    value.as_object_mut().unwrap().remove("structuredContent");
                }
                value
            }
            _ => return Some(rpc_error(id, -32601, "Method not found")),
        };
        Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
    }
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
        })?;
    if line.is_empty() {
        return Ok(None);
    }
    if line.len() > MAX_INPUT_BYTES || line.last() != Some(&b'\n') {
        return Err(wire::WireError {
            code: "invalid_frame".into(),
            message: "MCP input must be a complete line of at most 64 KiB".into(),
        });
    }
    Ok(Some(line))
}
