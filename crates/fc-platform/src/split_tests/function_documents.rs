//! The function documents' routes (`function::routes`, the assembly's):
//! the OpenAPI document and the manifest schema, served without a token,
//! and the Rust document registering Java's operations
//! (`function::openapi`, `function::schema`, fc-platform-functions).

use crate::function::openapi::{FUNCTIONS_OPENAPI, PATH_FUNCTIONS_OPENAPI};
use crate::function::schema::{FUNCTION_MANIFEST_SCHEMA, PATH_FUNCTION_MANIFEST_SCHEMA};
use axum::body::Body;
use axum::http::header::CONTENT_TYPE;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

#[tokio::test]
async fn serves_the_document_bytes_without_a_token() {
    let response = crate::function::routes::functions_openapi_router::<()>()
        .oneshot(
            Request::get(PATH_FUNCTIONS_OPENAPI)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[CONTENT_TYPE], "application/json");
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], FUNCTIONS_OPENAPI);
}

#[tokio::test]
async fn serves_the_schema_bytes_without_a_token() {
    let response = crate::function::routes::function_manifest_schema_router::<()>()
        .oneshot(
            Request::get(PATH_FUNCTION_MANIFEST_SCHEMA)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[CONTENT_TYPE], "application/schema+json");
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body[..], FUNCTION_MANIFEST_SCHEMA);
}

/// The routes Rust registers (in its own utoipa document) are exactly
/// the `/api/` operations of Java's document.
#[test]
fn rust_registers_javas_operations() {
    use std::collections::BTreeSet;
    let java: serde_json::Value = serde_json::from_slice(FUNCTIONS_OPENAPI).unwrap();
    let java: BTreeSet<(String, String)> = java["paths"]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(path, _)| path.starts_with("/api/"))
        .flat_map(|(path, ops)| {
            ops.as_object()
                .unwrap()
                .keys()
                .map(move |m| (path.clone(), m.clone()))
        })
        .collect();

    let (_, rust) = crate::function::routes::function_routes().split_for_parts();
    let rust: BTreeSet<(String, String)> = serde_json::to_value(&rust).unwrap()["paths"]
        .as_object()
        .unwrap()
        .iter()
        .flat_map(|(path, ops)| {
            ops.as_object()
                .unwrap()
                .keys()
                .filter(|k| !k.starts_with("parameters"))
                .map(move |m| (path.clone(), m.clone()))
        })
        .collect();
    assert_eq!(rust, java);
}
