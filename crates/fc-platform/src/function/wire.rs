//! What the function API writes and reads on the wire, where that differs
//! from the rest of the platform: Java's timestamp format and its
//! `INVALID_JSON` answer to a body that does not bind.

use chrono::{DateTime, SecondsFormat, Utc};
use serde::de::DeserializeOwned;
use serde::Serializer;

use crate::shared::error::PlatformError;

/// A timestamp as Java's `Json` mapper writes it (shared/json/Json.java):
/// RFC 3339 with exactly six fractional digits and `Z`.
pub fn micros(at: &DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Micros, true)
}

/// `#[serde(serialize_with = "micros_ser")]` for a `DateTime<Utc>` field.
pub fn micros_ser<S: Serializer>(at: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&micros(at))
}

/// [`micros_ser`] for an optional field; pair it with
/// `skip_serializing_if = "Option::is_none"` (Java's `NON_ABSENT`).
pub fn micros_opt_ser<S: Serializer>(at: &Option<DateTime<Utc>>, s: S) -> Result<S::Ok, S::Error> {
    match at {
        Some(at) => s.serialize_str(&micros(at)),
        None => s.serialize_none(),
    }
}

/// `400 INVALID_JSON` (Java `Json.invalidJson`): the body is not JSON, or
/// does not bind to the request type.
pub fn invalid_json(error: &serde_json::Error) -> PlatformError {
    PlatformError::Coded {
        status: axum::http::StatusCode::BAD_REQUEST,
        code: "INVALID_JSON".to_string(),
        message: error.to_string(),
        details: Default::default(),
    }
}

/// Parse a request body as Java's `ctx.bodyAsClass` does: unknown fields
/// are ignored, anything that does not bind is `400 INVALID_JSON`. Handlers
/// take the body as bytes and call this after their permission check, so a
/// caller without the permission gets Java's 403 whatever the body holds.
pub fn parse_body<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, PlatformError> {
    serde_json::from_slice(bytes).map_err(|e| invalid_json(&e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn timestamps_carry_exactly_six_fractional_digits() {
        let whole = Utc.with_ymd_and_hms(2026, 9, 24, 10, 0, 0).unwrap();
        assert_eq!(micros(&whole), "2026-09-24T10:00:00.000000Z");
        let fine = whole + chrono::Duration::nanoseconds(123_456_789);
        assert_eq!(micros(&fine), "2026-09-24T10:00:00.123456Z");
    }

    #[test]
    fn a_body_that_does_not_bind_is_invalid_json() {
        #[derive(serde::Deserialize, Debug)]
        #[allow(dead_code)]
        struct Req {
            value: String,
        }
        for body in [&b"not json"[..], b"", b"{\"value\": 5}"] {
            match parse_body::<Req>(body) {
                Err(PlatformError::Coded { status, code, .. }) => {
                    assert_eq!(status.as_u16(), 400);
                    assert_eq!(code, "INVALID_JSON");
                }
                other => panic!("unexpected {other:?}"),
            }
        }
        // Unknown fields are ignored, as Java's mapper does.
        let ok: Req = parse_body(br#"{"value":"x","extra":1}"#).unwrap();
        assert_eq!(ok.value, "x");
    }
}
