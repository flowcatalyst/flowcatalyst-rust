//! Signs a delivery the way every FlowCatalyst receiver verifies it (Java
//! `router/wire/WebhookSigner.java`, contract C of `docs/spec/router.md`
//! §6.3): `X-FlowCatalyst-Signature` is the lower-case hex
//! `HMAC-SHA256(secret, timestamp ‖ body)` over the exact bytes sent, and
//! `X-FlowCatalyst-Timestamp` is `yyyy-MM-ddTHH:mm:ss.SSSZ` in UTC, always
//! 24 characters. The function host's `webhook` endpoints check exactly this
//! (`fc-fnhost-core` `listener::webhook`, Java `WebhookVerifier`), as do the
//! SDKs.
//!
//! Used by both of the platform's own deliveries: the dispatch-job
//! processing endpoint (subscriptions) and the scheduled-job dispatcher.

use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use sha2::Sha256;

pub const SIGNATURE_HEADER: &str = "X-FlowCatalyst-Signature";
pub const TIMESTAMP_HEADER: &str = "X-FlowCatalyst-Timestamp";

/// `at` as the signed and transmitted timestamp: exactly three fractional
/// digits and a literal `Z`, deliberately not the platform's JSON format.
pub fn timestamp(at: DateTime<Utc>) -> String {
    at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// The signature for `body` at `timestamp`, lower-case hex. The caller
/// passes the same `timestamp` to the header, so the two cannot disagree.
pub fn sign(secret: &str, timestamp: &str, body: &[u8]) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC takes a key of any length");
    mac.update(timestamp.as_bytes());
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}

/// The `X-FlowCatalyst-Timestamp` and `X-FlowCatalyst-Signature` headers
/// for `body` signed with `secret` at `at`, in that order.
pub fn signature_headers(
    secret: &str,
    at: DateTime<Utc>,
    body: &[u8],
) -> [(&'static str, String); 2] {
    let timestamp = timestamp(at);
    let signature = sign(secret, &timestamp, body);
    [(TIMESTAMP_HEADER, timestamp), (SIGNATURE_HEADER, signature)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// Java `WebhookSignerTest`'s golden vector, shared with the SDKs.
    #[test]
    fn the_known_vector() {
        assert_eq!(
            sign("secret", "2026-08-07T09:32:12.123Z", b"hello"),
            "a7d62aade7a79e88792f84e035b72788a3c78a30f46ad265ec8d6fb12a4e9f91"
        );
    }

    #[test]
    fn the_timestamp_is_always_24_characters_with_millis() {
        let at = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        assert_eq!(timestamp(at), "2026-01-01T00:00:00.000Z");
        let at = at + chrono::Duration::microseconds(123_456);
        assert_eq!(timestamp(at), "2026-01-01T00:00:00.123Z");
    }
}
