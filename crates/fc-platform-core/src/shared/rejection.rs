//! Extractor rejections in Go's envelope.
//!
//! axum answers a body, query or path its extractor cannot read with a
//! `text/plain` 400/415/422. Go's huma answers the same requests with a 400
//! JSON `VALIDATION` error (shared/httpcompat/httpcompat.go `newError`: huma's
//! request validation becomes `{"error": "VALIDATION", "message": "validation
//! failed", "details": {"errors": [{"location", "message", "value"}]}}`,
//! always status 400). This layer rewrites the axum rejections centrally, so
//! no handler needs a custom extractor:
//!
//! - platform routes: 400 `VALIDATION` with huma's details where the
//!   rejection can be read back (`expected required property <name> to be
//!   present` at `body`, `invalid integer` at `query.<name>`);
//! - `/oauth/*`: 400 `{"error": "invalid_request", "error_description"}` with
//!   `Cache-Control: no-store`, as Go's OAuth handlers answer a request they
//!   cannot parse (auth/oauthapi/token.go `writeOAuthError`).
//!
//! An axum rejection is recognised by its shape: a 400, 415 or 422 whose body
//! is `text/plain`. Platform handlers never answer that way (their errors are
//! JSON), so nothing else is touched.

use axum::{
    body::{Body, Bytes},
    extract::Request,
    http::{header, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

/// Rejection bodies are a line of text; anything longer is not one.
const MAX_REJECTION_BYTES: usize = 16 * 1024;

/// A JSON request body up to this size is kept, so a rejection can echo it
/// as huma's `value`. Larger (or unsized) bodies are streamed untouched.
const MAX_ECHOED_BODY_BYTES: u64 = 64 * 1024;

fn is_rejection(response: &Response) -> bool {
    matches!(
        response.status(),
        StatusCode::BAD_REQUEST
            | StatusCode::UNSUPPORTED_MEDIA_TYPE
            | StatusCode::UNPROCESSABLE_ENTITY
    ) && response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.starts_with("text/plain"))
}

fn is_small_json(request: &Request) -> bool {
    let json = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.starts_with("application/json"));
    let small = request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .is_some_and(|len| len <= MAX_ECHOED_BODY_BYTES);
    json && small
}

/// The name between the first pair of backticks: serde's ``missing field `x` ``.
fn backticked(text: &str) -> Option<&str> {
    let start = text.find('`')? + 1;
    let len = text[start..].find('`')?;
    Some(&text[start..start + len])
}

/// The value of query parameter `name` in `query`, percent-decoded.
fn query_value(query: Option<&str>, name: &str) -> Option<String> {
    url_pairs(query?)
        .into_iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v)
}

fn url_pairs(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            let decode = |s: &str| {
                urlencoding::decode(&s.replace('+', " "))
                    .map(|c| c.into_owned())
                    .unwrap_or_else(|_| s.to_string())
            };
            (decode(k), decode(v))
        })
        .collect()
}

/// huma's detail for a rejection, as far as axum's text says what failed.
fn detail(text: &str, query: Option<&str>, body: Option<&Bytes>) -> Value {
    if let Some(rest) = text.strip_prefix("Failed to deserialize query string: ") {
        // `pageSize: invalid digit found in string`, or a bare reason when
        // the field is unknown (a flattened struct).
        let (field, reason) = match rest.split_once(": ") {
            Some((f, r)) if !f.contains(' ') => (Some(f), r),
            _ => (None, rest),
        };
        let message = if reason.contains("invalid digit") || reason.contains("cannot parse integer")
        {
            "invalid integer".to_string()
        } else if let Some(missing) = reason.strip_prefix("missing field ") {
            let name = backticked(missing).unwrap_or(missing);
            return json!({
                "location": format!("query.{name}"),
                "message": "required query parameter is missing",
            });
        } else {
            reason.to_string()
        };
        return match field {
            Some(f) => {
                let mut d = json!({"location": format!("query.{f}"), "message": message});
                if let Some(v) = query_value(query, f) {
                    d["value"] = Value::String(v);
                }
                d
            }
            // serde names no field when the struct is flattened; the
            // parameter is then the first one whose value is not an integer.
            None if message == "invalid integer" => {
                let offending = query.map(url_pairs).and_then(|pairs| {
                    pairs
                        .into_iter()
                        .find(|(_, v)| !v.is_empty() && v.parse::<i64>().is_err())
                });
                match offending {
                    Some((name, value)) => json!({
                        "location": format!("query.{name}"),
                        "message": message,
                        "value": value,
                    }),
                    None => json!({"location": "query", "message": message}),
                }
            }
            None => json!({"location": "query", "message": message}),
        };
    }
    let body_value = || body.and_then(|b| serde_json::from_slice::<Value>(b).ok());
    if let Some(rest) =
        text.strip_prefix("Failed to deserialize the JSON body into the target type: ")
    {
        // A top-level missing field (no `path: ` prefix).
        if let Some(missing) = rest.strip_prefix("missing field ") {
            if let Some(name) = backticked(missing) {
                let mut d = json!({
                    "location": "body",
                    "message": format!("expected required property {name} to be present"),
                });
                if let Some(v) = body_value() {
                    d["value"] = v;
                }
                return d;
            }
        }
        let (location, reason) = match rest.split_once(": ") {
            Some((path, reason)) if !path.contains(' ') => (format!("body.{path}"), reason),
            _ => ("body".to_string(), rest),
        };
        let reason = reason
            .rsplit_once(" at line ")
            .map(|(r, _)| r)
            .unwrap_or(reason);
        return json!({"location": location, "message": reason});
    }
    if text.contains("Content-Type") {
        return json!({"location": "body", "message": "request body is required"});
    }
    let location = if text.contains("URL") || text.contains("path") {
        "path"
    } else {
        "body"
    };
    json!({"location": location, "message": text})
}

