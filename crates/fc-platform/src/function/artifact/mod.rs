//! The platform's write side for uploaded function artifacts (Java
//! `function/artifact/ArtifactBlobStore.java`, `ArtifactBlobStores.java`,
//! `ArtifactBlobKeys.java`, `ArtifactException.java`,
//! `ArtifactHttpException.java`, `PlatformArtifactRef.java`; spec
//! `function-artifact-upload.md` §1-§3).
//!
//! A developer uploads an artifact to the platform
//! (`PUT /api/functions/{address}/artifacts/{digest}`), the platform writes
//! it to the configured store, and a publish refers to it as
//! `platform://<functionId>/<hex>`. Two stores, chosen once at startup by
//! `FC_FN_ARTIFACT_STORE` ([`configure`]): `file:///abs/dir`
//! ([`FileArtifactBlobStore`]) and `s3://bucket[/prefix]`
//! ([`S3ArtifactBlobStore`]). Unset means no store: the upload route and a
//! `platform://` publish answer `503 ARTIFACT_STORE_NOT_CONFIGURED`.
//!
//! **Why an upload writes no event and no audit row.** Storing a blob is
//! platform infrastructure, like event and dispatch-job ingest (CLAUDE.md,
//! "Exceptions: Platform Infrastructure Processing"): it changes nothing a
//! caller can observe until a version is published against it, and that
//! publish is a use case with its event and audit row. An orphan blob is
//! garbage, not state; a function delete collects it. This is Java's rule
//! too (spec §3, last paragraph).

mod file;
mod keys;
mod platform_ref;
mod s3;
pub mod upload;

use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;

pub use file::FileArtifactBlobStore;
pub use platform_ref::PlatformArtifactRef;
pub use s3::S3ArtifactBlobStore;

use super::Digest;
use crate::usecase::UseCaseError;

/// The upload cap: 256 MiB, the same constant the fetch side uses (Java
/// `ArtifactStoreSupport.defaultMaxBytes`).
pub const MAX_BYTES: u64 = 256 * 1024 * 1024;

/// A blob's bytes, as [`ArtifactBlobStore::open`] streams them.
pub type ArtifactStream = Pin<Box<dyn tokio::io::AsyncRead + Send>>;

/// Why a store call failed (the reasons of Java's `ArtifactException` a
/// blob store raises).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ArtifactError {
    #[error("artifact not found")]
    NotFound,
    /// A malformed function id or digest, refused by the store itself.
    #[error("malformed artifact reference: {0}")]
    BadRef(String),
    /// An I/O or S3 failure.
    #[error("transport failure: {0}")]
    Transport(String),
}

/// Where uploaded artifacts are kept, keyed by `(functionId, digest)` (Java
/// `ArtifactBlobStore`). Every implementation validates both key parts
/// itself: they become path segments and object keys.
#[async_trait]
pub trait ArtifactBlobStore: Send + Sync {
    /// Stores `file`, already hashed to `digest` by the caller. Idempotent:
    /// an existing blob is left exactly as it is.
    async fn put(
        &self,
        function_id: &str,
        digest: &Digest,
        file: &Path,
    ) -> Result<(), ArtifactError>;

    async fn exists(&self, function_id: &str, digest: &Digest) -> Result<bool, ArtifactError>;

    /// The blob's bytes; [`ArtifactError::NotFound`] when absent.
    async fn open(
        &self,
        function_id: &str,
        digest: &Digest,
    ) -> Result<ArtifactStream, ArtifactError>;

    async fn size(&self, function_id: &str, digest: &Digest) -> Result<u64, ArtifactError>;

    /// Every blob of a function. Best-effort callers only: a failure is a
    /// WARN, never a failed function delete.
    async fn delete_all(&self, function_id: &str) -> Result<(), ArtifactError>;
}

