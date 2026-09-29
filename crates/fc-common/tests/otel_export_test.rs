//! With the `otel` feature and `FC_OTEL_ENABLED=true`, spans reach an
//! OTLP/HTTP collector. Its own test binary: logging init is process-wide.
#![cfg(feature = "otel")]

use fc_common::logging;
use std::env;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spans_are_exported_over_otlp_http() {
    // A collector that answers 200 to anything and counts trace posts.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let posts = Arc::new(AtomicUsize::new(0));
    let seen = posts.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let seen = seen.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 64 * 1024];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]);
                if head.starts_with("POST /v1/traces") {
                    seen.fetch_add(1, Ordering::SeqCst);
                }
                let _ = sock
                    .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
                    .await;
            });
        }
    });

    env::set_var("FC_OTEL_ENABLED", "true");
    // Keep every span: production defaults to one trace in a thousand.
    env::set_var("FC_OTEL_SAMPLE_RATIO", "1.0");
    env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", format!("http://{addr}"));
    logging::init_production_logging("otel-test");

    tracing::info_span!("router.dispatch", message_id = "m-1").in_scope(|| {
        tracing::info!("inside");
    });

    // Flush from a blocking thread so the runtime stays free to send.
    task::spawn_blocking(logging::shutdown).await.unwrap();
    assert!(posts.load(Ordering::SeqCst) >= 1, "no OTLP export arrived");
}
