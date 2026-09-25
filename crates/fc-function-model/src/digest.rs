//! Java `function/Digest.java` and `function/SignerIdentity.java`: the two
//! value types the platform, the signature verifier and the function host
//! all share.

use std::fmt;

use crate::ValidationError;

/// A published version's artifact digest: `sha256:` followed by 64
/// lower-case hex characters. No normalisation: an upper-case digest is
/// rejected, not folded.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Digest(String);

/// Java's `UseCaseException.validation("DIGEST_INVALID", …)` message, as its
/// `getMessage()` renders it (the [`Display`](fmt::Display) of
/// [`Digest::parse`]'s error); the host reports it in `UNREADABLE:` errors.
pub const DIGEST_INVALID_MESSAGE: &str =
    "validation: DIGEST_INVALID: digest must be sha256: followed by 64 lower-case hex characters";

impl Digest {
    /// `DIGEST_INVALID` unless `raw` is `^sha256:[0-9a-f]{64}$`.
    pub fn parse(raw: &str) -> Result<Self, ValidationError> {
        let valid = raw.strip_prefix("sha256:").is_some_and(|hex| {
            hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        });
        if !valid {
            return Err(ValidationError::new(
                "DIGEST_INVALID",
                "digest must be sha256: followed by 64 lower-case hex characters",
            ));
        }
        Ok(Self(raw.to_owned()))
    }

    /// The digest of raw SHA-256 output.
    pub fn from_sha256(bytes: &[u8; 32]) -> Self {
        Self(format!("sha256:{}", hex::encode(bytes)))
    }

    pub fn value(&self) -> &str {
        &self.0
    }

    /// The 64 lower-case hex characters after `sha256:`.
    pub fn hex(&self) -> &str {
        self.0.strip_prefix("sha256:").unwrap_or(&self.0)
    }

    /// The 32 raw bytes.
    pub fn bytes(&self) -> [u8; 32] {
        let mut out = [0u8; 32];
        hex::decode_to_slice(self.hex(), &mut out).expect("validated at construction");
        out
    }

    /// A digest that skipped [`Digest::parse`], as Java's canonical
    /// constructor allows: only for tests that prove a store validates keys
    /// itself (Java `FileArtifactBlobStoreTest`, U9). Never call it on input.
    #[doc(hidden)]
    pub fn unchecked(raw: &str) -> Self {
        Self(raw.to_owned())
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Java's record `toString`.
impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Digest[value={}]", self.0)
    }
}

/// The keyless signer that published a version: the OIDC issuer and subject.
/// Compared exactly: no pattern, no case folding, no trimming.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SignerIdentity {
    pub issuer: String,
    pub subject: String,
}

impl SignerIdentity {
    pub fn new(issuer: impl Into<String>, subject: impl Into<String>) -> Self {
        Self {
            issuer: issuer.into(),
            subject: subject.into(),
        }
    }
}

/// Java `DigestTest`.
#[cfg(test)]
mod tests {
    use super::*;

    fn assert_rejected(raw: &str) {
        let err = Digest::parse(raw).unwrap_err();
        assert_eq!(err.code(), "DIGEST_INVALID", "{raw:?}");
        assert_eq!(err.to_string(), DIGEST_INVALID_MESSAGE);
    }

    #[test]
    fn valid_digest_accepted() {
        let raw = format!("sha256:{}", "a".repeat(64));
        let digest = Digest::parse(&raw).unwrap();
        assert_eq!(digest.value(), raw);
        assert_eq!(digest.hex(), "a".repeat(64));
        assert_eq!(digest.bytes(), [0xaa; 32]);
        assert_eq!(Digest::from_sha256(&[0xaa; 32]), digest);
    }

    #[test]
    fn rejected() {
        assert_rejected(&format!("sha256:{}", "A".repeat(64)));
        assert_rejected(&format!("sha256:{}", "a".repeat(63)));
        assert_rejected(&format!("sha256:{}", "a".repeat(65)));
        assert_rejected(&format!("sha1:{}", "a".repeat(40)));
        assert_rejected(&format!("sha512:{}", "a".repeat(64)));
        assert_rejected("deadbeef");
        assert_rejected("sha256deadbeef");
        assert_rejected("");
        assert_rejected(&format!("sha256:{}\n", "a".repeat(64)));
    }
}
