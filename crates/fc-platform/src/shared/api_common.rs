//! Common API types and utilities

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

mod string_or_number {
    use serde::{de, Deserialize, Deserializer};

    pub fn deserialize_u32_opt<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum StringOrNum {
            Num(u32),
            Str(String),
        }

        match Option::<StringOrNum>::deserialize(deserializer)? {
            Some(StringOrNum::Num(n)) => Ok(Some(n)),
            Some(StringOrNum::Str(s)) => s.parse().map(Some).map_err(de::Error::custom),
            None => Ok(None),
        }
    }
}

/// Standard API error response: the same envelope as
/// [`ErrorResponse`](crate::shared::error::ErrorResponse), `code` equal to
/// `error`. Build it with [`ApiError::new`].
#[derive(Debug, Serialize, ToSchema)]
pub struct ApiError {
    pub error: String,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl ApiError {
    /// `code` set to `error`, no details.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        let code = code.into();
        Self {
            error: code.clone(),
            code,
            message: message.into(),
            details: None,
        }
    }
}

/// Pagination parameters.
///
/// Accepts any of `size`, `pageSize`, or `limit` on the wire so Rust matches
/// both the Java-era and TS-era callers. Serializes back as camelCase.
#[derive(Debug, Deserialize, ToSchema, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct PaginationParams {
    #[serde(default, deserialize_with = "string_or_number::deserialize_u32_opt")]
    page: Option<u32>,
    #[serde(
        default,
        alias = "limit",
        alias = "pageSize",
        alias = "page_size",
        deserialize_with = "string_or_number::deserialize_u32_opt"
    )]
    size: Option<u32>,
}

impl PaginationParams {
    pub fn page(&self) -> u32 {
        self.page.unwrap_or(0)
    }

    pub fn size(&self) -> u32 {
        self.size.unwrap_or(20)
    }
}

impl Default for PaginationParams {
    fn default() -> Self {
        Self {
            page: Some(0),
            size: Some(20),
        }
    }
}

impl PaginationParams {
    pub fn offset(&self) -> u64 {
        (self.page() as u64) * (self.size() as u64)
    }

    pub fn limit(&self) -> i64 {
        self.size() as i64
    }
}

/// Paginated response wrapper
#[derive(Debug, Serialize, ToSchema)]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    pub page: u32,
    pub size: u32,
    pub total: u64,
    pub total_pages: u32,
}

// ─── Cursor pagination ────────────────────────────────────────────────────
//
// Used by `aud_logs` and `iam_login_attempts` — admin-style grids where
// operators legitimately scroll back through history. Keyset on
// `(created_at DESC, id DESC)`; the cursor encodes the last row of the
// page; the next request asks for rows strictly older than that key.
//
// The high-volume firehose tables (msg_events, msg_dispatch_jobs) do NOT
// use cursors — they expose `?size=` only and return the most recent rows.
// At ingest rates of 100/s+, page navigation is meaningless.
//
// Wire format is opaque base64 of "{created_at_micros}:{id}". Callers pass
// it back verbatim; the API never promises stability across major versions.

#[derive(Debug)]
pub struct CursorDecodeError;

impl std::fmt::Display for CursorDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid cursor")
    }
}

impl std::error::Error for CursorDecodeError {}

/// Encode a `(created_at, id)` pair as an opaque cursor. The `id` is hashed
/// to a `u64` so the cursor doesn't leak the full TSID structure and stays
/// compact; the consuming repository only needs strict ordering for the
/// keyset comparison.
pub fn encode_cursor(created_at: chrono::DateTime<chrono::Utc>, id: &str) -> String {
    use base64::Engine;
    let micros = created_at.timestamp_micros();
    // Use the raw id string in the cursor — it's already a stable, sortable
    // TSID, and including it lets the keyset comparison be exact.
    let raw = format!("{}:{}", micros, id);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw.as_bytes())
}

/// Decoded cursor for repositories. Returns the original (created_at, id)
/// pair so the keyset WHERE clause can use them directly.
pub struct DecodedCursor {
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub id: String,
}

pub fn decode_cursor(cursor: &str) -> Result<DecodedCursor, CursorDecodeError> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(cursor.as_bytes())
        .map_err(|_| CursorDecodeError)?;
    let raw = std::str::from_utf8(&bytes).map_err(|_| CursorDecodeError)?;
    let (micros_str, id) = raw.split_once(':').ok_or(CursorDecodeError)?;
    let micros: i64 = micros_str.parse().map_err(|_| CursorDecodeError)?;
    let created_at =
        chrono::DateTime::<chrono::Utc>::from_timestamp_micros(micros).ok_or(CursorDecodeError)?;
    Ok(DecodedCursor {
        created_at,
        id: id.to_string(),
    })
}

impl<T> PaginatedResponse<T> {
    pub fn new(data: Vec<T>, page: u32, size: u32, total: u64) -> Self {
        let total_pages = ((total as f64) / (size as f64)).ceil() as u32;
        Self {
            data,
            page,
            size,
            total,
            total_pages,
        }
    }
}

/// Success response with optional message
#[derive(Debug, Serialize, ToSchema)]
pub struct SuccessResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl SuccessResponse {
    pub fn ok() -> Self {
        Self {
            success: true,
            message: None,
        }
    }

    pub fn with_message(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: Some(message.into()),
        }
    }
}

/// Created response with ID
#[derive(Debug, Serialize, ToSchema)]
pub struct CreatedResponse {
    pub id: String,
}

impl CreatedResponse {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}
