//! `GET /api/openapi.json` and `GET /api/openapi.yaml` (Go
//! `internal/server/wire_spec.go:16,24`): the platform's OpenAPI document,
//! unauthenticated. Rust serves its own document (the one at `/q/openapi`,
//! `/bff/*` stripped), serialised once at boot.

use axum::{body::Bytes, http::header::CONTENT_TYPE, routing::get, Router};

/// The two spec routes for `openapi`.
pub fn openapi_router(openapi: &utoipa::openapi::OpenApi) -> Router {
    let json: Bytes = serde_json::to_vec(openapi)
        .map(Bytes::from)
        .unwrap_or_default();
    let yaml: Bytes = openapi.to_yaml().map(Bytes::from).unwrap_or_default();
    Router::new()
        .route(
            "/api/openapi.json",
            get(move || {
                let body = json.clone();
                async move { ([(CONTENT_TYPE, "application/json")], body) }
            }),
        )
        .route(
            "/api/openapi.yaml",
            get(move || {
                let body = yaml.clone();
                async move { ([(CONTENT_TYPE, "application/yaml")], body) }
            }),
        )
}
