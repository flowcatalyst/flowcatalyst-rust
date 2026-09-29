//! The metrics port's diagnostics (`FC_METRICS_PORT`, 9090): whatever roles
//! this process runs, an operator can see its runtime and take a task dump
//! here — a platform-only or worker node has no router API to ask.
//!
//! - `GET /metrics`: the process's Prometheus registry (router, scheduler,
//!   stream series) plus the `tokio_runtime_*`, `process_*` and panic
//!   series. Open, like the rest of the port (scrapers).
//! - `GET /diagnostics/runtime?sampleMs=`: the runtime report as JSON.
//! - `GET /diagnostics/task-dump?timeoutMs=`: every task's async backtrace.
//!
//! The two diagnostics routes take the platform bearer token the router API
//! takes (owner ruling 2): `platform:messaging:router:view` for the report,
//! `…:router:operate` for the dump. Tokens are verified against
//! `FC_DIAGNOSTICS_PLATFORM_URL`, else `FC_ROUTER_PLATFORM_URL`, else the
//! platform in this process; with none of those every diagnostics call is
//! 401 (never anonymous). Operator guide:
//! `docs/operations/diagnosing-stuck-processes.md`.

use std::time::Duration;

use axum::middleware;
use axum::{
    extract::Query,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use fc_common::diagnostics;
use fc_common::diagnostics::Exposition;
use fc_common::diagnostics::RuntimeReport;
use fc_router::api::diagnostics::task_dump_response;
use fc_router::api::platform_auth::{platform_auth_middleware, PlatformAuth};
use serde::Deserialize;
use std::env;

/// The platform to verify diagnostics tokens against.
pub fn platform_url(platform_enabled: bool, api_port: u16) -> Option<String> {
    ["FC_DIAGNOSTICS_PLATFORM_URL", "FC_ROUTER_PLATFORM_URL"]
        .iter()
        .find_map(|k| env::var(k).ok().filter(|v| !v.trim().is_empty()))
        .or_else(|| platform_enabled.then(|| format!("http://127.0.0.1:{api_port}")))
}

/// `/diagnostics/*`, behind the platform bearer.
pub fn routes(platform_url: Option<&str>) -> Router {
    let guard = PlatformAuth::new(platform_url);
    Router::new()
        .route("/diagnostics/runtime", get(runtime))
        .route("/diagnostics/task-dump", get(task_dump))
        .layer(middleware::from_fn_with_state(
            guard,
            platform_auth_middleware,
        ))
}

/// The `/metrics` body: the registry, then the runtime and process series,
/// then Go's `fc_server_up`.
pub fn metrics_text(handle: &metrics_exporter_prometheus::PrometheusHandle) -> Response {
    let mut out = handle.render();
    diagnostics::render_prometheus(&mut out, None, Exposition::Prometheus);
    out.push_str("# HELP fc_server_up Server is up\n# TYPE fc_server_up gauge\nfc_server_up 1\n");
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        out,
    )
        .into_response()
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RuntimeQuery {
    sample_ms: Option<u64>,
}

async fn runtime(Query(q): Query<RuntimeQuery>) -> Json<RuntimeReport> {
    let window = Duration::from_millis(q.sample_ms.unwrap_or(1000)).min(Duration::from_secs(10));
    Json(diagnostics::report(None, window).await)
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct DumpQuery {
    timeout_ms: Option<u64>,
}

async fn task_dump(Query(q): Query<DumpQuery>) -> Response {
    let timeout = Duration::from_millis(q.timeout_ms.unwrap_or(5000)).min(Duration::from_secs(30));
    task_dump_response(timeout).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    #[tokio::test]
    async fn diagnostics_are_never_anonymous() {
        // No platform to verify against: fail closed.
        let app = routes(None);
        for path in ["/diagnostics/runtime", "/diagnostics/task-dump"] {
            let resp = app
                .clone()
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "{path}");
        }
    }

    #[tokio::test]
    async fn metrics_carry_the_runtime_series() {
        let handle = fc_router::init_prometheus_recorder();
        let resp = metrics_text(&handle);
        let body = body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains("tokio_runtime_workers "), "{text}");
        assert!(text.contains("fc_server_up 1"));
    }
}
