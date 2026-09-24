//! A guest's WASI stdout/stderr routed to the function's logger (stdout INFO, stderr WARN,
//! lines split at `\n` or 8 KiB) — used by (b) and (c).

use crate::util::{LineSink, LineSplitter};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use wasmtime_wasi::cli::{IsTerminal, StdoutStream};

#[derive(Clone)]
pub struct LogOut(pub Arc<Mutex<LineSplitter>>);

impl LogOut {
    pub fn new(level: &'static str, sink: LineSink) -> Self {
        Self(Arc::new(Mutex::new(LineSplitter::new(level, sink))))
    }
    pub fn flush_line(&self) {
        self.0.lock().unwrap().flush();
    }
}

impl IsTerminal for LogOut {
    fn is_terminal(&self) -> bool {
        false
    }
}

impl StdoutStream for LogOut {
    fn async_stream(&self) -> Box<dyn tokio::io::AsyncWrite + Send + Sync> {
        Box::new(self.clone())
    }
}

impl tokio::io::AsyncWrite for LogOut {
    fn poll_write(self: Pin<&mut Self>, _cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        self.0.lock().unwrap().write(buf);
        Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
