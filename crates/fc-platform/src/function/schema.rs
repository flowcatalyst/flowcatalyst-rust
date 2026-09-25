//! The manifest's JSON Schema, served verbatim at
//! `GET /api/schemas/function-manifest.json` (Java
//! `shared/openapi/FunctionManifestSchemaRoutes.java`, registered at
//! `server/Platform.java:724-727`).
//!
//! The file began as a byte-identical copy of Java's
//! `server/src/main/resources/schemas/function-manifest.schema.json`; it now
//! adds `runtime: component` (owner decision 5), whose `entrypoint` is
//! optional. It is an editor aid: an author points `$schema` at it to get
//! validation while typing. The route is unauthenticated because an editor
//! has no token.

use axum::http::header::CONTENT_TYPE;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;

/// The schema's bytes.
pub const FUNCTION_MANIFEST_SCHEMA: &[u8] =
    include_bytes!("../../resources/schemas/function-manifest.schema.json");

/// Where the schema is served.
pub const PATH_FUNCTION_MANIFEST_SCHEMA: &str = "/api/schemas/function-manifest.json";

async fn function_manifest_schema() -> impl IntoResponse {
    (
        [(CONTENT_TYPE, "application/schema+json")],
        FUNCTION_MANIFEST_SCHEMA,
    )
}

/// The unauthenticated schema route, to merge into the platform router.
pub fn function_manifest_schema_router<S: Clone + Send + Sync + 'static>() -> Router<S> {
    Router::new().route(PATH_FUNCTION_MANIFEST_SCHEMA, get(function_manifest_schema))
}

/// Java `FunctionManifestSchemaTest`: the schema is an editor aid over the
/// parser's own shape, never a second definition, so it must agree with
/// [`Manifest`](super::manifest::Manifest) object by object.
#[cfg(test)]
mod drift_tests {
    use std::collections::BTreeSet;

    use serde_json::Value;

    use super::FUNCTION_MANIFEST_SCHEMA;
    use crate::function::manifest::{
        CORS_KEYS, DB_KEYS, ENDPOINT_KEYS, LIMITS_KEYS, PUBLIC_ROUTE_KEYS, SCHEDULE_KEYS,
        SUBSCRIPTION_KEYS, TOP_KEYS,
    };
    use crate::function::{EndpointAuth, HttpMethod, Runtime};

    fn schema() -> Value {
        serde_json::from_slice(FUNCTION_MANIFEST_SCHEMA).unwrap()
    }

    fn set<'a>(items: impl IntoIterator<Item = &'a str>) -> BTreeSet<String> {
        items.into_iter().map(str::to_string).collect()
    }

    fn properties(object: &Value) -> BTreeSet<String> {
        object["properties"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    }

    fn required(object: &Value) -> BTreeSet<String> {
        object["required"]
            .as_array()
            .map(|a| a.iter().map(|v| v.as_str().unwrap().to_string()).collect())
            .unwrap_or_default()
    }

    fn enum_values(schema: &Value) -> BTreeSet<String> {
        schema["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    }

    fn item(name: &str) -> Value {
        schema()["properties"][name]["items"].clone()
    }

    #[test]
    fn every_level_has_the_parsers_keys() {
        let schema = schema();
        let mut top = set(TOP_KEYS.iter().copied());
        top.insert("$schema".into());
        assert_eq!(properties(&schema), top);
        // `entrypoint` is required unless the runtime is `component`
        // (the `allOf` conditional).
        assert_eq!(required(&schema), set(["runtime"]));
        assert_eq!(
            schema["allOf"][1]["else"]["required"],
            serde_json::json!(["entrypoint"])
        );
        assert_eq!(
            properties(&schema["properties"]["limits"]),
            set(LIMITS_KEYS.iter().copied())
        );
        let endpoint = item("endpoints");
        assert_eq!(properties(&endpoint), set(ENDPOINT_KEYS.iter().copied()));
        assert_eq!(required(&endpoint), set(["path", "auth"]));
        assert_eq!(
            properties(&endpoint["properties"]["cors"]),
            set(CORS_KEYS.iter().copied())
        );
        let subscription = item("subscriptions");
        assert_eq!(
            properties(&subscription),
            set(SUBSCRIPTION_KEYS.iter().copied())
        );
        assert_eq!(required(&subscription), set(["eventType", "path"]));
        let schedule = item("schedules");
        assert_eq!(properties(&schedule), set(SCHEDULE_KEYS.iter().copied()));
        assert_eq!(required(&schedule), set(["cron", "path"]));
        let public = item("public");
        assert_eq!(properties(&public), set(PUBLIC_ROUTE_KEYS.iter().copied()));
        assert_eq!(required(&public), set(["hostname"]));
        let db = item("db");
        assert_eq!(properties(&db), set(DB_KEYS.iter().copied()));
        assert_eq!(required(&db), set(["name", "secretRef"]));
    }

    #[test]
    fn enums_equal_the_rust_enums_they_mirror() {
        let schema = schema();
        assert_eq!(
            enum_values(&schema["properties"]["runtime"]),
            set(Runtime::ALL.iter().map(|r| r.wire_value()))
        );
        let endpoint = &item("endpoints")["properties"];
        assert_eq!(
            enum_values(&endpoint["auth"]),
            set(EndpointAuth::ALL.iter().map(|a| a.wire_value()))
        );
        assert_eq!(
            enum_values(&endpoint["methods"]["items"]),
            set(HttpMethod::ALL.iter().map(|m| m.as_str()))
        );
        assert_eq!(
            enum_values(&item("subscriptions")["properties"]["mode"]),
            set(["IMMEDIATE", "NEXT_ON_ERROR", "BLOCK_ON_ERROR"])
        );
    }

    #[test]
    fn every_object_forbids_additional_properties() {
        let schema = schema();
        let endpoint = item("endpoints");
        for object in [
            &schema,
            &schema["properties"]["limits"],
            &endpoint,
            &endpoint["properties"]["cors"],
            &item("subscriptions"),
            &item("schedules"),
            &item("public"),
            &item("db"),
        ] {
            assert_eq!(object["additionalProperties"], Value::Bool(false));
        }
    }

    /// Java's schema plus `component`, when the Java checkout is next to
    /// this repo: the same properties, and Java's runtimes a subset of ours.
    #[test]
    fn extends_javas_file() {
        let java = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../../flowcatalyst-javalin/server/src/main/resources/schemas/function-manifest.schema.json",
        );
        let Ok(bytes) = std::fs::read(&java) else {
            eprintln!("skipped: {} not found", java.display());
            return;
        };
        let java: Value = serde_json::from_slice(&bytes).unwrap();
        let ours = schema();
        assert_eq!(properties(&java), properties(&ours));
        let theirs = enum_values(&java["properties"]["runtime"]);
        let mine = enum_values(&ours["properties"]["runtime"]);
        assert!(theirs.is_subset(&mine), "{theirs:?} ⊄ {mine:?}");
        assert_eq!(&mine - &theirs, set(["component"]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn serves_the_schema_bytes_without_a_token() {
        let response = function_manifest_schema_router::<()>()
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
}
