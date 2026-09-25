//! The function API's wire contract, served verbatim at
//! `GET /api/openapi-functions.json` (Java
//! `shared/openapi/FunctionOpenApiRoutes.java`, spec `function-openapi.md` §2).
//!
//! The file began as a byte-identical copy of Java's
//! `server/src/main/resources/openapi/functions.openapi.json` at `0118cdca`.
//! Owner decision 5 (2026-09-25) improves the contract in Rust,
//! backward-compatibly, so it is now a superset of Java's: the same
//! operations and every schema property Java has, plus additive fields and
//! answers (no-op writes answering `200`, promote's `expectedVersion`, the
//! `component` runtime, the error envelope's `code`).
//! The route is unauthenticated, as in Java: tooling fetches the contract
//! without a token. It is not under `/api/functions/…`, whose next segment
//! is always a function address.
//!
//! The same routes are also registered in the platform's own utoipa
//! document (`/q/openapi`), generated from the Rust handlers.

use axum::http::header::CONTENT_TYPE;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;

/// The document's bytes.
pub const FUNCTIONS_OPENAPI: &[u8] =
    include_bytes!("../../resources/openapi/functions.openapi.json");

/// Where the document is served.
pub const PATH_FUNCTIONS_OPENAPI: &str = "/api/openapi-functions.json";

async fn functions_openapi() -> impl IntoResponse {
    ([(CONTENT_TYPE, "application/json")], FUNCTIONS_OPENAPI)
}

/// The unauthenticated route, to merge into the platform router.
pub fn functions_openapi_router<S: Clone + Send + Sync + 'static>() -> Router<S> {
    Router::new().route(PATH_FUNCTIONS_OPENAPI, get(functions_openapi))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn serves_the_document_bytes_without_a_token() {
        let response = functions_openapi_router::<()>()
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

    /// The document extends Java's, when the Java checkout is next to this
    /// repo: every path and method Java has, every schema, and every
    /// property of every schema (a Java client's view of the wire still
    /// holds); only additions differ.
    #[test]
    fn extends_javas_file() {
        let java = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../../flowcatalyst-javalin/server/src/main/resources/openapi/functions.openapi.json",
        );
        let Ok(bytes) = std::fs::read(&java) else {
            eprintln!("skipped: {} not found", java.display());
            return;
        };
        let java: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let rust: serde_json::Value = serde_json::from_slice(FUNCTIONS_OPENAPI).unwrap();
        for (path, ops) in java["paths"].as_object().unwrap() {
            for (method, op) in ops.as_object().unwrap() {
                let ours = &rust["paths"][path][method];
                assert!(!ours.is_null(), "{method} {path} missing");
                for status in op["responses"]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .map(|r| r.0)
                {
                    assert!(
                        !ours["responses"][status].is_null(),
                        "{method} {path} lost its {status}"
                    );
                }
            }
        }
        for (name, schema) in java["components"]["schemas"].as_object().unwrap() {
            let ours = &rust["components"]["schemas"][name];
            assert!(!ours.is_null(), "schema {name} missing");
            for (property, _) in schema["properties"].as_object().into_iter().flatten() {
                assert!(
                    !ours["properties"][property].is_null(),
                    "{name}.{property} missing"
                );
            }
        }
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

        let (_, rust) = crate::function::api::function_routes().split_for_parts();
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
}
