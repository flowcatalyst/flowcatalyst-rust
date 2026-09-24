//! A guest's log lines on the function's own logger, `fn.<address>` (Java
//! `HostLogger` and `fnhost/wasm/GuestOutputStream.java`): the `log`
//! interface at the level the guest names, WASI standard output at INFO and
//! standard error at WARN, one line per `\n`, split at 8 KiB.
//!
//! A `tracing` target is a compile-time string, so the logger name travels
//! as the [`LOGGER_FIELD`](crate::logging::LOGGER_FIELD) field and the JSON
//! layer writes it as `logger`. Lines are emitted inside the invocation's
//! span, so they carry its `function`, `version`, `execution_id` and
//! `correlation_id`.

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use parking_lot::Mutex;
use tracing::Level;

use bytes::Bytes;
use wasmtime_wasi::cli::{IsTerminal, StdoutStream};
use wasmtime_wasi::p2::{OutputStream, Pollable, StreamResult};

/// A line longer than this is emitted in pieces (Java
/// `GuestOutputStream.MAX_LINE_BYTES`). A constant, not a knob.
pub const MAX_LINE_BYTES: usize = 8 * 1024;

/// The function's logger.
#[derive(Clone)]
pub struct GuestLogger {
    name: Arc<str>,
}

impl GuestLogger {
    /// `fn.<address>`.
    pub fn for_address(address: &str) -> Self {
        Self {
            name: format!("fn.{address}").into(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// One line at `level`.
    pub fn line(&self, level: Level, message: &str) {
        let logger = &*self.name;
        match level {
            Level::TRACE => tracing::trace!(target: "fn", fc_logger = logger, "{message}"),
            Level::DEBUG => tracing::debug!(target: "fn", fc_logger = logger, "{message}"),
            Level::INFO => tracing::info!(target: "fn", fc_logger = logger, "{message}"),
            Level::WARN => tracing::warn!(target: "fn", fc_logger = logger, "{message}"),
            Level::ERROR => tracing::error!(target: "fn", fc_logger = logger, "{message}"),
        }
    }
}

/// A guest's standard output or error. One per store; the host flushes it
/// after the call so a trailing partial line is not lost.
#[derive(Clone)]
pub struct GuestOutput(Arc<Mutex<LineBuffer>>);

struct LineBuffer {
    logger: GuestLogger,
    level: Level,
    line: Vec<u8>,
}

impl GuestOutput {
    pub fn new(logger: GuestLogger, level: Level) -> Self {
        Self(Arc::new(Mutex::new(LineBuffer {
            logger,
            level,
            line: Vec::new(),
        })))
    }

    pub fn push(&self, bytes: &[u8]) {
        let mut buffer = self.0.lock();
        for &b in bytes {
            if b == b'\n' {
                buffer.emit();
                continue;
            }
            buffer.line.push(b);
            if buffer.line.len() >= MAX_LINE_BYTES {
                buffer.emit();
            }
        }
    }

    /// Emits a pending partial line.
    pub fn flush(&self) {
        let mut buffer = self.0.lock();
        if !buffer.line.is_empty() {
            buffer.emit();
        }
    }
}

impl LineBuffer {
    fn emit(&mut self) {
        let mut text = String::from_utf8_lossy(&self.line).into_owned();
        self.line.clear();
        if text.ends_with('\r') {
            text.pop();
        }
        self.logger.line(self.level, &text);
    }
}

impl IsTerminal for GuestOutput {
    fn is_terminal(&self) -> bool {
        false
    }
}

/// The guest writes straight into the line buffer, inside its own call (so
/// inside the invocation's span): no background writer task, which is what
/// wasmtime-wasi's default `AsyncWrite` adapter would add.
impl StdoutStream for GuestOutput {
    fn p2_stream(&self) -> Box<dyn OutputStream> {
        Box::new(self.clone())
    }

    fn async_stream(&self) -> Box<dyn tokio::io::AsyncWrite + Send + Sync> {
        Box::new(self.clone())
    }
}

#[async_trait::async_trait]
impl Pollable for GuestOutput {
    async fn ready(&mut self) {}
}

impl OutputStream for GuestOutput {
    fn write(&mut self, bytes: Bytes) -> StreamResult<()> {
        self.push(&bytes);
        Ok(())
    }

    fn flush(&mut self) -> StreamResult<()> {
        Ok(())
    }

    fn check_write(&mut self) -> StreamResult<usize> {
        Ok(64 * 1024)
    }
}

impl tokio::io::AsyncWrite for GuestOutput {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        self.push(bytes);
        Poll::Ready(Ok(bytes.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
