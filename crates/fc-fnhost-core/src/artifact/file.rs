//! `file:///abs/path/fn.wasm` (Java `FileArtifactStore`): relative paths,
//! a host component, and a path that is not a readable regular file are
//! rejected. The source may change under us, so it is always copied into
//! the cache like any other store rather than read in place.

use async_trait::async_trait;

use super::cache::FileBody;
use super::{percent_decode, ArtifactError, Source, SourceStream};
use crate::digest::Digest;

#[derive(Debug, Default, Clone)]
pub struct FileSource;

#[async_trait]
impl Source for FileSource {
    async fn open(
        &self,
        artifact_ref: &str,
        _: &Digest,
        _: Option<&str>,
    ) -> Result<SourceStream, ArtifactError> {
        let path = resolve(artifact_ref)?;
        let metadata = tokio::fs::metadata(&path)
            .await
            .map_err(|_| ArtifactError::NotFound)?;
        if !metadata.is_file() {
            return Err(ArtifactError::NotFound);
        }
        let file = tokio::fs::File::open(&path)
            .await
            .map_err(|_| ArtifactError::NotFound)?;
        Ok(SourceStream {
            body: Box::new(FileBody(file)),
            content_length: Some(metadata.len()),
        })
    }
}

/// `new URI(ref)` → scheme `file`, no host, an absolute (percent-decoded)
/// path. Query and fragment are not part of the path.
fn resolve(artifact_ref: &str) -> Result<std::path::PathBuf, ArtifactError> {
    let rest = artifact_ref
        .strip_prefix("file:")
        .ok_or_else(|| ArtifactError::BadRef("not a file:// reference".into()))?;
    let rest = rest.split(['#', '?']).next().unwrap_or("");
    let path = match rest.strip_prefix("//") {
        Some(after) => {
            let (authority, path) = after.split_at(after.find('/').unwrap_or(after.len()));
            if !authority.is_empty() {
                return Err(ArtifactError::BadRef(
                    "file:// references must not carry a host".into(),
                ));
            }
            path
        }
        None => rest,
    };
    if !path.starts_with('/') {
        return Err(ArtifactError::BadRef(
            "file:// references must be absolute".into(),
        ));
    }
    Ok(std::path::PathBuf::from(percent_decode(path)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifact::{ArtifactCache, ArtifactStore, ArtifactStores, DEFAULT_MAX_BYTES};
    use sha2::{Digest as _, Sha256};
    use std::sync::Arc;

    #[test]
    fn references_resolve_like_java_uri() {
        assert_eq!(
            resolve("file:///a/b%20c.wasm").unwrap(),
            std::path::PathBuf::from("/a/b c.wasm")
        );
        assert_eq!(
            resolve("file:/a/b").unwrap(),
            std::path::PathBuf::from("/a/b")
        );
        assert!(matches!(
            resolve("file://host/a"),
            Err(ArtifactError::BadRef(_))
        ));
        assert!(matches!(
            resolve("file:relative"),
            Err(ArtifactError::BadRef(_))
        ));
    }

    #[tokio::test]
    async fn fetches_a_file_through_the_cache() {
        let dir = tempfile::tempdir().unwrap();
        let artifact = dir.path().join("fn.wasm");
        std::fs::write(&artifact, b"\0asm module bytes").unwrap();
        let digest = Digest::from_sha256(&Sha256::digest(b"\0asm module bytes").into());
        let stores = ArtifactStores::new(
            ArtifactCache::new(dir.path().join("cache"), DEFAULT_MAX_BYTES).unwrap(),
        )
        .with_source("file", Arc::new(FileSource));
        let reference = format!("file://{}", artifact.display());
        let fetched = stores.fetch(&reference, &digest, None).await.unwrap();
        assert!(fetched
            .file
            .starts_with(dir.path().join("cache").join("sha256")));

        let other = Digest::from_sha256(&Sha256::digest(b"never cached").into());
        let missing = format!("file://{}", dir.path().join("nope.wasm").display());
        assert_eq!(
            stores.fetch(&missing, &other, None).await,
            Err(ArtifactError::NotFound)
        );
        assert_eq!(
            stores.fetch("s3://bucket/key", &digest, None).await,
            Err(ArtifactError::UnsupportedScheme("s3".into()))
        );
    }
}
