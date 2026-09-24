//! `Digest` and `SignerIdentity`, the two value types the host shares with
//! the platform (Java `platform/function/Digest.java`, `SignerIdentity.java`).

use std::fmt;

/// A published version's artifact digest: `sha256:` followed by 64
/// lower-case hex characters. No normalisation: an upper-case digest is
/// rejected, not folded.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Digest(String);

/// Java's `UseCaseException.validation("DIGEST_INVALID", …)` message, as its
/// `getMessage()` renders it; the host reports it in `UNREADABLE:` errors.
pub const DIGEST_INVALID_MESSAGE: &str =
    "validation: DIGEST_INVALID: digest must be sha256: followed by 64 lower-case hex characters";

impl Digest {
    pub fn parse(raw: &str) -> Result<Self, &'static str> {
        let hex = raw.strip_prefix("sha256:").ok_or(DIGEST_INVALID_MESSAGE)?;
        if hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            Ok(Self(raw.to_owned()))
        } else {
            Err(DIGEST_INVALID_MESSAGE)
        }
    }

    /// Builds the digest of raw sha256 output.
    pub fn from_sha256(bytes: &[u8; 32]) -> Self {
        Self(format!("sha256:{}", hex::encode(bytes)))
    }

    pub fn value(&self) -> &str {
        &self.0
    }

    /// The 64-character lower-case hex part.
    pub fn hex(&self) -> &str {
        &self.0["sha256:".len()..]
    }

    /// The 32 raw bytes.
    pub fn bytes(&self) -> [u8; 32] {
        let mut out = [0u8; 32];
        hex::decode_to_slice(self.hex(), &mut out).expect("validated at construction");
        out
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_is_strict() {
        let ok = format!("sha256:{}", "a".repeat(64));
        assert!(Digest::parse(&ok).is_ok());
        assert!(Digest::parse(&format!("sha256:{}", "A".repeat(64))).is_err());
        assert!(Digest::parse(&format!("sha256:{}", "a".repeat(63))).is_err());
        assert!(Digest::parse(&format!("sha512:{}", "a".repeat(64))).is_err());
        assert_eq!(Digest::parse(&ok).unwrap().hex(), "a".repeat(64));
    }
}