/// `FC_FN_ARTIFACT_STORE` (Java `ArtifactBlobStores.configure`): blank is
/// no store; `file:///abs/dir` and `s3://bucket[/prefix]` are the two
/// stores; anything else fails startup, naming the variable, never a silent
/// default.
pub fn configure(spec: &str) -> Result<Option<Arc<dyn ArtifactBlobStore>>, String> {
    if super::java_is_blank(spec) {
        return Ok(None);
    }
    if let Some(rest) = spec.strip_prefix("file://") {
        let dir = file_dir(spec, rest)?;
        let store =
            FileArtifactBlobStore::new(dir).map_err(|e| format!("FC_FN_ARTIFACT_STORE: {e}"))?;
        return Ok(Some(Arc::new(store)));
    }
    if let Some(rest) = spec.strip_prefix("s3://") {
        let (bucket, prefix) = s3_location(spec, rest)?;
        return Ok(Some(Arc::new(S3ArtifactBlobStore::from_environment(
            bucket, prefix,
        ))));
    }
    Err(format!(
        "FC_FN_ARTIFACT_STORE must be file:///abs/dir or s3://bucket[/prefix], got: {spec}"
    ))
}

/// The store `FC_FN_ARTIFACT_STORE` names (Java `Env.java:609`, no default;
/// fc-dev sets its own).
pub fn store_from_env() -> Result<Option<Arc<dyn ArtifactBlobStore>>, String> {
    configure(&std::env::var("FC_FN_ARTIFACT_STORE").unwrap_or_default())
}

/// The publish-time signature policy (Java `Env.java:550,606-607` and
/// `Signatures.resolve`): `FC_FN_SIGNATURES` (`required` unless spelled
/// `off`), which is refused outside `FLOWCATALYST_DEV_MODE`, and
/// `FC_FN_TRUST_ROOT` (blank: the committed Sigstore public-good root).
pub fn signatures_from_env() -> Result<fc_function_signing::Signatures, String> {
    let var = |key: &str| std::env::var(key).unwrap_or_default();
    fc_function_signing::Signatures::resolve(
        fc_function_signing::SignaturesMode::parse(&var("FC_FN_SIGNATURES")),
        java_env_bool(&var("FLOWCATALYST_DEV_MODE")).unwrap_or(false),
        &var("FC_FN_TRUST_ROOT"),
    )
}

