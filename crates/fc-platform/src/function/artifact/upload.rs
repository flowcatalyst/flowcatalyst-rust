//! Receiving an uploaded artifact (Java `FunctionApi.uploadArtifact`,
//! `FunctionApi.java:327-375`, after its store, permission, reach and
//! digest-shape checks): the declared length, then the body streamed
//! through SHA-256 into a temp file, never buffered, then the count, the
//! digest, and the store.

use std::path::{Path, PathBuf};

use axum::body::Bytes;
use futures::{Stream, StreamExt};
use sha2::{Digest as _, Sha256};
use tokio::io::AsyncWriteExt;

use super::{digest_mismatch, empty, too_large, ArtifactBlobStore, MAX_BYTES};
use crate::function::Digest;
use crate::shared::error::PlatformError;
use crate::usecase::UseCaseError;

/// What an accepted upload stored: its byte count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Received {
    pub bytes: u64,
}

/// The temp file, removed on every path, success included.
struct TempUpload(PathBuf);

impl Drop for TempUpload {
    fn drop(&mut self) {
        // Best effort: a leftover temp file is harmless and never served.
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Receives `body` for `(function_id, digest)` into `store`, with the
/// 256 MiB cap and a temp file in the system temp directory.
pub async fn receive<S, E>(
    store: &dyn ArtifactBlobStore,
    function_id: &str,
    digest: &Digest,
    declared_length: Option<u64>,
    body: S,
) -> Result<Received, PlatformError>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
    E: std::fmt::Display,
{
    receive_into(
        store,
        function_id,
        digest,
        declared_length,
        body,
        MAX_BYTES,
        &std::env::temp_dir(),
    )
    .await
}

/// [`receive`] with the cap and the temp directory given. In order: a
/// declared length over the cap (413) before a byte is read; the running
/// count passing it mid-stream (413); an empty body (422); bytes that do
/// not hash to `digest` (422), nothing stored; else the store's idempotent
/// `put`. An upload of a digest already stored is still read and hashed:
/// the route never answers for bytes it did not see.
pub(crate) async fn receive_into<S, E>(
    store: &dyn ArtifactBlobStore,
    function_id: &str,
    digest: &Digest,
    declared_length: Option<u64>,
    mut body: S,
    max_bytes: u64,
    temp_dir: &Path,
) -> Result<Received, PlatformError>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
    E: std::fmt::Display,
{
    if declared_length.is_some_and(|n| n > max_bytes) {
        return Err(too_large());
    }
    let temp = TempUpload(temp_dir.join(format!(
        "fc-artifact-upload-{}.tmp",
        crate::shared::tsid::generate_untyped()
    )));
    let io = |e: std::io::Error| {
        tracing::error!(error = %e, "writing an uploaded artifact failed");
        PlatformError::internal(format!("reading the uploaded artifact: {e}"))
    };
    let mut file = tokio::fs::File::create(&temp.0).await.map_err(io)?;
    let mut sha256 = Sha256::new();
    let mut count: u64 = 0;
    while let Some(chunk) = body.next().await {
        let chunk = chunk
            .map_err(|e| PlatformError::internal(format!("reading the uploaded artifact: {e}")))?;
        count += chunk.len() as u64;
        if count > max_bytes {
            return Err(too_large());
        }
        sha256.update(&chunk);
        file.write_all(&chunk).await.map_err(io)?;
    }
    file.flush().await.map_err(io)?;
    drop(file);
    if count == 0 {
        return Err(empty().into());
    }
    let actual = Digest::from_sha256(&sha256.finalize());
    if &actual != digest {
        return Err(digest_mismatch(digest, &actual).into());
    }
    store.put(function_id, digest, &temp.0).await.map_err(|e| {
        tracing::error!(%function_id, error = %e, "storing an uploaded artifact failed");
        PlatformError::from(UseCaseError::internal(
            "ARTIFACT_STORE_ERROR",
            "storing the uploaded artifact failed",
        ))
    })?;
    Ok(Received { bytes: count })
}

/// Java `FunctionArtifactUploadApiTest`'s body-side cases (U2, U3, the
/// empty body and idempotence); the route-side ones need a database.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::function::artifact::FileArtifactBlobStore;
    use futures::stream;

    struct Dirs {
        base: PathBuf,
        store: FileArtifactBlobStore,
        temp: PathBuf,
    }

    impl Dirs {
        fn new() -> Dirs {
            let base = std::env::temp_dir().join(format!(
                "fc-upload-{}",
                crate::shared::tsid::generate_untyped()
            ));
            let temp = base.join("tmp");
            std::fs::create_dir_all(&temp).unwrap();
            let store = FileArtifactBlobStore::new(base.join("store")).unwrap();
            Dirs { base, store, temp }
        }

        fn temp_files(&self) -> usize {
            std::fs::read_dir(&self.temp).unwrap().count()
        }
    }

    impl Drop for Dirs {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }

    type Chunk = Result<Bytes, std::convert::Infallible>;

    fn chunks(parts: &[&'static [u8]]) -> impl Stream<Item = Chunk> + Unpin {
        stream::iter(
            parts
                .iter()
                .map(|p| Ok(Bytes::from_static(p)))
                .collect::<Vec<_>>(),
        )
    }

    fn digest_of(bytes: &[u8]) -> Digest {
        Digest::from_sha256(&Sha256::digest(bytes))
    }

    fn status(e: &PlatformError) -> (u16, String) {
        match e {
            PlatformError::Coded { status, code, .. } => (status.as_u16(), code.clone()),
            other => panic!("{other:?}"),
        }
    }

    async fn run(
        d: &Dirs,
        digest: &Digest,
        declared: Option<u64>,
        body: impl Stream<Item = Chunk> + Unpin,
    ) -> Result<Received, PlatformError> {
        receive_into(&d.store, "fnc_1", digest, declared, body, 16, &d.temp).await
    }

    #[tokio::test]
    async fn a_body_is_hashed_as_written_stored_and_counted() {
        let d = Dirs::new();
        let digest = digest_of(b"hello world");
        let got = run(&d, &digest, Some(11), chunks(&[b"hello ", b"world"]))
            .await
            .unwrap();
        assert_eq!(got, Received { bytes: 11 });
        assert!(d.store.exists("fnc_1", &digest).await.unwrap());
        assert_eq!(d.temp_files(), 0, "the temp file is removed on success too");
        // Idempotent: the same bytes again are read, hashed and accepted.
        let again = run(&d, &digest, None, chunks(&[b"hello world"]))
            .await
            .unwrap();
        assert_eq!(again, got);
    }

    /// U3: over the cap both ways, nothing stored, no temp file left.
    #[tokio::test]
    async fn over_the_cap_is_413_declared_or_discovered_mid_stream() {
        let d = Dirs::new();
        let big: &'static [u8] = b"0123456789abcdefX";
        let digest = digest_of(big);
        // Declared: refused before the body is polled at all.
        let untouched =
            stream::poll_fn(|_| -> std::task::Poll<Option<Chunk>> { panic!("the body was read") });
        let err = run(&d, &digest, Some(17), untouched).await.unwrap_err();
        assert_eq!(status(&err), (413, "ARTIFACT_TOO_LARGE".into()));
        // Chunked, no length: the running count passes the cap.
        let err = run(&d, &digest, None, chunks(&[b"0123456789", b"abcdefX"]))
            .await
            .unwrap_err();
        assert_eq!(status(&err), (413, "ARTIFACT_TOO_LARGE".into()));
        assert!(!d.store.exists("fnc_1", &digest).await.unwrap());
        assert_eq!(d.temp_files(), 0);
        // Exactly the cap is fine.
        let at_cap: &'static [u8] = b"0123456789abcdef";
        assert!(run(&d, &digest_of(at_cap), Some(16), chunks(&[at_cap]))
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn an_empty_body_is_422_artifact_empty() {
        let d = Dirs::new();
        let err = run(&d, &digest_of(b""), Some(0), chunks(&[]))
            .await
            .unwrap_err();
        assert_eq!(status(&err), (422, "ARTIFACT_EMPTY".into()));
        assert_eq!(d.temp_files(), 0);
    }

    /// U2: a wrong digest is 422 and nothing is stored under it.
    #[tokio::test]
    async fn bytes_that_do_not_hash_to_the_digest_are_422_and_never_stored() {
        let d = Dirs::new();
        let claimed = digest_of(b"what the client said");
        let err = run(&d, &claimed, None, chunks(&[b"what it sent"]))
            .await
            .unwrap_err();
        assert_eq!(status(&err), (422, "DIGEST_MISMATCH".into()));
        match err {
            PlatformError::Coded { message, .. } => assert_eq!(
                message,
                format!(
                    "digest mismatch: expected {claimed} but got {}",
                    digest_of(b"what it sent")
                )
            ),
            other => panic!("{other:?}"),
        }
        assert!(!d.store.exists("fnc_1", &claimed).await.unwrap());
        assert_eq!(d.temp_files(), 0);
    }
}
