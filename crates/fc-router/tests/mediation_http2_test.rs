//! Item 3 (owner ruling 2026-09-07; Go and Java both already do this):
//! deployed mode must speak h2c (prior-knowledge, cleartext) to a plain
//! `http://` target and ALPN h2 to an `https://` one — never silently
//! fall back to HTTP/1.1 the way this mediator did before the fix in
//! `mediator/inner.rs::make_client_builder`. `wiremock`'s `MockServer`
//! only ever speaks HTTP/1.1, so it can't tell these two shapes apart —
//! this file runs raw `hyper`/`hyper-util` servers instead: one that
//! auto-negotiates h1/h2c (`hyper_util::server::conn::auto`, what a real
//! h2c-capable target looks like), and one that speaks ONLY HTTP/1.1
//! (`hyper::server::conn::http1` directly — an h2c connection preface
//! sent at it is nonsense, not something it can downgrade its way out of).

use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

use fc_common::{DispatchMode, MediationResult, MediationType, Message};
use fc_router::{HttpMediator, HttpMediatorConfig, Mediator};

/// What the test server captured about the one request it received.
#[derive(Debug, Clone)]
struct CapturedRequest {
    version: hyper::Version,
    body: String,
}

fn healthy_message(target: &str) -> Message {
    Message {
        id: "item3-msg".to_string(),
        pool_code: "TEST".to_string(),
        auth_token: None,
        signing_secret: None,
        mediation_type: MediationType::HTTP,
        mediation_target: target.to_string(),
        message_group_id: None,
        high_priority: false,
        dispatch_mode: DispatchMode::default(),
        dispatch_mode_specified: true,
    }
}

/// Starts an h2c-capable server (auto h1/h2c negotiation off one plain TCP
/// listener — no TLS involved, matching what a real production target
/// speaking h2c looks like on the wire) on an ephemeral localhost port.
/// Returns the base URL and a receiver that yields the one request it
/// captures.
async fn start_h2c_server() -> (String, tokio::sync::oneshot::Receiver<CapturedRequest>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind h2c listener");
    let addr = listener.local_addr().expect("local addr");
    let (tx, rx) = tokio::sync::oneshot::channel();
    let tx = Arc::new(Mutex::new(Some(tx)));

    tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await {
            let io = TokioIo::new(stream);
            let tx = tx.clone();
            let service = hyper::service::service_fn(move |req: Request<Incoming>| {
                let tx = tx.clone();
                async move {
                    let version = req.version();
                    let body_bytes = req
                        .into_body()
                        .collect()
                        .await
                        .map(|c| c.to_bytes())
                        .unwrap_or_default();
                    let body = String::from_utf8_lossy(&body_bytes).to_string();
                    if let Some(sender) = tx.lock().unwrap().take() {
                        let _ = sender.send(CapturedRequest { version, body });
                    }
                    Ok::<_, Infallible>(Response::new(Full::new(Bytes::from(
                        r#"{"ok":true}"#,
                    ))))
                }
            });
            let builder =
                hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new());
            let _ = builder.serve_connection(io, service).await;
        }
    });

    (format!("http://127.0.0.1:{}", addr.port()), rx)
}

/// Starts a server that speaks ONLY HTTP/1.1 — no h2c support of any kind
/// — on an ephemeral localhost port.
async fn start_http1_only_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind h1 listener");
    let addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let io = TokioIo::new(stream);
                let service = hyper::service::service_fn(|_req: Request<Incoming>| async move {
                    Ok::<_, Infallible>(Response::new(Full::new(Bytes::from(
                        r#"{"ok":true}"#,
                    ))))
                });
                // `http1::Builder` only understands HTTP/1.1 — an h2c
                // connection preface sent at it is not valid h1 framing,
                // so the connection/request fails outright. That failure
                // IS the "no silent downgrade" behaviour under test.
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service)
                    .await;
            });
        }
    });

    format!("http://127.0.0.1:{}", addr.port())
}