/// Java `EnvReader.parseBool`: `1 true yes on` and `0 false no off`, trimmed
/// and in any case; anything else is unset.
fn java_env_bool(raw: &str) -> Option<bool> {
    match raw.trim().to_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// `file:///abs/dir`, read as an operator writes it: a literal space
/// (macOS's `Application Support`) and its percent-encoded form both name
/// the same directory. No host, and an absolute path.
fn file_dir(spec: &str, rest: &str) -> Result<std::path::PathBuf, String> {
    let (host, path) = match rest.find('/') {
        Some(slash) => (&rest[..slash], &rest[slash..]),
        None => (rest, ""),
    };
    if !host.is_empty() {
        return Err(format!(
            "FC_FN_ARTIFACT_STORE file:// must not carry a host: {spec}"
        ));
    }
    let path = urlencoding::decode(path)
        .map_err(|_| format!("FC_FN_ARTIFACT_STORE file:// path is not valid UTF-8: {spec}"))?;
    if path.is_empty() || !path.starts_with('/') {
        return Err(format!(
            "FC_FN_ARTIFACT_STORE file:// must be an absolute path: {spec}"
        ));
    }
    // `file:///C:/dir` names `C:/dir` on Windows.
    #[cfg(windows)]
    if path.as_bytes().get(2) == Some(&b':') {
        return Ok(std::path::PathBuf::from(&path[1..]));
    }
    Ok(std::path::PathBuf::from(path.into_owned()))
}

/// `s3://bucket[/prefix]`: the bucket is the URI's host (Java reads
/// `URI.getHost`, which is absent for anything but a hostname), the prefix
/// the path with its slashes trimmed.
fn s3_location(spec: &str, rest: &str) -> Result<(String, String), String> {
    let (bucket, path) = match rest.find('/') {
        Some(slash) => (&rest[..slash], &rest[slash..]),
        None => (rest, ""),
    };
    let hostname = !bucket.is_empty()
        && bucket
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.');
    if !hostname {
        return Err(format!(
            "FC_FN_ARTIFACT_STORE s3:// must name a bucket: {spec}"
        ));
    }
    Ok((bucket.to_string(), path.trim_matches('/').to_string()))
}

// ── The artifact errors a caller answers with (Java ArtifactHttpException) ──

/// `503 ARTIFACT_STORE_NOT_CONFIGURED`: the upload route and a
/// `platform://` publish alike.
pub fn store_not_configured() -> UseCaseError {
    UseCaseError::unavailable(
        "ARTIFACT_STORE_NOT_CONFIGURED",
        "no function-artifact store is configured (FC_FN_ARTIFACT_STORE is unset)",
    )
}

/// `413 ARTIFACT_TOO_LARGE`: a declared `Content-Length` over the cap, or
/// the running count passing it mid-stream.
pub fn too_large() -> crate::shared::error::PlatformError {
    crate::shared::error::PlatformError::Coded {
        status: axum::http::StatusCode::PAYLOAD_TOO_LARGE,
        code: "ARTIFACT_TOO_LARGE".to_string(),
        message: format!("artifact exceeds the {MAX_BYTES}-byte limit"),
        details: Default::default(),
    }
}

/// `422 ARTIFACT_EMPTY`.
pub fn empty() -> UseCaseError {
    UseCaseError::unprocessable("ARTIFACT_EMPTY", "the uploaded artifact is empty")
}

/// `422 DIGEST_MISMATCH`: the bytes do not hash to the `{digest}` segment.
pub fn digest_mismatch(expected: &Digest, actual: &Digest) -> UseCaseError {
    UseCaseError::unprocessable(
        "DIGEST_MISMATCH",
        format!(
            "digest mismatch: expected {} but got {}",
            expected.value(),
            actual.value()
        ),
    )
}

/// `422 ARTIFACT_REF_MISMATCH`: a `platform://` ref names another function,
/// or a hex other than the publish command's own digest.
pub fn ref_mismatch() -> UseCaseError {
    UseCaseError::unprocessable(
        "ARTIFACT_REF_MISMATCH",
        "platform:// artifactRef must name this function and the published digest",
    )
}

/// The kind of WASM an uploaded blob is, from its first bytes (the rest is
/// never read).
pub async fn sniff(
    store: &dyn ArtifactBlobStore,
    function_id: &str,
    digest: &Digest,
) -> Result<super::WasmKind, ArtifactError> {
    use tokio::io::AsyncReadExt;
    let mut stream = store.open(function_id, digest).await?;
    let mut header = Vec::with_capacity(super::WasmKind::HEADER_LEN);
    (&mut stream)
        .take(super::WasmKind::HEADER_LEN as u64)
        .read_to_end(&mut header)
        .await
        .map_err(|e| ArtifactError::Transport(e.to_string()))?;
    Ok(super::WasmKind::sniff(&header))
}

/// `422 ARTIFACT_RUNTIME_MISMATCH` (beyond Java): the runtime needs a WASI
/// component and the uploaded artifact is something else.
pub fn runtime_mismatch(runtime: super::Runtime, found: super::WasmKind) -> UseCaseError {
    UseCaseError::unprocessable(
        "ARTIFACT_RUNTIME_MISMATCH",
        format!(
            "runtime '{}' needs a WASI 0.2 component; the uploaded artifact is {}",
            runtime.wire_value(),
            found.describe()
        ),
    )
}

/// `422 ARTIFACT_NOT_UPLOADED`: the right function and digest, but nothing
/// was ever uploaded for them.
pub fn not_uploaded() -> UseCaseError {
    UseCaseError::unprocessable(
        "ARTIFACT_NOT_UPLOADED",
        "no artifact has been uploaded for this function at this digest yet",
    )
}

/// Java `ArtifactBlobStoresTest`.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_or_blank_is_no_store() {
        assert!(configure("").unwrap().is_none());
        assert!(configure("   ").unwrap().is_none());
    }

    #[tokio::test]
    async fn a_file_path_with_a_space_is_accepted_literally_and_percent_encoded() {
        let base = std::env::temp_dir().join(format!(
            "fc-blob-cfg-{}",
            crate::shared::tsid::generate_untyped()
        ));
        let dir = base.join("Application Support").join("fn-artifacts");
        assert_eq!(
            file_dir("", &format!("{}", dir.display())).unwrap(),
            dir,
            "literal"
        );
        let encoded = format!("{}", dir.display()).replace(' ', "%20");
        assert_eq!(file_dir("", &encoded).unwrap(), dir, "percent-encoded");
        assert!(configure(&format!("file://{}", dir.display()))
            .unwrap()
            .is_some());
        assert!(dir.is_dir(), "the store creates its directory");
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn an_unrecognised_scheme_fails_startup_naming_the_variable() {
        let err = configure("ftp://x").err().unwrap();
        assert!(err.contains("FC_FN_ARTIFACT_STORE"), "{err}");
        assert!(err.contains("ftp://x"), "{err}");
    }

    #[test]
    fn a_file_uri_with_a_host_or_a_relative_path_is_rejected() {
        let err = configure("file://host/some/path").err().unwrap();
        assert!(err.contains("FC_FN_ARTIFACT_STORE"), "{err}");
        assert!(configure("file://relative/path").is_err());
        assert!(configure("file://").is_err());
    }

    #[test]
    fn an_s3_uri_with_no_bucket_is_rejected() {
        let err = configure("s3:///no-bucket").err().unwrap();
        assert!(err.contains("bucket"), "{err}");
        assert_eq!(
            s3_location("", "my-bucket/fn-artifacts/").unwrap(),
            ("my-bucket".to_string(), "fn-artifacts".to_string())
        );
        assert_eq!(
            s3_location("", "my-bucket").unwrap(),
            ("my-bucket".to_string(), String::new())
        );
    }

    #[test]
    fn dev_mode_reads_as_javas_env_reader_does() {
        for yes in ["1", "true", " YES ", "On"] {
            assert_eq!(java_env_bool(yes), Some(true), "{yes}");
        }
        for no in ["0", "false", "No", "off"] {
            assert_eq!(java_env_bool(no), Some(false), "{no}");
        }
        for unset in ["", "y", "enabled"] {
            assert_eq!(java_env_bool(unset), None, "{unset}");
        }
    }

    #[test]
    fn the_http_errors_have_javas_statuses_and_codes() {
        let d = Digest::parse(&format!("sha256:{}", "a".repeat(64))).unwrap();
        let e = Digest::parse(&format!("sha256:{}", "b".repeat(64))).unwrap();
        let cases = [
            (store_not_configured(), 503, "ARTIFACT_STORE_NOT_CONFIGURED"),
            (empty(), 422, "ARTIFACT_EMPTY"),
            (digest_mismatch(&d, &e), 422, "DIGEST_MISMATCH"),
            (ref_mismatch(), 422, "ARTIFACT_REF_MISMATCH"),
            (not_uploaded(), 422, "ARTIFACT_NOT_UPLOADED"),
        ];
        for (err, status, code) in cases {
            assert_eq!((err.http_status_code(), err.code()), (status, code));
        }
        assert_eq!(
            digest_mismatch(&d, &e).message(),
            format!("digest mismatch: expected {d} but got {e}")
        );
        match too_large() {
            crate::shared::error::PlatformError::Coded {
                status,
                code,
                message,
                ..
            } => {
                assert_eq!(status.as_u16(), 413);
                assert_eq!(code, "ARTIFACT_TOO_LARGE");
                assert_eq!(message, "artifact exceeds the 268435456-byte limit");
            }
            other => panic!("{other:?}"),
        }
    }
}
