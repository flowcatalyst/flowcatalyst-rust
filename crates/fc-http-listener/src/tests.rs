//! Owner ruling 10 over real sockets, with the times shortened: idle
//! keep-alive connections close, in-flight work and streaming responses are
//! never cut, slow headers close the connection and a slow body is a 408
//! (a streamed upload only when it stalls).

use super::*;
use std::convert::Infallible;
use std::time::Duration;

use http_body_util::{BodyExt, StreamBody};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const IDLE: Duration = Duration::from_millis(400);
const READ: Duration = Duration::from_millis(400);
const TIMEOUT_BODY: &str = r#"{"error":"REQUEST_TIMEOUT"}"#;

fn timeouts() -> ListenerTimeouts {
    ListenerTimeouts {
        keep_alive_idle: IDLE,
        request_read: READ,
        streamed_upload: |method, uri| method == Method::PUT && uri.path() == "/upload",
        timeout_body: TIMEOUT_BODY,
    }
}

type TestBody = http_body_util::combinators::BoxBody<Bytes, Infallible>;

/// `/slow` answers after 1 s; `/stream` streams three chunks 300 ms apart;
/// `/echo` and `/upload` read the whole body and answer its length; anything
/// else answers `ok` at once.
async fn handle(request: Request<RequestBody>) -> Result<Response<TestBody>, Infallible> {
    let path = request.uri().path().to_string();
    let body: TestBody = match path.as_str() {
        "/slow" => {
            tokio::time::sleep(Duration::from_secs(1)).await;
            Full::new(Bytes::from_static(b"slow")).boxed()
        }
        "/stream" => {
            let (tx, rx) = tokio::sync::mpsc::channel::<Result<Frame<Bytes>, Infallible>>(1);
            tokio::spawn(async move {
                for chunk in ["a", "b", "c"] {
                    tokio::time::sleep(Duration::from_millis(300)).await;
                    let _ = tx
                        .send(Ok(Frame::data(Bytes::from_static(chunk.as_bytes()))))
                        .await;
                }
            });
            StreamBody::new(tokio_stream_from(rx)).boxed()
        }
        "/echo" | "/upload" => match request.into_body().collect().await {
            Ok(collected) => Full::new(Bytes::from(collected.to_bytes().len().to_string())).boxed(),
            Err(e) => Full::new(Bytes::from(format!("read failed: {e}"))).boxed(),
        },
        _ => Full::new(Bytes::from_static(b"ok")).boxed(),
    };
    Ok(Response::new(body))
}

/// An mpsc receiver as a stream.
fn tokio_stream_from<T>(mut rx: tokio::sync::mpsc::Receiver<T>) -> impl futures::Stream<Item = T> {
    futures::stream::poll_fn(move |cx| rx.poll_recv(cx))
}

/// A listener on a free port, serving [`handle`] under [`timeouts`].
async fn start() -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let service = hyper::service::service_fn(handle);
                let _ =
                    serve_connection(stream, service, &timeouts(), std::future::pending()).await;
            });
        }
    });
    addr
}

/// Reads until the server closes (true) or `within` passes (false),
/// returning what was read.
async fn read_until_closed(socket: &mut TcpStream, within: Duration) -> (bool, String) {
    let mut out = Vec::new();
    let mut buf = [0u8; 1024];
    let closed = tokio::time::timeout(within, async {
        loop {
            match socket.read(&mut buf).await {
                Ok(0) | Err(_) => return,
                Ok(n) => out.extend_from_slice(&buf[..n]),
            }
        }
    })
    .await
    .is_ok();
    (closed, String::from_utf8_lossy(&out).into_owned())
}

/// Reads one response whose body ends with `end`.
async fn read_response(socket: &mut TcpStream, end: &str) -> String {
    let mut out = Vec::new();
    let mut buf = [0u8; 1024];
    tokio::time::timeout(Duration::from_secs(5), async {
        while !String::from_utf8_lossy(&out).ends_with(end) {
            let n = socket.read(&mut buf).await.unwrap();
            assert!(n > 0, "closed early: {}", String::from_utf8_lossy(&out));
            out.extend_from_slice(&buf[..n]);
        }
    })
    .await
    .expect("a response in time");
    String::from_utf8_lossy(&out).into_owned()
}

#[tokio::test]
async fn an_idle_keep_alive_connection_closes_after_the_idle_time() {
    let addr = start().await;
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket
        .write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    let first = read_response(&mut socket, "ok").await;
    assert!(first.starts_with("HTTP/1.1 200"), "{first}");

    // Still open well within the idle time: a second request is served.
    tokio::time::sleep(IDLE / 2).await;
    socket
        .write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    read_response(&mut socket, "ok").await;

    // The request reset the timer; it closes once idle for IDLE.
    let (closed, _) = read_until_closed(&mut socket, IDLE * 4).await;
    assert!(closed, "the idle connection was closed");
}

