//! `/api/test/*` and `/api/benchmark/*` mock endpoints (development only).

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

// Global test stats counter
static TEST_REQUEST_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Test fast endpoint (100ms delay)
#[utoipa::path(
    post,
    path = "/api/test/fast",
    tag = "test",
    responses(
        (status = 200, description = "Fast response")
    )
)]
pub(crate) async fn test_fast() -> Json<serde_json::Value> {
    TEST_REQUEST_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    Json(serde_json::json!({ "status": "success", "ack": true }))
}

/// Test slow endpoint (60s delay)
#[utoipa::path(
    post,
    path = "/api/test/slow",
    tag = "test",
    responses(
        (status = 200, description = "Slow response")
    )
)]
pub(crate) async fn test_slow() -> Json<serde_json::Value> {
    TEST_REQUEST_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
    Json(serde_json::json!({ "status": "success", "ack": true }))
}

/// Test faulty endpoint (random responses)
#[utoipa::path(
    post,
    path = "/api/test/faulty",
    tag = "test",
    responses(
        (status = 200, description = "Success response"),
        (status = 400, description = "Client error"),
        (status = 500, description = "Server error")
    )
)]
pub(crate) async fn test_faulty() -> Response {
    use rand::Rng;

    TEST_REQUEST_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut rng = rand::rng();
    let roll: f64 = rng.random();

    if roll < 0.6 {
        // 60% success
        (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "success", "ack": true })),
        )
            .into_response()
    } else if roll < 0.8 {
        // 20% 400 error
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "status": "error", "error": "Client error" })),
        )
            .into_response()
    } else {
        // 20% 500 error
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "status": "error", "error": "Server error" })),
        )
            .into_response()
    }
}

/// Test fail endpoint (always 500)
#[utoipa::path(
    post,
    path = "/api/test/fail",
    tag = "test",
    responses(
        (status = 500, description = "Always fails")
    )
)]
pub(crate) async fn test_fail() -> Response {
    TEST_REQUEST_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "status": "error", "error": "Always fails" })),
    )
        .into_response()
}

/// Test success endpoint (always 200 with ack=true)
#[utoipa::path(
    post,
    path = "/api/test/success",
    tag = "test",
    responses(
        (status = 200, description = "Always succeeds")
    )
)]
pub(crate) async fn test_success() -> Json<serde_json::Value> {
    TEST_REQUEST_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Json(serde_json::json!({ "ack": true, "message": "" }))
}

/// Test pending endpoint (ack=false)
#[utoipa::path(
    post,
    path = "/api/test/pending",
    tag = "test",
    responses(
        (status = 200, description = "Returns pending")
    )
)]
pub(crate) async fn test_pending() -> Json<serde_json::Value> {
    TEST_REQUEST_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Json(serde_json::json!({ "ack": false, "message": "notBefore time not reached" }))
}

/// Test client error endpoint (always 400)
#[utoipa::path(
    post,
    path = "/api/test/client-error",
    tag = "test",
    responses(
        (status = 400, description = "Client error")
    )
)]
pub(crate) async fn test_client_error() -> Response {
    TEST_REQUEST_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "status": "error", "error": "Record not found" })),
    )
        .into_response()
}

/// Test server error endpoint (always 500)
#[utoipa::path(
    post,
    path = "/api/test/server-error",
    tag = "test",
    responses(
        (status = 500, description = "Server error")
    )
)]
pub(crate) async fn test_server_error() -> Response {
    TEST_REQUEST_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "status": "error", "error": "Internal server error" })),
    )
        .into_response()
}

/// Get test stats
#[utoipa::path(
    get,
    path = "/api/test/stats",
    tag = "test",
    responses(
        (status = 200, description = "Test statistics")
    )
)]
pub(crate) async fn test_stats() -> Json<serde_json::Value> {
    let count = TEST_REQUEST_COUNT.load(std::sync::atomic::Ordering::Relaxed);
    Json(serde_json::json!({ "totalRequests": count }))
}

/// Reset test stats
#[utoipa::path(
    post,
    path = "/api/test/stats/reset",
    tag = "test",
    responses(
        (status = 200, description = "Test stats reset")
    )
)]
pub(crate) async fn reset_test_stats() -> Json<serde_json::Value> {
    let previous = TEST_REQUEST_COUNT.swap(0, std::sync::atomic::Ordering::Relaxed);
    Json(serde_json::json!({ "previousCount": previous, "currentCount": 0 }))
}
