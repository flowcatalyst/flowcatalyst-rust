//! Phase-aware connection timeouts for FlowCatalyst's HTTP listeners (owner
//! ruling 10, 2026-09-25; Java 3f176222 `KeepAliveIdle`), shared by the
//! platform listeners (`fc-server`, `fc-platform-server`, `fc-dev`) and the
//! function host's private and public listeners.
//!
//! The rule is phase-aware, with fixed values:
//!
//! - **Between requests** a keep-alive connection closes [`KEEP_ALIVE_IDLE`]
//!   (75 s) after its last request ended — its response fully sent. 75 s is
//!   above the ALB's 60 s idle timeout, so the balancer never races a
//!   closing backend (a backend that closes first turns into intermittent
//!   502s). Requests reset the timer, bytes do not. A new connection that
//!   has sent nothing is idle from the moment it is accepted.
//! - **Reading a request**, its headers and body must arrive within
//!   [`REQUEST_READ`] (30 s) of its first byte:
//!   - headers that don't complete in time close the connection (HTTP/1;
//!     there is no request to answer yet), as hyper's own
//!     `header_read_timeout` does. hyper's is not used: it also runs while
//!     a keep-alive connection waits for its next request, and would close
//!     every idle connection at 30 s;
//!   - a body that doesn't arrive in time answers `408` and closes the
//!     connection (its unread rest can't be skipped);
//!   - a route that takes large uploads (the platform's function artifacts,
//!     up to 256 MiB) reads its body against a **stall** deadline instead:
//!     30 s without a byte, not 30 s in total, so a slow but steady upload
//!     is never cut (Java's streaming exchanges do the same).
//! - **While the handler runs, and while a response streams**, nothing
//!   fires: a long invocation working silently or an SSE stream between
//!   events is never cut.
//!
//! HTTP/2 connections (prior knowledge) get the idle rule and the body
//! deadline; their headers arrive in frames among PINGs and SETTINGS, so
//! bytes say nothing about a request's phase there.
//!
//! Unlike Java's (Vert.x surfaces a connection only once its first request's
//! headers are in), this sees a connection from accept, so a client that
//! connects and dribbles its first headers is closed too.

use std::convert::Infallible;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use http::{HeaderValue, Method, Request, Response, StatusCode, Uri, Version};
use http_body::{Body, Frame, SizeHint};
use http_body_util::{Either, Full};
use hyper::body::Incoming;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;
use tokio::time::{Instant, Sleep};

/// A keep-alive connection with no request in flight closes this long after
/// its last request ended. Fixed; above the ALB's 60 s.
pub const KEEP_ALIVE_IDLE: Duration = Duration::from_secs(75);

/// A request's headers and body must be read within this of its first byte
/// (a streamed upload: without a byte for this long). Fixed.
pub const REQUEST_READ: Duration = Duration::from_secs(30);

/// The remote address of the connection a request arrived on, inserted into
/// every request's extensions (the platform's client-IP fallback when no
/// `X-Forwarded-For` is present, as Go's `ratelimit.ClientIP` falls back to
/// `RemoteAddr`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeerAddr(pub std::net::SocketAddr);

/// Boxed error, as hyper and tower use.
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// The body a wrapped service sees: the connection's, read against the
/// request's deadline.
pub type RequestBody = DeadlineBody<Incoming>;

/// The timeouts of one listener. [`ListenerTimeouts::new`] has the ruling's
/// values; the durations are fields only so tests can shorten them.
#[derive(Clone, Debug)]
pub struct ListenerTimeouts {
    pub keep_alive_idle: Duration,
    pub request_read: Duration,
    /// Requests whose body is read against a stall deadline (no byte for
    /// `request_read`) instead of a total one.
    pub streamed_upload: fn(&Method, &Uri) -> bool,
    /// The `408` body, in the listener's own error shape (JSON).
    pub timeout_body: &'static str,
}

impl ListenerTimeouts {
    /// The ruling's timeouts; `timeout_body` is the `408` answer's JSON.
    pub fn new(timeout_body: &'static str) -> Self {
        Self {
            keep_alive_idle: KEEP_ALIVE_IDLE,
            request_read: REQUEST_READ,
            streamed_upload: |_, _| false,
            timeout_body,
        }
    }

    /// Bodies of requests `f` picks are read against a stall deadline.
    pub fn with_streamed_uploads(mut self, f: fn(&Method, &Uri) -> bool) -> Self {
        self.streamed_upload = f;
        self
    }
}

