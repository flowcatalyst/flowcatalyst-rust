//! Java `function/Digest.java`.

use std::fmt;

use crate::usecase::UseCaseError;

/// A published version's artifact digest: `sha256:` followed by 64
/// lower-case hex characters. Upper case is rejected, not folded.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Digest(String);

impl Digest {
    /// `DIGEST_INVALID` unless `raw` is `^sha256:[0-9a-f]{64}$`.
    pub fn parse(raw: &str) -> Result<Digest, UseCaseError> {
        let valid = raw.strip_prefix("sha256:").is_some_and(|hex| {
            hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        });
        if !valid {
            return Err(UseCaseError::validation(
                "DIGEST_INVALID",
                "digest must be sha256: followed by 64 lower-case hex characters",
            ));
        }
        Ok(Digest(raw.to_string()))
    }

    pub fn value(&self) -> &str {
        &self.0
    }

    /// The digest of raw SHA-256 output.
    pub fn from_sha256(bytes: &[u8]) -> Digest {
        Digest(format!("sha256:{}", hex::encode(bytes)))
    }

    /// The 64 hex characters after `sha256:`.
    pub fn hex(&self) -> &str {
        self.0.strip_prefix("sha256:").unwrap_or(&self.0)
    }

    /// A digest that skipped [`Digest::parse`], as Java's canonical
    /// constructor allows: only for tests that prove a store validates
    /// keys itself (Java `FileArtifactBlobStoreTest`, U9).
    #[cfg(test)]
    pub(crate) fn unchecked(raw: &str) -> Digest {
        Digest(raw.to_string())
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Java `DigestTest`.
#[cfg(test)]
mod tests {
    use super::*;

    fn assert_rejected(raw: &str) {
        assert_eq!(
            Digest::parse(raw).unwrap_err().code(),
            "DIGEST_INVALID",
            "{raw:?}"
        );
    }

    #[test]
    fn valid_digest_accepted() {
        let raw = format!("sha256:{}", "a".repeat(64));
        assert_eq!(Digest::parse(&raw).unwrap().value(), raw);
    }

    #[test]
    fn rejected() {
        assert_rejected(&format!("sha256:{}", "A".repeat(64)));
        assert_rejected(&format!("sha256:{}", "a".repeat(63)));
        assert_rejected(&format!("sha256:{}", "a".repeat(65)));
        assert_rejected(&format!("sha1:{}", "a".repeat(40)));
        assert_rejected("deadbeef");
        assert_rejected("sha256deadbeef");
        assert_rejected("");
        assert_rejected(&format!("sha256:{}\n", "a".repeat(64)));
    }
}
