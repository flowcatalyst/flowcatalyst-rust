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

pub use file::{ArtifactDirError, FileArtifactBlobStore};
pub use platform_ref::PlatformArtifactRef;
pub use s3::S3ArtifactBlobStore;

use super::Digest;
use axum::http::StatusCode;
use fc_platform_core::shared::error::PlatformError;
use fc_platform_core::usecase::UseCaseError;
use std::env;
use std::path::PathBuf;
use std::string::FromUtf8Error;
use tokio::io::AsyncRead;

/// The upload cap: 256 MiB, the same constant the fetch side uses (Java
/// `ArtifactStoreSupport.defaultMaxBytes`).
pub const MAX_BYTES: u64 = 256 * 1024 * 1024;

/// A blob's bytes, as [`ArtifactBlobStore::open`] streams them.
pub type ArtifactStream = Pin<Box<dyn AsyncRead + Send>>;

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

    /// Where the store lives, for diagnostics only (Go `Describer`). Never
    /// credentials.
    fn describe(&self) -> StoreLocation {
        StoreLocation::default()
    }
}

/// Where a store lives: the kind (`s3` or `file`), its bucket (S3 only) and
/// its prefix (S3) or directory (file).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoreLocation {
    pub kind: &'static str,
    pub bucket: String,
    pub prefix: String,
}

/// A store whose every transport failure says which store, which operation
/// and which digest it was (Go `WithDiagnostics`), so a 500's log line alone
/// diagnoses it. The cause is kept as the SDK worded it. `NotFound` and
/// `BadRef` pass through untouched: callers match on them. No credentials, no
/// presigned URLs.
pub struct DiagnosedBlobStore {
    inner: Arc<dyn ArtifactBlobStore>,
    location: StoreLocation,
}

impl DiagnosedBlobStore {
    pub fn new(inner: Arc<dyn ArtifactBlobStore>) -> Self {
        let location = inner.describe();
        Self { inner, location }
    }

    fn wrap<T>(
        &self,
        op: &str,
        digest: Option<&Digest>,
        result: Result<T, ArtifactError>,
    ) -> Result<T, ArtifactError> {
        match result {
            Err(ArtifactError::Transport(cause)) => {
                let mut loc = format!("store={}", self.location.kind);
                if !self.location.bucket.is_empty() {
                    loc.push_str(&format!(" bucket={}", self.location.bucket));
                }
                if !self.location.prefix.is_empty() {
                    loc.push_str(&format!(" prefix={}", self.location.prefix));
                }
                let digest = digest.map(|d| d.hex().to_string()).unwrap_or_default();
                Err(ArtifactError::Transport(format!(
                    "artifact store {loc} op={op} digest={digest}: {cause}"
                )))
            }
            other => other,
        }
    }
}

#[async_trait]
impl ArtifactBlobStore for DiagnosedBlobStore {
    async fn put(
        &self,
        function_id: &str,
        digest: &Digest,
        file: &Path,
    ) -> Result<(), ArtifactError> {
        self.wrap(
            "put",
            Some(digest),
            self.inner.put(function_id, digest, file).await,
        )
    }

    async fn exists(&self, function_id: &str, digest: &Digest) -> Result<bool, ArtifactError> {
        self.wrap(
            "exists",
            Some(digest),
            self.inner.exists(function_id, digest).await,
        )
    }

    async fn open(
        &self,
        function_id: &str,
        digest: &Digest,
    ) -> Result<ArtifactStream, ArtifactError> {
        self.wrap(
            "open",
            Some(digest),
            self.inner.open(function_id, digest).await,
        )
    }

    async fn size(&self, function_id: &str, digest: &Digest) -> Result<u64, ArtifactError> {
        self.wrap(
            "size",
            Some(digest),
            self.inner.size(function_id, digest).await,
        )
    }

    async fn delete_all(&self, function_id: &str) -> Result<(), ArtifactError> {
        self.wrap("delete", None, self.inner.delete_all(function_id).await)
    }

    fn describe(&self) -> StoreLocation {
        self.location.clone()
    }
}

