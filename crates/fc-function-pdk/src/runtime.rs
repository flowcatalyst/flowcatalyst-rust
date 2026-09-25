//! Running a handler: a single-threaded executor over `wasi:io/poll`, and
//! the glue between `wasi:http/incoming-handler` and a `#[handler]`
//! function.
//!
//! The executor is deliberately small. A component instance serves one
//! request on one thread, so all it needs is to poll the handler's future
//! and, whenever that future waits on host I/O (an outbound call, a body
//! stream), block in `wasi:io/poll.poll` on every pollable anything is
//! waiting for and wake the ones that became ready. Waiting on several at
//! once is what lets `join!` run outbound calls concurrently.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::{pin, Pin};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll, Wake, Waker};

use wasip2::http::types::{
    Fields, IncomingRequest, OutgoingBody, OutgoingResponse, ResponseOutparam,
};
use wasip2::io::poll::Pollable;

use crate::backend::Backend;
use crate::context::{Context, Level};
use crate::error::HandlerOutput;
use crate::request::Request;
use crate::Response;

/// Runs `future` to completion on this thread, waiting in
/// `wasi:io/poll.poll` whenever it waits on host I/O.
///
/// The `#[handler]` export runs the handler with it; tests can run a handler
/// with it too (natively, [`crate::testing::TestHost`]'s calls complete
/// without waiting).
///
/// # Panics
///
/// When the future is pending but neither waits on host I/O nor was woken:
/// nothing could ever wake it (for example, a channel whose sender was
/// never used).
pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let woken = Arc::new(Flag(AtomicBool::new(false)));
    let waker = Waker::from(woken.clone());
    let mut cx = TaskContext::from_waker(&waker);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return output;
        }
        if woken.0.swap(false, Ordering::SeqCst) {
            continue;
        }
        if !REACTOR.with(|reactor| reactor.borrow_mut().wait_any()) {
            panic!("the handler is waiting, but on no host I/O and nothing woke it: it can never finish");
        }
    }
}

struct Flag(AtomicBool);

impl Wake for Flag {
    fn wake(self: Arc<Self>) {
        self.0.store(true, Ordering::SeqCst);
    }
}

thread_local! {
    static REACTOR: RefCell<Reactor> = RefCell::new(Reactor::default());
}

#[derive(Default)]
struct Reactor {
    next: u64,
    waiting: BTreeMap<u64, Waiting>,
}

struct Waiting {
    pollable: Pollable,
    waker: Waker,
    ready: bool,
}

impl Reactor {
    /// Blocks until at least one waited-for pollable is ready, and wakes its
    /// waiters. False when nothing waits.
    fn wait_any(&mut self) -> bool {
        let pending: Vec<u64> = self
            .waiting
            .iter()
            .filter(|(_, w)| !w.ready)
            .map(|(key, _)| *key)
            .collect();
        if pending.is_empty() {
            return false;
        }
        let pollables: Vec<&Pollable> = pending.iter().map(|k| &self.waiting[k].pollable).collect();
        let ready = wasip2::io::poll::poll(&pollables);
        for index in ready {
            let waiting = self
                .waiting
                .get_mut(&pending[index as usize])
                .expect("poll answers indices of what it was given");
            waiting.ready = true;
            waiting.waker.wake_by_ref();
        }
        true
    }
}

/// Waits until `pollable` is ready.
pub(crate) fn wait(pollable: Pollable) -> Wait {
    Wait {
        pollable: Some(pollable),
        key: None,
    }
}

pub(crate) struct Wait {
    /// Until first registered with the reactor, which then owns it.
    pollable: Option<Pollable>,
    key: Option<u64>,
}

impl Future for Wait {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<()> {
        if let Some(pollable) = self.pollable.take() {
            if pollable.ready() {
                return Poll::Ready(());
            }
            let key = REACTOR.with(|reactor| {
                let mut reactor = reactor.borrow_mut();
                let key = reactor.next;
                reactor.next += 1;
                reactor.waiting.insert(
                    key,
                    Waiting {
                        pollable,
                        waker: cx.waker().clone(),
                        ready: false,
                    },
                );
                key
            });
            self.key = Some(key);
            return Poll::Pending;
        }
        let Some(key) = self.key else {
            return Poll::Ready(());
        };
        REACTOR.with(|reactor| {
            let mut reactor = reactor.borrow_mut();
            let waiting = reactor
                .waiting
                .get_mut(&key)
                .expect("a registered wait stays registered until it completes");
            if waiting.ready {
                reactor.waiting.remove(&key);
                self.key = None;
                Poll::Ready(())
            } else {
                waiting.waker.clone_from(cx.waker());
                Poll::Pending
            }
        })
    }
}

impl Drop for Wait {
    /// Drops the pollable (a child of the stream or future it watches, which
    /// must outlive it) as soon as nobody waits on it.
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            REACTOR.with(|reactor| reactor.borrow_mut().waiting.remove(&key));
        }
    }
}