/// Deployed mode (`HttpMediator::production()`, `HttpVersion::Http2`)
/// against a plain `http://` target that DOES speak h2c: the request must
/// negotiate HTTP/2, and the wire body must be exactly
/// `{"messageId":"<id>"}` — item 3 changes the protocol, not the payload.
///
/// Pins: (1) `req.version()` on the server side reads `HTTP_2`; (2) the
/// captured body is byte-for-byte the golden shape; (3) the mediation
/// still reports `Success`.
///
/// Mutant check (reverted `make_client_builder`'s `HttpVersion::Http2` arm
/// to never call `.http2_prior_knowledge()` for an `http://` `HostKey` —
/// i.e. the pre-item-3 behaviour — confirmed by hand while implementing
/// this fix, then restored): assertion (1) fails, `captured.version` reads
/// `HTTP_11` instead of `HTTP_2`.
#[tokio::test]
async fn deployed_mode_negotiates_h2c_against_a_cleartext_target() {
    let (target, rx) = start_h2c_server().await;
    let mediator = HttpMediator::production();

    let message = healthy_message(&format!("{target}/hook"));
    let outcome = tokio::time::timeout(Duration::from_secs(5), mediator.mediate(&message))
        .await
        .expect("mediation must not hang");

    let captured = tokio::time::timeout(Duration::from_secs(5), rx)
        .await
        .expect("server must receive a request within 5s")
        .expect("capture channel must not be dropped without sending");

    assert_eq!(
        captured.version,
        hyper::Version::HTTP_2,
        "deployed mode must negotiate HTTP/2 (h2c, prior knowledge) \
         against a plain http:// target that supports it, not fall back \
         to HTTP/1.1"
    );
    assert_eq!(
        captured.body,
        format!(r#"{{"messageId":"{}"}}"#, message.id),
        "the wire body must be unchanged by the protocol switch"
    );
    assert_eq!(
        outcome.result,
        MediationResult::Success,
        "delivery over h2c must still classify as a normal success"
    );
}

/// Deployed mode against a target that speaks ONLY HTTP/1.1 must FAIL the
/// delivery outright, not silently downgrade — deployed mode is supposed
/// to know its real targets speak h2c, and a target that doesn't is a
/// genuine configuration/reachability problem the router must surface,
/// not paper over.
///
/// `max_retries: 0` so this resolves in one attempt rather than the full
/// retry burst — the test only cares about the classification, not the
/// retry schedule (that's `mediator/retry.rs`'s own coverage).
#[tokio::test]
async fn deployed_mode_fails_against_an_http1_only_target() {
    let target = start_http1_only_server().await;
    let mediator = HttpMediator::with_config(HttpMediatorConfig {
        max_retries: 0,
        ..HttpMediatorConfig::production()
    });

    let message = healthy_message(&format!("{target}/hook"));
    let outcome = tokio::time::timeout(Duration::from_secs(5), mediator.mediate(&message))
        .await
        .expect("mediation must not hang");

    assert_ne!(
        outcome.result,
        MediationResult::Success,
        "deployed mode must not succeed against an HTTP/1.1-only target \
         — it must fail rather than silently downgrade to h1"
    );
}

/// The flip side, against the SAME kind of HTTP/1.1-only target: dev mode
/// must still succeed. This is what makes the previous test's failure
/// mean something — it proves the target itself is reachable and correct,
/// and the deployed-mode failure is specifically about protocol choice.
///
/// Mutant check (temporarily made `HttpVersion::Http1` ALSO call
/// `.http2_prior_knowledge()` in `make_client_builder` — confirmed by hand
/// while implementing this fix, then restored): this test starts failing
/// too (dev mode can no longer reach its own target), which would make
/// the deployed-mode failure test above stop being a meaningful contrast.
#[tokio::test]
async fn dev_mode_still_succeeds_against_an_http1_only_target() {
    let target = start_http1_only_server().await;
    let mediator = HttpMediator::dev();

    let message = healthy_message(&format!("{target}/hook"));
    let outcome = tokio::time::timeout(Duration::from_secs(5), mediator.mediate(&message))
        .await
        .expect("mediation must not hang");

    assert_eq!(
        outcome.result,
        MediationResult::Success,
        "dev mode (HTTP/1.1) must still succeed against an HTTP/1.1-only target"
    );
}
