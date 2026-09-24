//! The content-addressed artifact cache every store shares (Java
//! `ArtifactStoreSupport`).

use std::io;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use bytes::Bytes;
use sha2::{Digest as _, Sha256};
use tokio::io::AsyncWriteExt;

use super::{ArtifactError, Fetched, Source};
use crate::digest::Digest;

/// 256 MiB.
pub const DEFAULT_MAX_BYTES: u64 = 256 * 1024 * 1024;

const BUFFER_SIZE: usize = 64 * 1024;

/// A source's byte stream, read chunk by chunk.
#[async_trait]
pub trait BodyReader: Send {
    /// The next chunk, or `None` at the end.
    async fn next_chunk(&mut self) -> io::Result<Option<Bytes>>;
}

/// A fetch source: the body and, when the transport declared one, its
/// length, checked against the cap before a byte is read.
pub struct SourceStream {
    pub body: Box<dyn BodyReader>,
    pub content_length: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct ArtifactCache {
    dir: PathBuf,
    max_bytes: u64,
}

impl ArtifactCache {
    /// Creates `<dir>/sha256` up front, as Java's store constructors do.
    pub fn new(dir: impl Into<PathBuf>, max_bytes: u64) -> io::Result<Self> {
        let dir = dir.into();
        assert!(max_bytes > 0, "max_bytes must be positive");
        std::fs::create_dir_all(dir.join("sha256"))?;
        Ok(Self { dir, max_bytes })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn path_for(&self, digest: &Digest) -> PathBuf {
        self.dir.join("sha256").join(digest.hex())
    }

    /// A cached copy is re-hashed, never trusted, and replaced when it no
    /// longer matches; otherwise the source is streamed through a running
    /// sha256 into a temp file that is moved into place only on a match.
    pub async fn fetch(
        &self,
        source: &dyn Source,
        artifact_ref: &str,
        expected: &Digest,
        version_id: Option<&str>,
    ) -> Result<Fetched, ArtifactError> {
        let cached = self.path_for(expected);
        if tokio::fs::metadata(&cached)
            .await
            .is_ok_and(|m| m.is_file())
        {
            let (actual, size) = hash_file(cached.clone()).await?;
            if &actual == expected {
                return Ok(Fetched {
                    file: cached,
                    bytes: size,
                });
            }
            tracing::warn!(
                digest = %expected,
                actual = %actual,
                "cached artifact no longer matches its digest; replacing it"
            );
            let _ = tokio::fs::remove_file(&cached).await;
        }
        self.download(source, artifact_ref, expected, version_id, cached)
            .await
    }

    async fn download(
        &self,
        source: &dyn Source,
        artifact_ref: &str,
        expected: &Digest,
        version_id: Option<&str>,
        target: PathBuf,
    ) -> Result<Fetched, ArtifactError> {
        let mut stream = source.open(artifact_ref, expected, version_id).await?;
        if stream
            .content_length
            .is_some_and(|len| len > self.max_bytes)
        {
            return Err(ArtifactError::TooLarge {
                limit: self.max_bytes,
            });
        }
        let temp = self
            .dir
            .join(format!("artifact-{}.tmp", rand::random::<u64>()));
        let result = self.write_verified(&mut stream, &temp, expected).await;
        let result = match result {
            Ok(count) => tokio::fs::rename(&temp, &target)
                .await
                .map(|()| Fetched {
                    file: target,
                    bytes: count,
                })
                .map_err(ArtifactError::transport),
            Err(e) => Err(e),
        };
        if result.is_err() {
            let _ = tokio::fs::remove_file(&temp).await;
        }
        result
    }

    /// Aborts as soon as the byte count passes the cap, checked on every
    /// chunk, so a source that streams for ever is still cut off.
    async fn write_verified(
        &self,
        stream: &mut SourceStream,
        temp: &Path,
        expected: &Digest,
    ) -> Result<u64, ArtifactError> {
        let mut file = tokio::fs::File::create(temp)
            .await
            .map_err(ArtifactError::transport)?;
        let mut hasher = Sha256::new();
        let mut count: u64 = 0;
        while let Some(chunk) = stream
            .body
            .next_chunk()
            .await
            .map_err(ArtifactError::transport)?
        {
            count += chunk.len() as u64;
            if count > self.max_bytes {
                return Err(ArtifactError::TooLarge {
                    limit: self.max_bytes,
                });
            }
            hasher.update(&chunk);
            file.write_all(&chunk)
                .await
                .map_err(ArtifactError::transport)?;
        }
        file.flush().await.map_err(ArtifactError::transport)?;
        file.sync_all().await.map_err(ArtifactError::transport)?;
        let actual = Digest::from_sha256(&hasher.finalize().into());
        if &actual != expected {
            return Err(ArtifactError::DigestMismatch {
                expected: expected.clone(),
                actual,
            });
        }
        Ok(count)
    }
}

async fn hash_file(path: PathBuf) -> Result<(Digest, u64), ArtifactError> {
    tokio::task::spawn_blocking(move || -> io::Result<(Digest, u64)> {
        use std::io::Read;
        let mut file = std::fs::File::open(&path)?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; BUFFER_SIZE];
        let mut size = 0u64;
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            size += n as u64;
            hasher.update(&buffer[..n]);
        }
        Ok((Digest::from_sha256(&hasher.finalize().into()), size))
    })
    .await
    .map_err(ArtifactError::transport)?
    .map_err(ArtifactError::transport)
}