/// Accept connections on `listener` and serve each with `service` (any tower
/// service, e.g. an axum `Router`) under `timeouts`, until `shutdown`
/// completes. Then stop accepting, let every connection finish its
/// in-flight requests (keep-alive connections close at once), and return
/// when they have. Used by the platform binaries in place of `axum::serve`.
pub async fn serve<S, B>(
    listener: TcpListener,
    service: S,
    timeouts: ListenerTimeouts,
    shutdown: impl Future<Output = ()> + Send,
) where
    S: tower_service::Service<Request<RequestBody>, Response = Response<B>, Error = Infallible>
        + Clone
        + Send
        + Sync
        + 'static,
    S::Future: Send + 'static,
    B: Body<Data = Bytes> + Send + 'static,
    B::Error: Into<BoxError>,
{
    let timeouts = Arc::new(timeouts);
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let mut connections = tokio::task::JoinSet::new();
    let mut shutdown = std::pin::pin!(shutdown);
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, peer) = match accepted {
                    Ok(accepted) => accepted,
                    Err(e) => {
                        // EMFILE and friends: back off, keep serving.
                        tracing::warn!(error = %e, "accepting a connection failed");
                        tokio::time::sleep(Duration::from_millis(10)).await;
                        continue;
                    }
                };
                let _ = stream.set_nodelay(true);
                let service = hyper_util::service::TowerToHyperService::new(service.clone());
                let timeouts = timeouts.clone();
                let mut stop = stop_rx.clone();
                connections.spawn(async move {
                    let stopped = async move {
                        let _ = stop.wait_for(|s| *s).await;
                    };
                    if let Err(e) = serve_connection(stream, service, &timeouts, stopped).await {
                        tracing::debug!(error = %e, %peer, "connection ended with an error");
                    }
                });
            }
            Some(_) = connections.join_next(), if !connections.is_empty() => {}
            _ = &mut shutdown => break,
        }
    }
    drop(listener);
    let _ = stop_tx.send(true);
    while connections.join_next().await.is_some() {}
}

/// Serve one accepted connection under `timeouts` (HTTP/1.1, or HTTP/2 by
/// prior knowledge). `shutdown` completing shuts the connection down
/// gracefully: in-flight requests finish, then it closes. Returns when the
/// connection is closed.
pub async fn serve_connection<S, B>(
    stream: TcpStream,
    service: S,
    timeouts: &ListenerTimeouts,
    shutdown: impl Future<Output = ()>,
) -> Result<(), BoxError>
where
    S: hyper::service::Service<Request<RequestBody>, Response = Response<B>>
        + Send
        + Sync
        + 'static,
    S::Future: Send + 'static,
    S::Error: Into<BoxError>,
    B: Body<Data = Bytes> + Send + 'static,
    B::Error: Into<BoxError>,
{
    let conn = Arc::new(ConnState::new());
    let peer = stream.peer_addr().ok().map(PeerAddr);
    let io = TokioIo::new(PhaseIo {
        inner: stream,
        conn: conn.clone(),
    });
    let service = Timed {
        inner: Arc::new(service),
        conn: conn.clone(),
        timeouts: timeouts.clone(),
        peer,
    };
    let builder = auto::Builder::new(TokioExecutor::new());
    let connection = builder.serve_connection(io, service);
    let mut connection = std::pin::pin!(connection);
    let mut shutdown = std::pin::pin!(shutdown);
    // Once closing: when the graceful close began. A client that never
    // completes it (an HTTP/2 peer that ignores GOAWAY's PING) is dropped
    // after the read time with nothing in flight.
    let mut closing: Option<Instant> = None;
    loop {
        tokio::select! {
            result = connection.as_mut() => return result,
            _ = &mut shutdown, if closing.is_none() => {
                connection.as_mut().graceful_shutdown();
                closing = Some(Instant::now());
            }
            action = conn.next_action(timeouts), if closing.is_none() => match action {
                Action::Idle => {
                    tracing::trace!("closing an idle keep-alive connection");
                    connection.as_mut().graceful_shutdown();
                    closing = Some(Instant::now());
                }
                Action::HeadersTimedOut => {
                    tracing::debug!("request headers not received in time; closing the connection");
                    return Ok(());
                }
            },
            _ = conn.quiet_for(closing.unwrap_or_else(Instant::now), timeouts.request_read),
                if closing.is_some() => {
                tracing::debug!("a closing connection did not close; dropping it");
                return Ok(());
            }
        }
    }
}

// ── Connection phase ────────────────────────────────────────────────────────

/// What a connection's timer decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// No request in flight for the idle time: close it gracefully.
    Idle,
    /// A request's headers didn't arrive within the read time: drop it.
    HeadersTimedOut,
}

struct Phase {
    in_flight: usize,
    /// When the last request ended (or the connection was accepted).
    idle_since: Instant,
    /// When the first byte of the next request arrived, until the request
    /// reaches the service.
    head_started: Option<Instant>,
    /// Decided by the connection's first bytes: the HTTP/2 preface.
    h2: Option<bool>,
}