/// The body of the `#[handler]` export: reads the request, runs the
/// handler, writes its response.
pub fn serve<F, Fut>(request: IncomingRequest, out: ResponseOutparam, handler: F)
where
    F: FnOnce(Request, Context) -> Fut,
    Fut: Future,
    Fut::Output: HandlerOutput,
{
    #[cfg(feature = "log")]
    crate::log_bridge::install();
    let backend: Rc<dyn Backend> = Rc::new(crate::wasi::WasiBackend::new());
    let response = match crate::wasi::read_request(request, backend.clone()) {
        Ok(request) => {
            let context = Context::new(backend.clone());
            match block_on(handler(request, context)).into_response() {
                Ok(response) => response,
                Err(message) => {
                    // The cause chain goes to the function's log only. The body
                    // is the host's generic failure, so an `Err` bubbling up
                    // with `?` can't leak internals (hosts, SQL, paths) to a
                    // caller, which matters on public routes. A handler that
                    // wants to tell the caller something returns
                    // `Response::fail(message)` explicitly.
                    backend.log(Level::Error, &format!("the handler failed: {message}"));
                    Response::function_failed()
                }
            }
        }
        Err(why) => {
            backend.log(
                Level::Error,
                &format!("the request could not be read: {why}"),
            );
            Response::function_failed()
        }
    };
    write_response(out, response, &*backend);
}

/// Sends `response`. A header `wasi:http` refuses (a forbidden or malformed
/// name or value) is the handler's bug: it is logged and the caller gets the
/// host's `500 {"error":"the function failed"}` instead.
fn write_response(out: ResponseOutparam, response: Response, backend: &dyn Backend) {
    let (status, headers, body) = response.into_parts();
    let entries: Vec<(String, Vec<u8>)> = headers
        .into_iter()
        .flat_map(|(name, values)| {
            values
                .into_iter()
                .map(move |value| (name.clone(), value.into_bytes()))
        })
        .collect();
    let (status, fields, body) = match Fields::from_list(&entries) {
        Ok(fields) => (status, fields, body),
        Err(e) => {
            backend.log(
                Level::Error,
                &format!("the handler's response headers were refused: {e:?}"),
            );
            let (status, headers, body) = Response::function_failed().into_parts();
            let entries: Vec<(String, Vec<u8>)> = headers
                .into_iter()
                .flat_map(|(n, vs)| vs.into_iter().map(move |v| (n.clone(), v.into_bytes())))
                .collect();
            let fields = Fields::from_list(&entries).expect("the host's own headers are legal");
            (status, fields, body)
        }
    };
    let outgoing = OutgoingResponse::new(fields);
    outgoing
        .set_status_code(status)
        .expect("Response keeps its status within 100-599");
    let outgoing_body = outgoing.body().expect("the body is taken once");
    ResponseOutparam::set(out, Ok(outgoing));
    {
        let stream = outgoing_body.write().expect("the stream is taken once");
        for chunk in body.chunks(4096) {
            if stream.blocking_write_and_flush(chunk).is_err() {
                return; // the host stopped reading (the caller went away)
            }
        }
    }
    let _ = OutgoingBody::finish(outgoing_body, None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_on_runs_a_future_that_needs_no_io() {
        assert_eq!(block_on(async { 40 + 2 }), 42);
    }

    #[test]
    fn block_on_repolls_a_future_that_woke_itself() {
        struct YieldOnce(bool);
        impl Future for YieldOnce {
            type Output = &'static str;
            fn poll(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Self::Output> {
                if self.0 {
                    Poll::Ready("done")
                } else {
                    self.0 = true;
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
            }
        }
        assert_eq!(block_on(YieldOnce(false)), "done");
    }

    /// On the real `wasi:io/poll`: two waits run concurrently, and each
    /// pollable is gone once its wait is.
    #[cfg(target_arch = "wasm32")]
    #[test]
    fn waits_on_host_pollables_run_concurrently() {
        use wasip2::clocks::monotonic_clock::{now, subscribe_duration};

        struct Both<A, B>(Option<A>, Option<B>);
        impl<A: Future<Output = ()> + Unpin, B: Future<Output = ()> + Unpin> Future for Both<A, B> {
            type Output = ();
            fn poll(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<()> {
                if let Some(a) = self.0.as_mut() {
                    if Pin::new(a).poll(cx).is_ready() {
                        self.0 = None;
                    }
                }
                if let Some(b) = self.1.as_mut() {
                    if Pin::new(b).poll(cx).is_ready() {
                        self.1 = None;
                    }
                }
                if self.0.is_none() && self.1.is_none() {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            }
        }

        let ms = 1_000_000;
        let start = now();
        block_on(Both(
            Some(wait(subscribe_duration(60 * ms))),
            Some(wait(subscribe_duration(60 * ms))),
        ));
        let took = now() - start;
        assert!((60 * ms..110 * ms).contains(&took), "took {} ms", took / ms);
        assert!(REACTOR.with(|r| r.borrow().waiting.is_empty()));

        // A wait dropped before it completes lets its pollable go.
        let mut early = Box::pin(wait(subscribe_duration(10_000 * ms)));
        let waker = Waker::from(Arc::new(Flag(AtomicBool::new(false))));
        assert!(early
            .as_mut()
            .poll(&mut TaskContext::from_waker(&waker))
            .is_pending());
        assert_eq!(REACTOR.with(|r| r.borrow().waiting.len()), 1);
        drop(early);
        assert!(REACTOR.with(|r| r.borrow().waiting.is_empty()));
    }

    #[test]
    #[should_panic(expected = "it can never finish")]
    fn block_on_refuses_to_wait_forever() {
        block_on(std::future::pending::<()>());
    }
}
