//! Stdio lifecycle around SDK byte I/O, without a second protocol parser.
use super::{
    mcp::Handler,
    server::{Factory, Server},
    wire::WireError,
};
use rmcp::{transport::async_rw::AsyncRwTransport, ServiceExt};
use std::{
    future::Future,
    io,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_util::sync::CancellationToken;

pub const MAX_INPUT_BYTES: usize = 512 * 1024;
const OUTPUT_STALL_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Default)]
pub(super) struct Lifecycle {
    pub stopped: CancellationToken,
    failure: Mutex<Option<WireError>>,
}
impl Lifecycle {
    pub fn fail(&self, code: &str, message: &str) {
        self.failure
            .lock()
            .unwrap()
            .get_or_insert_with(|| failure(code, message));
        self.stopped.cancel();
    }
}
fn failure(code: &str, message: &str) -> WireError {
    WireError {
        code: code.into(),
        message: message.into(),
        details: None,
    }
}

/// One connection owns one Server. The handler/executor does not depend on stdio.
pub async fn serve_io<F, R, W>(server: Arc<Server<F>>, read: R, write: W) -> Result<(), WireError>
where
    F: Factory,
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let lifecycle = Arc::new(Lifecycle::default());
    let _stop_on_drop = lifecycle.stopped.clone().drop_guard();
    let closing = server.clone();
    let token = lifecycle.stopped.clone();
    let cleanup = tokio::spawn(async move {
        token.cancelled().await;
        tokio::task::spawn_blocking(move || closing.close())
            .await
            .map_err(|_| {
                failure(
                    "shutdown_failed",
                    "Cleanup panicked; native outcome remains unknown",
                )
            })?
    });
    let transport = AsyncRwTransport::new_server(
        ReadLifetime {
            inner: read,
            lifecycle: lifecycle.clone(),
        },
        WriteLifetime {
            inner: write,
            lifecycle: lifecycle.clone(),
            stalled: None,
        },
    )
    .with_max_line_length(MAX_INPUT_BYTES);
    let handler = Handler {
        server,
        lifecycle: lifecycle.clone(),
    };
    let outcome = match handler
        .serve_with_ct(transport, lifecycle.stopped.clone())
        .await
    {
        Ok(service) => service
            .waiting()
            .await
            .map(|_| ())
            .map_err(|_| failure("transport_error", "MCP service failed")),
        // A peer may close before initialize; EOF is still a normal shutdown.
        Err(_) if lifecycle.stopped.is_cancelled() => Ok(()),
        Err(error) => Err(failure(
            "transport_error",
            &format!("MCP initialization failed: {error}"),
        )),
    };
    lifecycle.stopped.cancel();
    cleanup
        .await
        .map_err(|_| failure("shutdown_failed", "MCP cleanup task failed"))??;
    if let Some(error) = lifecycle.failure.lock().unwrap().take() {
        return Err(error);
    }
    outcome
}

struct ReadLifetime<R> {
    inner: R,
    lifecycle: Arc<Lifecycle>,
}
impl<R: AsyncRead + Unpin> AsyncRead for ReadLifetime<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        let had_space = buf.remaining() > 0;
        let result = Pin::new(&mut this.inner).poll_read(cx, buf);
        match &result {
            Poll::Ready(Ok(())) if had_space && buf.filled().len() == before => {
                // rmcp otherwise drains handlers for five seconds before it
                // cancels them. Desktop actions must stop when the peer exits.
                this.lifecycle.stopped.cancel();
            }
            Poll::Ready(Err(_)) => this
                .lifecycle
                .fail("transport_error", "Cannot read MCP input"),
            _ => {}
        }
        result
    }
}

struct WriteLifetime<W> {
    inner: W,
    lifecycle: Arc<Lifecycle>,
    stalled: Option<Pin<Box<tokio::time::Sleep>>>,
}
impl<W> WriteLifetime<W> {
    fn checked<T>(
        &mut self,
        cx: &mut Context<'_>,
        result: Poll<io::Result<T>>,
    ) -> Poll<io::Result<T>> {
        match result {
            Poll::Ready(result) => {
                self.stalled = None;
                if result.is_err() {
                    self.lifecycle
                        .fail("transport_error", "Cannot write MCP response");
                }
                Poll::Ready(result)
            }
            Poll::Pending => {
                let timer = self
                    .stalled
                    .get_or_insert_with(|| Box::pin(tokio::time::sleep(OUTPUT_STALL_TIMEOUT)));
                if timer.as_mut().poll(cx).is_ready() {
                    self.lifecycle.fail(
                        "output_backpressure",
                        "MCP output is stalled; stopping native work",
                    );
                    Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "MCP output stalled",
                    )))
                } else {
                    Poll::Pending
                }
            }
        }
    }
}
impl<W: AsyncWrite + Unpin> AsyncWrite for WriteLifetime<W> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.inner).poll_write(cx, buf);
        let result = match result {
            Poll::Ready(Ok(0)) if !buf.is_empty() => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "MCP output stopped accepting bytes",
            ))),
            other => other,
        };
        this.checked(cx, result)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.inner).poll_flush(cx);
        this.checked(cx, result)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.inner).poll_shutdown(cx);
        this.checked(cx, result)
    }
}