struct ConnState {
    phase: Mutex<Phase>,
    changed: Notify,
}

impl ConnState {
    fn new() -> Self {
        Self {
            phase: Mutex::new(Phase {
                in_flight: 0,
                idle_since: Instant::now(),
                head_started: None,
                h2: None,
            }),
            changed: Notify::new(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Phase> {
        self.phase.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Bytes arrived. With nothing in flight they start a request's header
    /// deadline (HTTP/1 only).
    fn on_bytes(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let mut phase = self.lock();
        let h2 = *phase.h2.get_or_insert_with(|| bytes.starts_with(b"PRI "));
        if !h2 && phase.in_flight == 0 && phase.head_started.is_none() {
            phase.head_started = Some(Instant::now());
            drop(phase);
            self.changed.notify_one();
        }
    }

    /// A request reached the service; when its first byte arrived.
    fn on_request(&self) -> Instant {
        let mut phase = self.lock();
        phase.in_flight += 1;
        let started = phase.head_started.take().unwrap_or_else(Instant::now);
        drop(phase);
        self.changed.notify_one();
        started
    }

    /// A request ended: its response was sent, or abandoned.
    fn on_request_end(&self) {
        let mut phase = self.lock();
        phase.in_flight = phase.in_flight.saturating_sub(1);
        if phase.in_flight == 0 {
            phase.idle_since = Instant::now();
        }
        drop(phase);
        self.changed.notify_one();
    }

    /// The pending deadline, if any: none while a request is in flight.
    fn deadline(&self, timeouts: &ListenerTimeouts) -> Option<(Instant, Action)> {
        let phase = self.lock();
        if phase.in_flight > 0 {
            None
        } else if let Some(started) = phase.head_started {
            Some((started + timeouts.request_read, Action::HeadersTimedOut))
        } else {
            Some((phase.idle_since + timeouts.keep_alive_idle, Action::Idle))
        }
    }

    /// Resolves once no request has been in flight for `quiet`, counted
    /// from `since` at the earliest.
    async fn quiet_for(&self, since: Instant, quiet: Duration) {
        loop {
            let changed = self.changed.notified();
            let mut changed = std::pin::pin!(changed);
            changed.as_mut().enable();
            let at = {
                let phase = self.lock();
                (phase.in_flight == 0).then(|| phase.idle_since.max(since) + quiet)
            };
            match at {
                None => changed.await,
                Some(at) if at <= Instant::now() => return,
                Some(at) => {
                    tokio::select! {
                        _ = tokio::time::sleep_until(at) => {}
                        _ = changed => {}
                    }
                }
            }
        }
    }

    /// Resolves when a deadline passes in the phase it was set for.
    async fn next_action(&self, timeouts: &ListenerTimeouts) -> Action {
        loop {
            let changed = self.changed.notified();
            let mut changed = std::pin::pin!(changed);
            changed.as_mut().enable();
            match self.deadline(timeouts) {
                None => changed.await,
                Some((at, action)) if at <= Instant::now() => return action,
                Some((at, _)) => {
                    tokio::select! {
                        _ = tokio::time::sleep_until(at) => {}
                        _ = changed => {}
                    }
                }
            }
        }
    }
}

/// The connection's socket, watched for the first byte of each request.
struct PhaseIo {
    inner: TcpStream,
    conn: Arc<ConnState>,
}

impl AsyncRead for PhaseIo {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        let poll = Pin::new(&mut this.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = poll {
            this.conn.on_bytes(&buf.filled()[before..]);
        }
        poll
    }
}

impl AsyncWrite for PhaseIo {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

/// Counts a request in flight until dropped: with the response body, once
/// hyper has sent it (or the connection went away); with the service's
/// future, if the request is abandoned first.
struct InFlight(Arc<ConnState>);

impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.on_request_end();
    }
}

// ── The service wrapper ─────────────────────────────────────────────────────

/// The response body: the service's, holding its request in flight until
/// sent, or the `408`.
pub type ResponseBody<B> = Either<Tracked<B>, Full<Bytes>>;

struct Timed<S> {
    inner: Arc<S>,
    conn: Arc<ConnState>,
    timeouts: ListenerTimeouts,
    peer: Option<PeerAddr>,
}

impl<S, B> hyper::service::Service<Request<Incoming>> for Timed<S>
where
    S: hyper::service::Service<Request<RequestBody>, Response = Response<B>>
        + Send
        + Sync
        + 'static,
    S::Future: Send + 'static,
    B: Body<Data = Bytes> + Send + 'static,
{
    type Response = Response<ResponseBody<B>>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, S::Error>> + Send>>;

    fn call(&self, request: Request<Incoming>) -> Self::Future {
        let started = self.conn.on_request();
        let in_flight = InFlight(self.conn.clone());
        let (mut parts, body) = request.into_parts();
        if let Some(peer) = self.peer {
            parts.extensions.insert(peer);
        }
        let window = self.timeouts.request_read;
        let body = if (self.timeouts.streamed_upload)(&parts.method, &parts.uri) {
            DeadlineBody::stall(body, window)
        } else {
            DeadlineBody::total(body, started + window)
        };
        let timed_out = body.timed_out.clone();
        let http1 = parts.version < Version::HTTP_2;
        let timeout_body = self.timeouts.timeout_body;
        let future = self.inner.call(Request::from_parts(parts, body));
        Box::pin(async move {
            let response = future.await?;
            if timed_out.load(Ordering::SeqCst) {
                drop(response);
                return Ok(request_timeout(timeout_body, http1));
            }
            Ok(response.map(|body| {
                Either::Left(Tracked {
                    inner: body,
                    _in_flight: in_flight,
                })
            }))
        })
    }
}

/// `408`, whatever the handler answered: its body read failed on the
/// deadline. HTTP/1 closes the connection (the rest of the body is unread).
fn request_timeout<B>(body: &'static str, http1: bool) -> Response<ResponseBody<B>> {
    let mut response = Response::new(Either::Right(Full::new(Bytes::from_static(
        body.as_bytes(),
    ))));
    *response.status_mut() = StatusCode::REQUEST_TIMEOUT;
    let headers = response.headers_mut();
    headers.insert(
        http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    if http1 {
        headers.insert(http::header::CONNECTION, HeaderValue::from_static("close"));
    }
    response
}

pin_project_lite::pin_project! {
    /// A response body that keeps its request in flight until it is sent.
    pub struct Tracked<B> {
        #[pin]
        inner: B,
        _in_flight: InFlight,
    }
}

impl<B> Body for Tracked<B>
where
    B: Body<Data = Bytes>,
{
    type Data = Bytes;
    type Error = B::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, B::Error>>> {
        self.project().inner.poll_frame(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

// ── The request body's deadline ─────────────────────────────────────────────

/// Why a request body read failed.
#[derive(Debug)]
pub enum BodyReadError {
    /// It didn't arrive within the request's read deadline.
    TimedOut,
    /// The connection's own error.
    Connection(BoxError),
}

impl std::fmt::Display for BodyReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TimedOut => f.write_str("the request body was not received in time"),
            Self::Connection(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for BodyReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::TimedOut => None,
            Self::Connection(e) => Some(e.as_ref()),
        }
    }
}

enum Deadline {
    /// The whole body by this instant.
    Total,
    /// A byte at least this often.
    Stall(Duration),
}

/// A request body read against a deadline, checked only while the body is
/// awaited: a handler that is busy, or never reads, is never cut.
pub struct DeadlineBody<B> {
    inner: B,
    sleep: Pin<Box<Sleep>>,
    deadline: Deadline,
    timed_out: Arc<AtomicBool>,
    done: bool,
}

impl<B> DeadlineBody<B> {
    /// Every byte by `at`.
    pub fn total(inner: B, at: Instant) -> Self {
        Self::new(inner, at, Deadline::Total)
    }

    /// No gap between bytes longer than `window`.
    pub fn stall(inner: B, window: Duration) -> Self {
        Self::new(inner, Instant::now() + window, Deadline::Stall(window))
    }

    fn new(inner: B, at: Instant, deadline: Deadline) -> Self {
        Self {
            inner,
            sleep: Box::pin(tokio::time::sleep_until(at)),
            deadline,
            timed_out: Arc::new(AtomicBool::new(false)),
            done: false,
        }
    }
}

impl<B> Body for DeadlineBody<B>
where
    B: Body<Data = Bytes> + Unpin,
    B::Error: Into<BoxError>,
{
    type Data = Bytes;
    type Error = BodyReadError;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BodyReadError>>> {
        let this = self.get_mut();
        if this.done {
            return Poll::Ready(None);
        }
        match Pin::new(&mut this.inner).poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) => {
                if let Deadline::Stall(window) = this.deadline {
                    this.sleep.as_mut().reset(Instant::now() + window);
                }
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Ready(Some(Err(e))) => {
                Poll::Ready(Some(Err(BodyReadError::Connection(e.into()))))
            }
            Poll::Ready(None) => {
                this.done = true;
                Poll::Ready(None)
            }
            Poll::Pending => match this.sleep.as_mut().poll(cx) {
                Poll::Ready(()) => {
                    this.done = true;
                    this.timed_out.store(true, Ordering::SeqCst);
                    Poll::Ready(Some(Err(BodyReadError::TimedOut)))
                }
                Poll::Pending => Poll::Pending,
            },
        }
    }

    fn is_end_stream(&self) -> bool {
        self.done || self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

#[cfg(test)]
mod tests;