/// A local file, read in fixed-size chunks.
pub(crate) struct FileBody(pub(crate) tokio::fs::File);

#[async_trait]
impl BodyReader for FileBody {
    async fn next_chunk(&mut self) -> io::Result<Option<Bytes>> {
        use tokio::io::AsyncReadExt;
        let mut buffer = vec![0u8; BUFFER_SIZE];
        let n = self.0.read(&mut buffer).await?;
        if n == 0 {
            return Ok(None);
        }
        buffer.truncate(n);
        Ok(Some(Bytes::from(buffer)))
    }
}

/// How long an HTTP body may stall between chunks. Java bounds only the
/// wait for response headers; a stalled body would block a prepare (and so
/// the reconcile) for ever, so the Rust host also bounds the gap between
/// chunks.
pub(crate) const BODY_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// An HTTP response body.
pub(crate) struct HttpBody(pub(crate) reqwest::Response);

#[async_trait]
impl BodyReader for HttpBody {
    async fn next_chunk(&mut self) -> io::Result<Option<Bytes>> {
        match tokio::time::timeout(BODY_IDLE_TIMEOUT, self.0.chunk()).await {
            Ok(Ok(chunk)) => Ok(chunk),
            Ok(Err(e)) => Err(io::Error::other(e)),
            Err(_) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "artifact body stalled",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Bytes1(Vec<Bytes>);

    #[async_trait]
    impl BodyReader for Bytes1 {
        async fn next_chunk(&mut self) -> io::Result<Option<Bytes>> {
            Ok(if self.0.is_empty() {
                None
            } else {
                Some(self.0.remove(0))
            })
        }
    }

    struct Memory {
        content: Vec<u8>,
        declared: Option<u64>,
        opens: AtomicUsize,
    }

    #[async_trait]
    impl Source for Memory {
        async fn open(
            &self,
            _: &str,
            _: &Digest,
            _: Option<&str>,
        ) -> Result<SourceStream, ArtifactError> {
            self.opens.fetch_add(1, Ordering::SeqCst);
            let chunks = self.content.chunks(3).map(Bytes::copy_from_slice).collect();
            Ok(SourceStream {
                body: Box::new(Bytes1(chunks)),
                content_length: self.declared,
            })
        }
    }

    fn memory(content: &[u8]) -> Memory {
        Memory {
            content: content.to_vec(),
            declared: None,
            opens: AtomicUsize::new(0),
        }
    }

    fn digest(content: &[u8]) -> Digest {
        Digest::from_sha256(&Sha256::digest(content).into())
    }

    fn leftovers(dir: &Path) -> usize {
        std::fs::read_dir(dir)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".tmp")
            })
            .count()
    }

    #[tokio::test]
    async fn fetch_caches_under_the_hex_and_reuses_after_rehash() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ArtifactCache::new(dir.path(), DEFAULT_MAX_BYTES).unwrap();
        let source = memory(b"hello artifact");
        let d = digest(b"hello artifact");
        let fetched = cache.fetch(&source, "mem://x", &d, None).await.unwrap();
        assert_eq!(fetched.file, dir.path().join("sha256").join(d.hex()));
        assert_eq!(fetched.bytes, 14);
        assert_eq!(std::fs::read(&fetched.file).unwrap(), b"hello artifact");
        cache.fetch(&source, "mem://x", &d, None).await.unwrap();
        assert_eq!(
            source.opens.load(Ordering::SeqCst),
            1,
            "a good cached copy is not refetched"
        );
    }

    #[tokio::test]
    async fn a_poisoned_cache_entry_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ArtifactCache::new(dir.path(), DEFAULT_MAX_BYTES).unwrap();
        let d = digest(b"good bytes");
        std::fs::write(cache.path_for(&d), b"poison").unwrap();
        let source = memory(b"good bytes");
        let fetched = cache.fetch(&source, "mem://x", &d, None).await.unwrap();
        assert_eq!(std::fs::read(fetched.file).unwrap(), b"good bytes");
        assert_eq!(source.opens.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_digest_mismatch_leaves_nothing_under_the_digest() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ArtifactCache::new(dir.path(), DEFAULT_MAX_BYTES).unwrap();
        let d = digest(b"expected");
        let err = cache
            .fetch(&memory(b"something else"), "mem://x", &d, None)
            .await
            .unwrap_err();
        assert!(matches!(err, ArtifactError::DigestMismatch { .. }));
        assert_eq!(err.simple_name(), "DigestMismatch");
        assert!(!cache.path_for(&d).exists());
        assert_eq!(leftovers(dir.path()), 0);
    }

    #[tokio::test]
    async fn the_cap_applies_to_declared_and_streamed_lengths() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ArtifactCache::new(dir.path(), 10).unwrap();
        let content = b"more than ten bytes";
        let d = digest(content);
        let err = cache
            .fetch(&memory(content), "mem://x", &d, None)
            .await
            .unwrap_err();
        assert_eq!(err, ArtifactError::TooLarge { limit: 10 });
        let mut declared = memory(b"short");
        declared.declared = Some(11);
        let err = cache
            .fetch(&declared, "mem://x", &digest(b"short"), None)
            .await
            .unwrap_err();
        assert_eq!(err, ArtifactError::TooLarge { limit: 10 });
        assert_eq!(leftovers(dir.path()), 0);
    }
}
