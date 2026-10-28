//! Official MCP SDK adapter. Desktop state and effects remain owned by Server.
use super::{
    app_id,
    mcp_io::Lifecycle,
    problem,
    server::{Call, Factory, Server, Started},
    Reply,
};
use rmcp::{
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, CancelledNotificationParam,
        ContentBlock, Implementation, ListToolsResult, PaginatedRequestParams, ServerCapabilities,
        ServerConfig,
    },
    service::{NotificationContext, RequestContext},
    ErrorData, RoleServer, ServerHandler,
};
use serde_json::{json, Value};
use std::sync::Arc;

pub use super::mcp_io::{serve_io, MAX_INPUT_BYTES};

pub(super) struct Handler<F: Factory> {
    pub server: Arc<Server<F>>,
    pub lifecycle: Arc<Lifecycle>,
}

impl<F: Factory> ServerHandler for Handler<F> {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("desktop-world", env!("CARGO_PKG_VERSION")))
            .with_instructions("Call apps for application identifiers and compact method signatures. Call api for selected method schemas when needed. Call exec with app and source; DTW assigns execution_id and waits for completion. Standard MCP cancellation stops the matching call. result with app alone reads this session's latest accepted execution; optional execution_id selects an older receipt without replay. New MCP requests execute new work. DTW validates targets and confines execution.")
    }

    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let mut result = ListToolsResult::default();
        result.tools = serde_json::from_value(tools()).expect("static DTW tool definitions");
        Ok(result)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let compact = request.name == "exec";
        if context.ct.is_cancelled() || self.lifecycle.stopped.is_cancelled() {
            return Ok(result(
                problem("cancelled", "Call cancelled before admission"),
                compact,
            ));
        }
        let id = context.id.into_json_value();
        let args = Value::Object(request.arguments.unwrap_or_default());
        let call = match self.server.begin(id, &request.name, args) {
            Started::Reply(reply) => return Ok(result(reply, compact)),
            // Every duplicate waits for the same owner. Do not depend solely
            // on the SDK token: another duplicate can replace it for this ID.
            Started::InFlight(call) => {
                return tokio::select! {
                    _ = call.wait_finished() => Ok(result(self.server.completed_reply(&call), compact)),
                    _ = context.ct.cancelled() => Ok(result(problem("cancelled", "Original call owns this response"), compact)),
                };
            }
            Started::Call(call) => call,
        };
        let mut guard = CancelOnDrop {
            server: self.server.clone(),
            lifecycle: self.lifecycle.clone(),
            call: Some(call.clone()),
        };
        let server = self.server.clone();
        let token = context.ct.clone();
        let mut worker = tokio::task::spawn_blocking(move || {
            if token.is_cancelled() {
                server.cancel_call(&call);
            }
            // Even pre-dispatch cancellation must settle the accepted record.
            server.run(&call)
        });
        let completed = tokio::select! {
            completed = &mut worker => completed,
            _ = context.ct.cancelled() => {
                guard.cancel().await;
                worker.await
            }
        };
        guard.call = None;
        match completed {
            Ok(reply) => Ok(result(reply, compact)),
            Err(_) => {
                self.lifecycle.fail(
                    "execution_panicked",
                    "Tool worker failed; original effects may be unknown",
                );
                Err(ErrorData::internal_error(
                    "Desktop execution failed; connection is closing",
                    None,
                ))
            }
        }
    }

    async fn on_cancelled(
        &self,
        notification: CancelledNotificationParam,
        _: NotificationContext<RoleServer>,
    ) {
        // A repeated RPC frame can replace rmcp's token for the ID. Forward
        // cancellation to the execution owner as well, never replay its work.
        if let Some(id) = notification.request_id {
            let server = self.server.clone();
            if tokio::task::spawn_blocking(move || server.cancel(&id.into_json_value()))
                .await
                .is_err()
            {
                self.lifecycle
                    .fail("shutdown_failed", "Cancellation cleanup panicked");
            }
        }
    }
}

