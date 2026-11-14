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
        let mut config = ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("dtw", env!("CARGO_PKG_VERSION")))
            .with_instructions("Load the installed dtw Skill (desktop-world:dtw in Caelis) and API.md. Use the tools actually listed for this connection. apps discovers application authorization identities without reading contents. If exec is listed, call public app methods in an async JavaScript body; otherwise call the listed operation with the same name and object arguments, adding app. Every application call requires one explicit app identity. DTW owns startup, window routing and persistent observation state. Inspect view/effects to verify results; never replay input whose effects are unknown. Standard MCP cancellation stops its matching call.");
        config.server_info.description = self.server.provider_id();
        config
    }

    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let mut result = ListToolsResult::default();
        result.tools = serde_json::from_value(self.server.toolset().tools())
            .expect("static DTW tool definitions");
        Ok(result)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let compact = request.name != "apps";
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
        let completed_call = call.clone();
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
            // guard.cancel() may have settled shared cleanup after run()
            // produced its reply. Read the original record again, never return
            // that earlier running snapshot or replay the request.
            Ok(reply) if reply.data["status"] == "running" => Ok(result(
                self.server.completed_reply(&completed_call),
                compact,
            )),
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
    let methods = super::operations::ALL
        .iter()
        .map(|operation| operation.name)
        .collect::<Vec<_>>()
        .join(", ");
    let read = json!({"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true});
    json!([
        {"name":"apps","description":"Find installed and running user applications. Search by name or identifier; filter running/stopped/all. Returns authorization identities and running status, with bounded pages and explicit coverage. Reads no window contents.","inputSchema":{"type":"object","properties":{"query":{"type":"string","maxLength":256},"state":{"type":"string","enum":["all","running","stopped"]},"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":100}},"additionalProperties":false},"annotations":read},
        {"name":"exec","description":format!("Execute an async JavaScript body inside one explicitly authorized app. Public app operations: {methods}. Use object arguments documented in Skill API.md. DTW owns startup, window routing, input readiness and observation state. print emits bounded output. New calls execute new work; never replay unknown effects."),
            "inputSchema":{"type":"object","additionalProperties":false,"properties":{"app":app_id::schema(),"source":{"type":"string","minLength":1,"maxLength":65536,"description":"Async JavaScript function body; await app operations sequentially. No imports, filesystem, network or process APIs."}},"required":["app","source"]},
            "annotations":{"readOnlyHint":false,"destructiveHint":true,"idempotentHint":false,"openWorldHint":true}}
    ])
}
