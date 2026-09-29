//! Java `ArtifactBlobKeys`: a store validates `functionId` and the digest's
//! hex itself, not only its callers, because both become path segments and
//! object keys.

use super::ArtifactError;
use crate::function::Digest;

/// A validated `(functionId, hex)` pair.
pub(super) struct Key<'a> {
    pub function_id: &'a str,
    pub hex: &'a str,
}

/// `^[A-Za-z0-9_]+$`, else `BadRef`.
pub(super) fn validate_function_id(function_id: &str) -> Result<&str, ArtifactError> {
    let valid = !function_id.is_empty()
        && function_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_');
    if valid {
        Ok(function_id)
    } else {
        Err(ArtifactError::BadRef(
            "functionId must match ^[A-Za-z0-9_]+$".into(),
        ))
    }
}

/// Both parts validated: `sha256:` followed by `^[0-9a-f]{64}$`.
pub(super) fn of<'a>(function_id: &'a str, digest: &'a Digest) -> Result<Key<'a>, ArtifactError> {
    let function_id = validate_function_id(function_id)?;
    let Some(hex) = digest.value().strip_prefix("sha256:") else {
        return Err(ArtifactError::BadRef(
            "digest must be a sha256: digest".into(),
        ));
    };
    if hex.len() != 64 || !hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return Err(ArtifactError::BadRef(
            "digest must be sha256: followed by 64 lower-case hex characters".into(),
        ));
    }
    Ok(Key { function_id, hex })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_ids_and_hex_are_validated() {
        let good = Digest::unchecked(&format!("sha256:{}", "a".repeat(64)));
        assert!(of("fnc_0HZXEQ5Y8JY5Z", &good).is_ok());
        for bad in ["", "../x", "a/b", "a.b", "é"] {
            assert!(
                matches!(of(bad, &good), Err(ArtifactError::BadRef(_))),
                "{bad:?}"
            );
        }
        for bad in [
            format!("sha256:{}", "a".repeat(63)),
            format!("sha256:{}", "A".repeat(64)),
            format!("sha512:{}", "a".repeat(64)),
        ] {
            assert!(
                matches!(
                    of("fn1", &Digest::unchecked(&bad)),
                    Err(ArtifactError::BadRef(_))
                ),
                "{bad}"
            );
        }
    }
}
