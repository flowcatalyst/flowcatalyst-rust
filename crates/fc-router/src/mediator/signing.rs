//! Webhook signing — HMAC-SHA256 over `timestamp || body`.
//!
//! The wire format must stay byte-identical to the Java / TypeScript / Go /
//! Laravel SDKs that verify signatures on the receiving side:
//!
//! - `timestamp` is ISO-8601 with millisecond precision (`2025-01-30T12:00:00.123Z`).
//! - `signing_payload = timestamp + body` (concatenated bytes, no separator).
//! - `signature = lowercase_hex(HMAC_SHA256(signing_secret, signing_payload))`.
//!
//! Any change here MUST be coordinated with the SDKs.

use chrono::Utc;
use hmac::{Hmac, Mac};
use serde::Serialize;
use sha2::Sha256;

/// FlowCatalyst webhook signature header.
pub const SIGNATURE_HEADER: &str = "X-FLOWCATALYST-SIGNATURE";
/// FlowCatalyst webhook timestamp header.
pub const TIMESTAMP_HEADER: &str = "X-FLOWCATALYST-TIMESTAMP";

type HmacSha256 = Hmac<Sha256>;

/// Payload sent to the mediation target — `{"messageId":"<id>"}`.
#[derive(Debug, Serialize)]
pub(super) struct MediationPayload<'a> {
    #[serde(rename = "messageId")]
    pub message_id: &'a str,
}

/// Sign a webhook body. Returns `(signature_hex_lowercase, iso8601_timestamp)`.
pub(super) fn sign_webhook(payload: &str, signing_secret: &str) -> (String, String) {
    let timestamp = Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    let signature_payload = format!("{}{}", timestamp, payload);
    let mut mac = HmacSha256::new_from_slice(signing_secret.as_bytes())
        .expect("HMAC can take key of any size");
    mac.update(signature_payload.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());
    (signature, timestamp)
}
