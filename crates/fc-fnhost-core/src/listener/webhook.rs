//! `auth: webhook` (Java `fnhost/http/WebhookVerifier.java`, spec
//! `function-host-listener.md` §3): `X-FlowCatalyst-Signature` equals
//! `hex(HMAC-SHA256(secret, timestamp ‖ body))`, the timestamp no older than
//! 300 s and no more than 60 s ahead, the current secret tried first and
//! then the previous one inside its rotation window.

use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

/// No older than this.
pub const MAX_AGE_SECONDS: i64 = 300;
/// No more than this far in the future.
pub const FUTURE_GRACE_SECONDS: i64 = 60;

/// `Err` carries Java's reason: `NO_SIGNING_SECRET`, `MISSING_SIGNATURE`,
/// `MISSING_TIMESTAMP`, `INVALID_TIMESTAMP`, `TIMESTAMP_EXPIRED`,
/// `TIMESTAMP_IN_FUTURE` or `INVALID_SIGNATURE`.
pub fn verify(
    body: &[u8],
    signature: Option<&str>,
    timestamp: Option<&str>,
    current_secret: Option<&str>,
    previous_secret: Option<&str>,
    now_epoch_seconds: i64,
) -> Result<(), &'static str> {
    let Some(current) = current_secret else {
        return Err("NO_SIGNING_SECRET");
    };
    let signature = match signature {
        Some(s) if !s.is_empty() => s,
        _ => return Err("MISSING_SIGNATURE"),
    };
    let timestamp = match timestamp {
        Some(t) if !t.is_empty() => t,
        _ => return Err("MISSING_TIMESTAMP"),
    };
    let epoch_seconds = parse_timestamp(timestamp).ok_or("INVALID_TIMESTAMP")?;
    if epoch_seconds < now_epoch_seconds - MAX_AGE_SECONDS {
        return Err("TIMESTAMP_EXPIRED");
    }
    if epoch_seconds > now_epoch_seconds + FUTURE_GRACE_SECONDS {
        return Err("TIMESTAMP_IN_FUTURE");
    }
    if matches(body, signature, timestamp, current)
        || previous_secret.is_some_and(|previous| matches(body, signature, timestamp, previous))
    {
        Ok(())
    } else {
        Err("INVALID_SIGNATURE")
    }
}

/// The platform's signature (Java `WebhookSigner.sign`): lower-case hex.
pub fn sign(secret: &str, timestamp: &str, body: &[u8]) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC takes a key of any length");
    mac.update(timestamp.as_bytes());
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}

/// Constant-time over equal lengths.
fn matches(body: &[u8], signature: &str, timestamp: &str, secret: &str) -> bool {
    let expected = sign(secret, timestamp, body);
    let given = signature.to_lowercase();
    expected.len() == given.len() && bool::from(expected.as_bytes().ct_eq(given.as_bytes()))
}

/// Epoch seconds from a bare integer (backward compatibility), else an
/// RFC 3339 instant (`Timestamp::parse`; the platform sends
/// `yyyy-MM-ddTHH:mm:ss.SSSZ`).
fn parse_timestamp(raw: &str) -> Option<i64> {
    if raw.bytes().all(|b| b.is_ascii_digit()) {
        return raw.parse().ok();
    }
    fc_function_abi::Timestamp::parse(raw).map(|t| t.epoch_second())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_786_000_000;

    fn iso(epoch: i64) -> String {
        chrono::DateTime::from_timestamp(epoch, 0)
            .unwrap()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string()
    }

    // WebhookSignerTest / the SDKs' shared vector
    #[test]
    fn the_known_vector() {
        assert_eq!(
            sign("secret", "2026-08-07T09:32:12.123Z", b"hello"),
            "a7d62aade7a79e88792f84e035b72788a3c78a30f46ad265ec8d6fb12a4e9f91"
        );
    }

    #[test]
    fn every_rule() {
        let body = b"{}";
        let ts = iso(NOW);
        let sig = sign("s1", &ts, body);
        let ok = |sig: &str, ts: &str, current: Option<&str>, previous: Option<&str>| {
            verify(body, Some(sig), Some(ts), current, previous, NOW)
        };
        assert_eq!(ok(&sig, &ts, Some("s1"), None), Ok(()));
        assert_eq!(ok(&sig.to_uppercase(), &ts, Some("s1"), None), Ok(()));
        assert_eq!(ok(&sig, &ts, None, Some("s1")), Err("NO_SIGNING_SECRET"));
        assert_eq!(ok(&sig, &ts, Some("s2"), None), Err("INVALID_SIGNATURE"));
        assert_eq!(ok(&sig, &ts, Some("s2"), Some("s1")), Ok(()));
        assert_eq!(
            verify(b"{ }", Some(&sig), Some(&ts), Some("s1"), None, NOW),
            Err("INVALID_SIGNATURE")
        );
        assert_eq!(
            verify(body, None, Some(&ts), Some("s1"), None, NOW),
            Err("MISSING_SIGNATURE")
        );
        assert_eq!(
            verify(body, Some(""), Some(&ts), Some("s1"), None, NOW),
            Err("MISSING_SIGNATURE")
        );
        assert_eq!(
            verify(body, Some(&sig), None, Some("s1"), None, NOW),
            Err("MISSING_TIMESTAMP")
        );
        assert_eq!(
            ok(&sig, "yesterday", Some("s1"), None),
            Err("INVALID_TIMESTAMP")
        );
        let stale = iso(NOW - 301);
        assert_eq!(
            ok(&sign("s1", &stale, body), &stale, Some("s1"), None),
            Err("TIMESTAMP_EXPIRED")
        );
        let edge = iso(NOW - 300);
        assert_eq!(
            ok(&sign("s1", &edge, body), &edge, Some("s1"), None),
            Ok(())
        );
        let future = iso(NOW + 61);
        assert_eq!(
            ok(&sign("s1", &future, body), &future, Some("s1"), None),
            Err("TIMESTAMP_IN_FUTURE")
        );
        let grace = iso(NOW + 60);
        assert_eq!(
            ok(&sign("s1", &grace, body), &grace, Some("s1"), None),
            Ok(())
        );
        let seconds = NOW.to_string();
        assert_eq!(
            ok(&sign("s1", &seconds, body), &seconds, Some("s1"), None),
            Ok(())
        );
    }
}
