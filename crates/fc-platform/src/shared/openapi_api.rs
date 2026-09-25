//! `GET /api/openapi.json` and `GET /api/openapi.yaml` (Go
//! `internal/server/wire_spec.go:16,24`): the platform's OpenAPI document,
//! unauthenticated. Rust serves its own document (the one at `/q/openapi`,
//! `/bff/*` stripped), serialised once at boot.

use axum::{
    body::Bytes, extract::State, http::header::CONTENT_TYPE, response::IntoResponse, routing::get,
    Router,
};

/// The document, serialised once.
#[derive(Clone)]
struct Specs {
    json: Bytes,
    yaml: Bytes,
}

async fn openapi_json(State(specs): State<Specs>) -> impl IntoResponse {
    ([(CONTENT_TYPE, "application/json")], specs.json)
}

async fn openapi_yaml(State(specs): State<Specs>) -> impl IntoResponse {
    ([(CONTENT_TYPE, "application/yaml")], specs.yaml)
}

/// The two spec routes for `openapi`.
pub fn openapi_router(openapi: &utoipa::openapi::OpenApi) -> Router {
    let specs = Specs {
        json: serde_json::to_vec(openapi)
            .map(Bytes::from)
            .unwrap_or_default(),
        yaml: openapi.to_yaml().map(Bytes::from).unwrap_or_default(),
    };
    Router::new()
        .route("/api/openapi.json", get(openapi_json))
        .route("/api/openapi.yaml", get(openapi_yaml))
        .with_state(specs)
}
