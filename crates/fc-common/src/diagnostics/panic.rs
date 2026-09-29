//! The process-wide panic hook: every panic becomes one `ERROR` log line
//! through `tracing`, with its message, location, thread, a backtrace, and
//! — because the line is emitted on the panicking thread while its spans
//! are still entered — the span context of whatever was running (a
//! message's id, pool and group; a job id; a function address). Nothing is
//! printed to bare stderr where the JSON log pipeline would drop it.
//!
//! Tokio still catches the unwind at the task boundary and each component
//! decides what a dead task means (see [`super::supervise`]); the hook only
//! guarantees the panic is never silent.

use std::any::Any;
use std::backtrace::Backtrace;
use std::panic;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, Once};
use std::thread;
use std::time::{Duration, Instant};
use tracing::dispatcher;

static PANICS: AtomicU64 = AtomicU64::new(0);
static INSTALL: Once = Once::new();

/// Backtraces captured per [`BACKTRACE_WINDOW`]. A task that panics on
/// every message would otherwise spend its CPU symbolising stacks; past the
/// budget the line is still logged, marked `backtrace_suppressed`.
const BACKTRACES_PER_WINDOW: u32 = 10;
const BACKTRACE_WINDOW: Duration = Duration::from_secs(10);

/// Panics seen since the process started (all threads).
pub fn panic_count() -> u64 {
    PANICS.load(Ordering::Relaxed)
}

/// Install the hook once; later calls are no-ops. Until a `tracing`
/// subscriber is installed the previous hook (Rust's default stderr
/// message) still runs, so a panic during start-up is never lost either.
pub fn install_panic_hook() {
    INSTALL.call_once(|| {
        let previous = panic::take_hook();
        let budget = Mutex::new((Instant::now(), 0u32));
        panic::set_hook(Box::new(move |info| {
            PANICS.fetch_add(1, Ordering::Relaxed);
            if !dispatcher::has_been_set() {
                previous(info);
                return;
            }
            let message = payload_text(info.payload());
            let location = info
                .location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                .unwrap_or_default();
            let thread = thread::current();
            let thread = thread.name().unwrap_or("<unnamed>");
            let span = tracing::Span::current();
            let span_name = span.metadata().map(|m| m.name()).unwrap_or("");
            let capture = {
                let mut b = budget.lock().unwrap_or_else(|p| p.into_inner());
                if b.0.elapsed() > BACKTRACE_WINDOW {
                    *b = (Instant::now(), 0);
                }
                b.1 += 1;
                b.1 <= BACKTRACES_PER_WINDOW
            };
            if capture {
                let backtrace = Backtrace::force_capture();
                tracing::error!(
                    target: "panic",
                    panic_message = %message,
                    panic_location = %location,
                    thread,
                    span = span_name,
                    backtrace = %backtrace,
                    "panic"
                );
            } else {
                tracing::error!(
                    target: "panic",
                    panic_message = %message,
                    panic_location = %location,
                    thread,
                    span = span_name,
                    backtrace_suppressed = true,
                    "panic"
                );
            }
        }));
    });
}

/// The text of a panic payload (`panic!("…")` gives a `&str` or `String`).
pub fn payload_text(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::any::Any;

    #[test]
    fn payload_text_reads_both_string_kinds() {
        let a: Box<dyn Any + Send> = Box::new("static");
        let b: Box<dyn Any + Send> = Box::new(String::from("owned"));
        let c: Box<dyn Any + Send> = Box::new(7u8);
        assert_eq!(payload_text(a.as_ref()), "static");
        assert_eq!(payload_text(b.as_ref()), "owned");
        assert_eq!(payload_text(c.as_ref()), "<non-string panic payload>");
    }
}