/// Why `FC_FN_ARTIFACT_STORE` cannot be turned into a store. `Display` is the
/// text the start-up refusal printed before this type.
#[derive(Debug, thiserror::Error)]
pub enum ArtifactStoreSpecError {
    #[error("FC_FN_ARTIFACT_STORE must be file:///abs/dir or s3://bucket[/prefix], got: {spec}")]
    Unrecognised { spec: String },
    #[error("FC_FN_ARTIFACT_STORE file:// must not carry a host: {spec}")]
    FileHost { spec: String },
    #[error("FC_FN_ARTIFACT_STORE file:// path is not valid UTF-8: {spec}")]
    FilePathNotUtf8 {
        spec: String,
        #[source]
        source: FromUtf8Error,
    },
    #[error("FC_FN_ARTIFACT_STORE file:// must be an absolute path: {spec}")]
    FilePathNotAbsolute { spec: String },
    #[error("FC_FN_ARTIFACT_STORE s3:// must name a bucket: {spec}")]
    S3NoBucket { spec: String },
    #[error("FC_FN_ARTIFACT_STORE: {0}")]
    Directory(#[from] ArtifactDirError),
}

/// `FC_FN_ARTIFACT_STORE` (Java `ArtifactBlobStores.configure`): blank is
/// no store; `file:///abs/dir` and `s3://bucket[/prefix]` are the two
/// stores; anything else fails startup, naming the variable, never a silent
/// default.
pub fn configure(spec: &str) -> Result<Option<Arc<dyn ArtifactBlobStore>>, ArtifactStoreSpecError> {
    if super::java_is_blank(spec) {
        return Ok(None);
    }
    if let Some(rest) = spec.strip_prefix("file://") {
        let dir = file_dir(spec, rest)?;
        let store = FileArtifactBlobStore::new(dir)?;
        return Ok(Some(Arc::new(DiagnosedBlobStore::new(Arc::new(store)))));
    }
    if let Some(rest) = spec.strip_prefix("s3://") {
        let (bucket, prefix) = s3_location(spec, rest)?;
        // The startup self-check logs the effective S3 configuration and
        // whether the credential chain resolves. It runs in the background
        // and never fails startup: a bad chain is reported here and again,
        // with full detail, on the first request that needs S3.
        s3::spawn_startup_self_check(bucket.clone(), prefix.clone());
        let store = S3ArtifactBlobStore::from_environment(bucket, prefix);
        return Ok(Some(Arc::new(DiagnosedBlobStore::new(Arc::new(store)))));
    }
    Err(ArtifactStoreSpecError::Unrecognised {
        spec: spec.to_string(),
    })
}

/// The store `FC_FN_ARTIFACT_STORE` names (Java `Env.java:609`, no default;
/// fc-dev sets its own).
pub fn store_from_env() -> Result<Option<Arc<dyn ArtifactBlobStore>>, ArtifactStoreSpecError> {
    configure(&env::var("FC_FN_ARTIFACT_STORE").unwrap_or_default())
}

/// The publish-time signature policy (Java `Env.java:550,606-607` and
/// `Signatures.resolve`): `FC_FN_SIGNATURES` (`required` unless spelled
/// `off`), which is refused outside `FLOWCATALYST_DEV_MODE`, and
/// `FC_FN_TRUST_ROOT` (blank: the committed Sigstore public-good root).
pub fn signatures_from_env() -> Result<fc_function_signing::Signatures, String> {
    let var = |key: &str| env::var(key).unwrap_or_default();
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
fn file_dir(spec: &str, rest: &str) -> Result<PathBuf, ArtifactStoreSpecError> {
    let (host, path) = match rest.find('/') {
        Some(slash) => (&rest[..slash], &rest[slash..]),
        None => (rest, ""),
    };
    if !host.is_empty() {
        return Err(ArtifactStoreSpecError::FileHost {
            spec: spec.to_string(),
        });
    }
    let path =
        urlencoding::decode(path).map_err(|source| ArtifactStoreSpecError::FilePathNotUtf8 {
            spec: spec.to_string(),
            source,
        })?;
    if path.is_empty() || !path.starts_with('/') {
        return Err(ArtifactStoreSpecError::FilePathNotAbsolute {
            spec: spec.to_string(),
        });
    }
    // `file:///C:/dir` names `C:/dir` on Windows.
    #[cfg(windows)]
    if path.as_bytes().get(2) == Some(&b':') {
        return Ok(std::path::PathBuf::from(&path[1..]));
    }
    Ok(PathBuf::from(path.into_owned()))
}

/// `s3://bucket[/prefix]`: the bucket is the URI's host (Java reads
/// `URI.getHost`, which is absent for anything but a hostname), the prefix
/// the path with its slashes trimmed.
fn s3_location(spec: &str, rest: &str) -> Result<(String, String), ArtifactStoreSpecError> {
    let (bucket, path) = match rest.find('/') {
        Some(slash) => (&rest[..slash], &rest[slash..]),
        None => (rest, ""),
    };
    let hostname = !bucket.is_empty()
        && bucket
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.');
    if !hostname {
        return Err(ArtifactStoreSpecError::S3NoBucket {
            spec: spec.to_string(),
        });
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
pub fn too_large() -> PlatformError {
    PlatformError::Coded {
        status: StatusCode::PAYLOAD_TOO_LARGE,
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

/// What an uploaded blob is, from its first bytes (at most
/// [`ArtifactKind::PREFIX_LEN`](super::ArtifactKind::PREFIX_LEN); the rest
/// is never read).
pub async fn sniff(
    store: &dyn ArtifactBlobStore,
    function_id: &str,
    digest: &Digest,
) -> Result<super::ArtifactKind, ArtifactError> {
    use tokio::io::AsyncReadExt;
    let mut stream = store.open(function_id, digest).await?;
    let mut prefix = Vec::with_capacity(super::ArtifactKind::PREFIX_LEN);
    (&mut stream)
        .take(super::ArtifactKind::PREFIX_LEN as u64)
        .read_to_end(&mut prefix)
        .await
        .map_err(|e| ArtifactError::Transport(e.to_string()))?;
    Ok(super::ArtifactKind::sniff(&prefix))
}

/// `422 ARTIFACT_RUNTIME_MISMATCH` (beyond Java): the runtime needs a WASI
/// component (`component`) or a JavaScript bundle (`js`), and the uploaded
/// artifact is something else.
pub fn runtime_mismatch(runtime: super::Runtime, found: super::ArtifactKind) -> UseCaseError {
    let needs = if runtime.requires_js_bundle() {
        "a JavaScript bundle (a UTF-8 ES module)"
    } else {
        "a WASI 0.2 component"
    };
    UseCaseError::unprocessable(
        "ARTIFACT_RUNTIME_MISMATCH",
        format!(
            "runtime '{}' needs {needs}; the uploaded artifact is {}",
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
    use fc_platform_core::shared::error::PlatformError;
    use fc_platform_core::shared::tsid;
    use std::env;
    use std::fs;

    #[test]
    fn unset_or_blank_is_no_store() {
        assert!(configure("").unwrap().is_none());
        assert!(configure("   ").unwrap().is_none());
    }

    #[tokio::test]
    async fn a_file_path_with_a_space_is_accepted_literally_and_percent_encoded() {
        let base = env::temp_dir().join(format!("fc-blob-cfg-{}", tsid::generate_untyped()));
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
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn an_unrecognised_scheme_fails_startup_naming_the_variable() {
        let err = configure("ftp://x").err().unwrap().to_string();
        assert!(err.contains("FC_FN_ARTIFACT_STORE"), "{err}");
        assert!(err.contains("ftp://x"), "{err}");
    }

    #[test]
    fn a_file_uri_with_a_host_or_a_relative_path_is_rejected() {
        let err = configure("file://host/some/path")
            .err()
            .unwrap()
            .to_string();
        assert!(err.contains("FC_FN_ARTIFACT_STORE"), "{err}");
        assert!(configure("file://relative/path").is_err());
        assert!(configure("file://").is_err());
    }

    // The expected strings are the `format!` texts the start-up refusal
    // printed before it had an error type.
    #[test]
    fn the_refusal_texts_are_unchanged() {
        assert_eq!(
            configure("ftp://x").err().unwrap().to_string(),
            "FC_FN_ARTIFACT_STORE must be file:///abs/dir or s3://bucket[/prefix], got: ftp://x"
        );
        assert_eq!(
            configure("file://host/some/path")
                .err()
                .unwrap()
                .to_string(),
            "FC_FN_ARTIFACT_STORE file:// must not carry a host: file://host/some/path"
        );
        assert_eq!(
            configure("file://").err().unwrap().to_string(),
            "FC_FN_ARTIFACT_STORE file:// must be an absolute path: file://"
        );
        assert_eq!(
            configure("s3:///no-bucket").err().unwrap().to_string(),
            "FC_FN_ARTIFACT_STORE s3:// must name a bucket: s3:///no-bucket"
        );
        let dir = env::temp_dir().join(format!("fc-blob-cfg-{}", tsid::generate_untyped()));
        fs::write(&dir, b"a file where the directory should go").unwrap();
        let err = configure(&format!("file://{}", dir.display()))
            .err()
            .unwrap();
        assert_eq!(
            err.to_string(),
            format!(
                "FC_FN_ARTIFACT_STORE: cannot create artifact directory {}: {}",
                dir.display(),
                fs::create_dir_all(&dir).unwrap_err()
            )
        );
        fs::remove_file(&dir).unwrap();
    }

    #[test]
    fn an_s3_uri_with_no_bucket_is_rejected() {
        let err = configure("s3:///no-bucket").err().unwrap().to_string();
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
        let err = too_large();
        let PlatformError::Coded {
            status,
            code,
            message,
            ..
        } = &err
        else {
            panic!("{err:?}");
        };
        assert_eq!(status.as_u16(), 413);
        assert_eq!(code, "ARTIFACT_TOO_LARGE");
        assert_eq!(message, "artifact exceeds the 268435456-byte limit");
    }

    /// A store that always fails with `result`.
    struct Failing(ArtifactError);

    #[async_trait]
    impl ArtifactBlobStore for Failing {
        async fn put(&self, _: &str, _: &Digest, _: &Path) -> Result<(), ArtifactError> {
            Err(self.0.clone())
        }
        async fn exists(&self, _: &str, _: &Digest) -> Result<bool, ArtifactError> {
            Err(self.0.clone())
        }
        async fn open(&self, _: &str, _: &Digest) -> Result<ArtifactStream, ArtifactError> {
            Err(self.0.clone())
        }
        async fn size(&self, _: &str, _: &Digest) -> Result<u64, ArtifactError> {
            Err(self.0.clone())
        }
        async fn delete_all(&self, _: &str) -> Result<(), ArtifactError> {
            Err(self.0.clone())
        }
        fn describe(&self) -> StoreLocation {
            StoreLocation {
                kind: "s3",
                bucket: "my-bucket".into(),
                prefix: "fn-artifacts".into(),
            }
        }
    }

    /// Go `diag_test.go`: a transport failure names the store, the operation
    /// and the digest, keeps the cause; the sentinels pass through untouched.
    #[tokio::test]
    async fn a_transport_failure_names_the_store_the_operation_and_the_digest() {
        let d = Digest::parse(&format!("sha256:{}", "c".repeat(64))).unwrap();
        let wrapped = DiagnosedBlobStore::new(Arc::new(Failing(ArtifactError::Transport(
            "s3 head_object: boom".into(),
        ))));
        let ArtifactError::Transport(msg) = wrapped.exists("fn1", &d).await.unwrap_err() else {
            panic!("a transport failure stays one");
        };
        for want in [
            "store=s3",
            "bucket=my-bucket",
            "prefix=fn-artifacts",
            "op=exists",
            &format!("digest={}", "c".repeat(64)),
            "s3 head_object: boom",
        ] {
            assert!(msg.contains(want), "{want:?} missing from {msg:?}");
        }
        assert_eq!(wrapped.describe().bucket, "my-bucket");

        for sentinel in [ArtifactError::NotFound, ArtifactError::BadRef("x".into())] {
            let store = DiagnosedBlobStore::new(Arc::new(Failing(sentinel.clone())));
            assert_eq!(store.size("fn1", &d).await.unwrap_err(), sentinel);
        }
    }
}