#[tokio::test]
async fn a_silent_new_connection_is_idle_not_a_slow_request() {
    let addr = start().await;
    let started = Instant::now();
    let mut socket = TcpStream::connect(addr).await.unwrap();
    let (closed, read) = read_until_closed(&mut socket, IDLE * 4).await;
    assert!(closed && read.is_empty(), "{read}");
    assert!(started.elapsed() >= IDLE, "closed by the idle rule");
}

#[tokio::test]
async fn a_handler_outliving_the_idle_time_is_not_cut() {
    let addr = start().await;
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket
        .write_all(b"GET /slow HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    let response = read_response(&mut socket, "slow").await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    let (closed, _) = read_until_closed(&mut socket, IDLE * 4).await;
    assert!(closed, "then closed once idle");
}

#[tokio::test]
async fn a_streaming_response_is_not_cut() {
    let addr = start().await;
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket
        .write_all(b"GET /stream HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    // 900 ms of streaming, gaps of 300 ms: past both the idle and the read time.
    let response = read_response(&mut socket, "0\r\n\r\n").await;
    assert!(
        response.contains("\r\na\r\n") && response.contains("\r\nc\r\n"),
        "{response}"
    );
}

#[tokio::test]
async fn headers_that_do_not_arrive_in_time_close_the_connection() {
    let addr = start().await;
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket
        .write_all(b"GET / HTTP/1.1\r\nHost: x\r\n")
        .await
        .unwrap();
    let started = Instant::now();
    let (closed, read) = read_until_closed(&mut socket, IDLE * 4).await;
    assert!(
        closed && read.is_empty(),
        "closed without an answer: {read}"
    );
    assert!(started.elapsed() < IDLE + READ, "by the read deadline");
}

#[tokio::test]
async fn a_body_that_does_not_arrive_in_time_is_a_408() {
    let addr = start().await;
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket
        .write_all(b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 10\r\n\r\nabc")
        .await
        .unwrap();
    let (closed, read) = read_until_closed(&mut socket, Duration::from_secs(3)).await;
    assert!(read.starts_with("HTTP/1.1 408"), "{read}");
    assert!(read.ends_with(TIMEOUT_BODY), "{read}");
    assert!(
        read.to_ascii_lowercase().contains("connection: close"),
        "{read}"
    );
    assert!(closed, "and the connection closes");
}

#[tokio::test]
async fn a_body_read_in_time_is_served() {
    let addr = start().await;
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket
        .write_all(b"POST /echo HTTP/1.1\r\nHost: x\r\nContent-Length: 6\r\n\r\nabc")
        .await
        .unwrap();
    tokio::time::sleep(READ / 2).await;
    socket.write_all(b"def").await.unwrap();
    let response = read_response(&mut socket, "\r\n\r\n6").await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}

/// A streamed upload takes as long as it takes, as long as it never stalls
/// for the read time; one that stalls is a 408.
#[tokio::test]
async fn a_streamed_upload_is_cut_only_when_it_stalls() {
    let addr = start().await;
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket
        .write_all(b"PUT /upload HTTP/1.1\r\nHost: x\r\nContent-Length: 5\r\n\r\n")
        .await
        .unwrap();
    for _ in 0..5 {
        tokio::time::sleep(READ / 2).await; // 1 s in all: past the read time
        socket.write_all(b"x").await.unwrap();
    }
    let response = read_response(&mut socket, "\r\n\r\n5").await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");

    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket
        .write_all(b"PUT /upload HTTP/1.1\r\nHost: x\r\nContent-Length: 5\r\n\r\nxx")
        .await
        .unwrap();
    let (_, read) = read_until_closed(&mut socket, Duration::from_secs(3)).await;
    assert!(read.starts_with("HTTP/1.1 408"), "{read}");
}

#[tokio::test]
async fn shutdown_lets_the_in_flight_request_finish() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let service = hyper::service::service_fn(handle);
        serve_connection(stream, service, &timeouts(), async {
            let _ = stop_rx.await;
        })
        .await
    });
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket
        .write_all(b"GET /slow HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    stop_tx.send(()).unwrap();
    let (closed, read) = read_until_closed(&mut socket, Duration::from_secs(3)).await;
    assert!(
        read.starts_with("HTTP/1.1 200") && read.ends_with("slow"),
        "{read}"
    );
    assert!(closed);
    server.await.unwrap().unwrap();
}

/// HTTP/2 frames say nothing about a request's phase: an idle h2 connection
/// is closed by the idle rule (GOAWAY), never by the header deadline. This
/// client never acknowledges GOAWAY's PING, so the close completes by
/// dropping it a read time later.
#[tokio::test]
async fn an_idle_h2_connection_closes_by_the_idle_rule() {
    let addr = start().await;
    let started = Instant::now();
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket
        .write_all(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n\x00\x00\x00\x04\x00\x00\x00\x00\x00")
        .await
        .unwrap();
    let (closed, _) = read_until_closed(&mut socket, (IDLE + READ) * 3).await;
    assert!(closed, "the idle h2 connection was closed");
    assert!(
        started.elapsed() >= IDLE,
        "by the idle rule, not the read deadline"
    );
}
