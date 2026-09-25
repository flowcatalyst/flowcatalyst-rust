//! What one side answered for one step (spec §4): the status, the compared
//! headers, and the body: JSON when the content type says so, SHA-256 for a
//! known binary body (the QR PNG, the OpenAPI document), raw text otherwise.

use indexmap::IndexMap;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

/// The wire-format oracle: a body diff on the wrong content type is usually the whole story.
pub const CONTENT_TYPE: &str = "Content-Type";
/// Redirect targets, compared after normalisation.
pub const LOCATION: &str = "Location";
/// The 401 challenge shape.
pub const WWW_AUTHENTICATE: &str = "WWW-Authenticate";
/// Rate-limit backoff: presence only (the normaliser masks the value).
pub const RETRY_AFTER: &str = "Retry-After";
/// Cache directives on static/semi-static responses (the OpenAPI document, JWKS).
pub const CACHE_CONTROL: &str = "Cache-Control";
/// Cookie name + attributes compared, the value masked (rule 5).
pub const SET_COOKIE: &str = "Set-Cookie";

/// Every header a [`StepRecord`] keeps, and nothing else (`Date`, `Server`,
/// `Content-Length`, … are never compared).
pub const COMPARED_HEADERS: [&str; 6] = [
    CONTENT_TYPE,
    LOCATION,
    WWW_AUTHENTICATE,
    RETRY_AFTER,
    CACHE_CONTROL,
    SET_COOKIE,
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Body {
    Json(Value),
    Text(String),
    Sha256(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct StepRecord {
    pub status: u16,
    pub headers: IndexMap<String, String>,
    pub body: Body,
}

impl StepRecord {
    /// Classifies a raw response. Only the first value of each compared
    /// header is kept (as Java's `HttpHeaders.firstValue`). A body that
    /// claims JSON but does not parse is an error (Java's `readTree` throws).
    pub fn from_response(
        status: u16,
        headers: &reqwest::header::HeaderMap,
        body: &[u8],
    ) -> anyhow::Result<Self> {
        let mut compared = IndexMap::new();
        for name in COMPARED_HEADERS {
            if let Some(v) = headers.get(name) {
                compared.insert(
                    name.to_string(),
                    String::from_utf8_lossy(v.as_bytes()).into_owned(),
                );
            }
        }
        let content_type = headers
            .get(CONTENT_TYPE)
            .map(|v| String::from_utf8_lossy(v.as_bytes()).to_ascii_lowercase())
            .unwrap_or_default();
        let body = if content_type.contains("json") {
            if body.is_empty() {
                Body::Json(Value::Null)
            } else {
                Body::Json(
                    serde_json::from_slice(body)
                        .map_err(|e| anyhow::anyhow!("response claims JSON but is not: {e}"))?,
                )
            }
        } else if content_type.starts_with("image/png")
            || content_type.contains("application/openapi")
        {
            Body::Sha256(hex::encode(Sha256::digest(body)))
        } else {
            Body::Text(String::from_utf8_lossy(body).into_owned())
        };
        Ok(Self {
            status,
            headers: compared,
            body,
        })
    }
}