/// Rewrite an axum extractor rejection into Go's envelope (see the module
/// docs). Mounted once, around every route.
pub async fn go_rejections(request: Request, next: Next) -> Response {
    let oauth = request.uri().path().starts_with("/oauth/");
    let query = request.uri().query().map(str::to_string);
    let (request, body) =
        if !oauth && is_small_json(&request) {
            let (parts, body) = request.into_parts();
            match axum::body::to_bytes(body, MAX_ECHOED_BODY_BYTES as usize).await {
                Ok(bytes) => (
                    Request::from_parts(parts, Body::from(bytes.clone())),
                    Some(bytes),
                ),
                Err(_) => return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "VALIDATION", "message": "unable to read request body"})),
                )
                    .into_response(),
            }
        } else {
            (request, None)
        };

    let response = next.run(request).await;
    if !is_rejection(&response) {
        return response;
    }
    let (parts, rejected) = response.into_parts();
    let text = match axum::body::to_bytes(rejected, MAX_REJECTION_BYTES).await {
        Ok(bytes) => String::from_utf8_lossy(&bytes).trim().to_string(),
        Err(_) => String::new(),
    };
    let mut response = if oauth {
        let mut r = (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "invalid_request",
                "error_description": text,
            })),
        )
            .into_response();
        let headers = r.headers_mut();
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        headers.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
        r
    } else {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "VALIDATION",
                "message": "validation failed",
                "details": {"errors": [detail(&text, query.as_deref(), body.as_ref())]},
            })),
        )
            .into_response()
    };
    // Keep what the rejected response carried besides its body (a
    // correlation id, CORS headers set inside).
    for (name, value) in parts.headers.iter() {
        if name != header::CONTENT_TYPE && name != header::CONTENT_LENGTH {
            response.headers_mut().entry(name).or_insert(value.clone());
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::Query, routing::post, Router};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[derive(serde::Deserialize)]
    #[allow(dead_code)]
    struct Thing {
        name: String,
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    #[allow(dead_code)]
    struct Paging {
        page_size: Option<u32>,
    }

    async fn create(Json(_): Json<Thing>) -> StatusCode {
        StatusCode::OK
    }

    async fn list(Query(_): Query<Paging>) -> StatusCode {
        StatusCode::OK
    }

    fn app() -> Router {
        Router::new()
            .route("/api/things", post(create).get(list))
            .route("/oauth/things", post(create))
            .layer(axum::middleware::from_fn(go_rejections))
    }

    async fn call(
        method: &str,
        uri: &str,
        content_type: Option<&str>,
        body: &str,
    ) -> (StatusCode, Value) {
        let mut req = axum::http::Request::builder().method(method).uri(uri);
        if let Some(ct) = content_type {
            req = req
                .header(header::CONTENT_TYPE, ct)
                .header(header::CONTENT_LENGTH, body.len());
        }
        let resp = app()
            .oneshot(req.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes)
                .unwrap_or(Value::String(String::from_utf8_lossy(&bytes).to_string())),
        )
    }

    #[tokio::test]
    async fn a_missing_field_is_humas_required_property() {
        let (status, v) = call(
            "POST",
            "/api/things",
            Some("application/json"),
            r#"{"code":"x"}"#,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            v,
            json!({
                "error": "VALIDATION",
                "message": "validation failed",
                "details": {"errors": [{
                    "location": "body",
                    "message": "expected required property name to be present",
                    "value": {"code": "x"},
                }]},
            })
        );
    }

    #[tokio::test]
    async fn a_non_integer_query_parameter_is_invalid_integer() {
        let (status, v) = call("GET", "/api/things?pageSize=not-a-number", None, "").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            v["details"]["errors"][0],
            json!({"location": "query.pageSize", "message": "invalid integer", "value": "not-a-number"})
        );
    }

    #[tokio::test]
    async fn malformed_json_and_a_missing_content_type_are_400s() {
        let (status, v) = call("POST", "/api/things", Some("application/json"), "{").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(v["error"], "VALIDATION");
        let (status, v) = call("POST", "/api/things", None, "").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(v["error"], "VALIDATION");
    }

    #[tokio::test]
    async fn oauth_rejections_are_invalid_request() {
        let (status, v) = call("POST", "/oauth/things", Some("application/json"), "{}").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(v["error"], "invalid_request");
    }
}
