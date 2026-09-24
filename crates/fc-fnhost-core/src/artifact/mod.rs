//! Fetching a function version's artifact by content digest (Java
//! `platform/function/artifact/{ArtifactStore,ArtifactStores,
//! ArtifactStoreSupport,FileArtifactStore,OciArtifactStore}.java` and the
//! host's `reconcile/PlatformArtifactStore.java`).
//!
//! Every store shares one cache (`<cache>/sha256/<hex>`): a fetch streams
//! through a running sha256 into a temp file and atomically moves it into
//! place only once the digest matches; a cached copy is re-hashed on every
//! fetch and replaced when it no longer matches; transfers are capped at
//! 256 MiB.

mod cache;
mod file;
mod oci;
mod platform;

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;

pub use cache::{ArtifactCache, BodyReader, SourceStream, DEFAULT_MAX_BYTES};
pub use file::FileSource;
pub use oci::{OciSource, RegistryCredentials};
pub use platform::PlatformSource;

use crate::digest::Digest;

/// Why a fetch failed. [`ArtifactError::simple_name`] is Java's record
/// simple name, which the heartbeat carries as `ARTIFACT:<Name>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactError {
    /// A registry 404, or a `file://` path that is not a regular file.
    NotFound,
    /// The bytes hash to something other than expected.
    DigestMismatch { expected: Digest, actual: Digest },
    /// The transfer passed the byte cap (or declared a length over it).
    TooLarge { limit: u64 },
    /// No store for the reference's scheme.
    UnsupportedScheme(String),
    /// A registry or the control plane rejected every credential.
    Unauthorized,
    /// An I/O error, a timeout, or an unexpected status.
    Transport(String),
    /// The reference itself is malformed.
    BadRef(String),
    /// A `platform://` fetch without a version id.
    VersionRequired,
}

impl ArtifactError {
    pub fn simple_name(&self) -> &'static str {
        match self {
            ArtifactError::NotFound => "NotFound",
            ArtifactError::DigestMismatch { .. } => "DigestMismatch",
            ArtifactError::TooLarge { .. } => "TooLarge",
            ArtifactError::UnsupportedScheme(_) => "UnsupportedScheme",
            ArtifactError::Unauthorized => "Unauthorized",
            ArtifactError::Transport(_) => "Transport",
            ArtifactError::BadRef(_) => "BadRef",
            ArtifactError::VersionRequired => "VersionRequired",
        }
    }

    pub(crate) fn transport(e: impl fmt::Display) -> Self {
        ArtifactError::Transport(e.to_string())
    }
}

impl fmt::Display for ArtifactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArtifactError::NotFound => f.write_str("artifact not found"),
            ArtifactError::DigestMismatch { expected, actual } => {
                write!(f, "digest mismatch: expected {expected} but got {actual}")
            }
            ArtifactError::TooLarge { limit } => {
                write!(f, "artifact exceeds the {limit}-byte limit")
            }
            ArtifactError::UnsupportedScheme(s) => {
                write!(f, "unsupported artifact reference scheme: {s}")
            }
            ArtifactError::Unauthorized => f.write_str("registry rejected every credential"),
            ArtifactError::Transport(cause) => write!(f, "transport failure: {cause}"),
            ArtifactError::BadRef(why) => write!(f, "malformed artifact reference: {why}"),
            ArtifactError::VersionRequired => {
                f.write_str("platform:// artifact fetch requires a version id")
            }
        }
    }
}

impl std::error::Error for ArtifactError {}

/// A fetched artifact: a regular file under the cache directory, named
/// `<cache>/sha256/<hex>`, whose bytes hash to the requested digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    pub file: PathBuf,
    pub bytes: u64,
}

/// Fetches an artifact by reference, verifying it hashes to `expected`.
/// `version_id` is the desired-state entry's own id: `platform://` needs it
/// (the download route is keyed by it), the other schemes ignore it.
#[async_trait]
pub trait ArtifactStore: Send + Sync {
    async fn fetch(
        &self,
        artifact_ref: &str,
        expected: &Digest,
        version_id: Option<&str>,
    ) -> Result<Fetched, ArtifactError>;
}

/// Where a scheme's bytes come from; the cache does the rest.
#[async_trait]
pub trait Source: Send + Sync {
    async fn open(
        &self,
        artifact_ref: &str,
        expected: &Digest,
        version_id: Option<&str>,
    ) -> Result<SourceStream, ArtifactError>;
}

/// Routes a fetch to the source for its reference's scheme: `file`, `oci`,
/// `platform`, and anything else (including `s3`, as in Java's host) is
/// `UnsupportedScheme`.
pub struct ArtifactStores {
    cache: ArtifactCache,
    sources: Vec<(String, Arc<dyn Source>)>,
}

impl ArtifactStores {
    pub fn new(cache: ArtifactCache) -> Self {
        Self {
            cache,
            sources: Vec::new(),
        }
    }

    pub fn with_source(mut self, scheme: &str, source: Arc<dyn Source>) -> Self {
        self.sources.push((scheme.to_owned(), source));
        self
    }

    fn source_for(&self, artifact_ref: &str) -> Result<&Arc<dyn Source>, ArtifactError> {
        let scheme = scheme_of(artifact_ref)?;
        self.sources
            .iter()
            .find(|(s, _)| *s == scheme)
            .map(|(_, source)| source)
            .ok_or(ArtifactError::UnsupportedScheme(scheme))
    }
}

#[async_trait]
impl ArtifactStore for ArtifactStores {
    async fn fetch(
        &self,
        artifact_ref: &str,
        expected: &Digest,
        version_id: Option<&str>,
    ) -> Result<Fetched, ArtifactError> {
        let source = self.source_for(artifact_ref)?;
        self.cache
            .fetch(source.as_ref(), artifact_ref, expected, version_id)
            .await
    }
}

/// `java.net.URI#getScheme`: `ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )`
/// before the first `:`, as written (case preserved).
fn scheme_of(artifact_ref: &str) -> Result<String, ArtifactError> {
    let Some(colon) = artifact_ref.find(':') else {
        return Err(ArtifactError::BadRef(
            "artifact reference has no scheme".into(),
        ));
    };
    let scheme = &artifact_ref[..colon];
    let valid = scheme
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !valid {
        return Err(if scheme.contains('/') || scheme.is_empty() {
            ArtifactError::BadRef("artifact reference has no scheme".into())
        } else {
            ArtifactError::BadRef(format!(
                "malformed URI: illegal character in scheme name: {artifact_ref}"
            ))
        });
    }
    if artifact_ref.chars().any(|c| c == ' ' || c.is_control()) {
        return Err(ArtifactError::BadRef(format!(
            "malformed URI: illegal character: {artifact_ref}"
        )));
    }
    Ok(scheme.to_owned())
}

/// `%XX` decoding, as `URI#getPath` applies it.
pub(crate) fn percent_decode(raw: &str) -> Result<String, ArtifactError> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = raw
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
                .ok_or_else(|| {
                    ArtifactError::BadRef(format!("malformed URI: malformed escape pair: {raw}"))
                })?;
            out.push(hex);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheme_is_taken_as_written() {
        assert_eq!(scheme_of("file:///x").unwrap(), "file");
        assert_eq!(scheme_of("FILE:///x").unwrap(), "FILE");
        assert_eq!(scheme_of("s3://b/k").unwrap(), "s3");
        assert!(matches!(
            scheme_of("/abs/path"),
            Err(ArtifactError::BadRef(_))
        ));
        assert!(matches!(
            scheme_of("no-scheme"),
            Err(ArtifactError::BadRef(_))
        ));
    }
}