struct CancelOnDrop<F: Factory> {
    server: Arc<Server<F>>,
    lifecycle: Arc<Lifecycle>,
    call: Option<Arc<Call<F::Provider>>>,
}
impl<F: Factory> CancelOnDrop<F> {
    async fn cancel(&self) {
        if let Some(call) = self.call.clone() {
            let server = self.server.clone();
            if tokio::task::spawn_blocking(move || server.cancel_call(&call))
                .await
                .is_err()
            {
                self.lifecycle
                    .fail("shutdown_failed", "Cancellation cleanup panicked");
            }
        }
    }
}
impl<F: Factory> Drop for CancelOnDrop<F> {
    fn drop(&mut self) {
        if let Some(call) = self.call.take() {
            let server = self.server.clone();
            let lifecycle = self.lifecycle.clone();
            tokio::task::spawn_blocking(move || {
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    server.cancel_call(&call)
                }))
                .is_err()
                {
                    lifecycle.fail("shutdown_failed", "Dropped handler cleanup panicked");
                }
            });
        }
    }
}

fn result(reply: Reply, compact: bool) -> CallToolResponse {
    let reply = if compact {
        super::projection::execution(reply)
    } else {
        reply
    };
    let mut result = CallToolResult::structured(reply.data);
    result.is_error = Some(reply.is_error);
    if let Some(image) = reply.image {
        result
            .content
            .push(ContentBlock::image(image.as_ref().clone(), "image/png"));
    }
    result.into()
}

pub fn tools() -> Value {
    let id = json!({"type":"string","minLength":1,"maxLength":128,"description":"Optional DTW-generated execution ID from an earlier response in this MCP session. Omit to read this app's latest accepted execution."});
    let read = json!({"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true});
    json!([
        {"name":"apps","description":"List running applications with stable identifiers, compact script method signatures and provider conditions. Use api for selected argument and return schemas. No window contents or screenshots are read.","inputSchema":{"type":"object","properties":{},"additionalProperties":false},"annotations":read},
        {"name":"api","description":"Read detailed argument/return schemas and semantics for selected script methods. No native desktop read or input. Omit methods to list compact signatures.","inputSchema":{"type":"object","properties":{"methods":{"type":"array","minItems":1,"maxItems":10,"items":{"type":"string","enum":["windows","observe","capture","press","click","typeText","keypress","scroll","focus","setValue"]}}},"additionalProperties":false},"annotations":read},
        {"name":"exec","description":"Run JavaScript in exactly the app identified by the required app identifier. Use app.windows/observe/capture/press/click/typeText/keypress/scroll/focus/setValue; await methods sequentially. apps lists signatures; api returns selected schemas. The call waits for completion, returns a compact step/effect summary and the last PNG outside JS; result retrieves the full original ledger. state persists separately for each app in this MCP session. DTW generates execution_id; callers supply only app and source. A new MCP request ID executes new work even when source is identical. Inspect effects independently; cancellation is not undo.",
            "inputSchema":{"type":"object","additionalProperties":false,"properties":{"app":app_id::schema(),"source":{"type":"string","minLength":1,"maxLength":65536,"description":"JavaScript async function body, at most 64 KiB UTF-8; supports await and return. No imports, filesystem, network or process APIs."}},"required":["app","source"]},
            "annotations":{"readOnlyHint":false,"destructiveHint":true,"idempotentHint":false,"openWorldHint":true}},
        {"name":"result","description":"Read the full original execution ledger for this MCP session's latest accepted execution for the app, including a running or failed execution, cancellation and app exit. Optionally pass a DTW-generated execution_id to retrieve an older record. This is read-only and never replays work. Records do not cross connections; missing records never prove absence of effects.","inputSchema":{"type":"object","additionalProperties":false,"properties":{"app":app_id::schema(),"execution_id":id},"required":["app"]},"annotations":read}
    ])
}
