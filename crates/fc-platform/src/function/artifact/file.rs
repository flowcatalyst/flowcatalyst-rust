//! Java `FileArtifactBlobStore`: `FC_FN_ARTIFACT_STORE=file:///abs/dir`,
//! layout `<dir>/<functionId>/<hex>`. A temp file in the same directory is
//! renamed into place, so a reader never sees a partial blob.

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use super::{keys, ArtifactBlobStore, ArtifactError, ArtifactStream};
use crate::function::Digest;

#[derive(Debug)]
pub struct FileArtifactBlobStore {
    dir: PathBuf,
}

fn transport(e: std::io::Error) -> ArtifactError {
    ArtifactError::Transport(e.to_string())
}

impl FileArtifactBlobStore {
    /// Creates `dir` when missing; a directory that cannot be created fails
    /// startup.
    pub fn new(dir: PathBuf) -> Result<FileArtifactBlobStore, String> {
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("cannot create artifact directory {}: {e}", dir.display()))?;
        Ok(FileArtifactBlobStore { dir })
    }

    /// The directory this store writes under.
    pub fn root(&self) -> &Path {
        &self.dir
    }

    fn path_of(&self, function_id: &str, digest: &Digest) -> Result<PathBuf, ArtifactError> {
        let k = keys::of(function_id, digest)?;
        Ok(self.dir.join(k.function_id).join(k.hex))
    }
}

async fn is_file(path: &Path) -> bool {
    tokio::fs::metadata(path)
        .await
        .map(|m| m.is_file())
        .unwrap_or(false)
}

#[async_trait]
impl ArtifactBlobStore for FileArtifactBlobStore {
    async fn put(
        &self,
        function_id: &str,
        digest: &Digest,
        file: &Path,
    ) -> Result<(), ArtifactError> {
        let target = self.path_of(function_id, digest)?;
        // Idempotent: an existing blob is left exactly as it is.
        if is_file(&target).await {
            return Ok(());
        }
        let parent = target.parent().expect("a blob path has a parent");
        tokio::fs::create_dir_all(parent).await.map_err(transport)?;
        let temp = parent.join(format!(
            "upload-{}.tmp",
            crate::shared::tsid::generate_untyped()
        ));
        let result = async {
            tokio::fs::copy(file, &temp).await?;
            tokio::fs::rename(&temp, &target).await
        }
        .await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(&temp).await;
        }
        result.map_err(transport)
    }

    async fn exists(&self, function_id: &str, digest: &Digest) -> Result<bool, ArtifactError> {
        Ok(is_file(&self.path_of(function_id, digest)?).await)
    }

    async fn open(
        &self,
        function_id: &str,
        digest: &Digest,
    ) -> Result<ArtifactStream, ArtifactError> {
        let path = self.path_of(function_id, digest)?;
        if !is_file(&path).await {
            return Err(ArtifactError::NotFound);
        }
        let file = tokio::fs::File::open(&path).await.map_err(transport)?;
        Ok(Box::pin(file))
    }

    async fn size(&self, function_id: &str, digest: &Digest) -> Result<u64, ArtifactError> {
        match tokio::fs::metadata(self.path_of(function_id, digest)?).await {
            Ok(m) => Ok(m.len()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(ArtifactError::NotFound),
            Err(e) => Err(transport(e)),
        }
    }

    async fn delete_all(&self, function_id: &str) -> Result<(), ArtifactError> {
        let dir = self.dir.join(keys::validate_function_id(function_id)?);
        match tokio::fs::remove_dir_all(&dir).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(transport(e)),
        }
    }
}

/// Java `FileArtifactBlobStoreTest`.
#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> TempDir {
            let dir = std::env::temp_dir().join(format!(
                "fc-blob-file-{}",
                crate::shared::tsid::generate_untyped()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn digest(c: char) -> Digest {
        Digest::parse(&format!("sha256:{}", c.to_string().repeat(64))).unwrap()
    }

    fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, text).unwrap();
        p
    }

    async fn read(store: &FileArtifactBlobStore, id: &str, d: &Digest) -> String {
        let mut s = String::new();
        store
            .open(id, d)
            .await
            .unwrap()
            .read_to_string(&mut s)
            .await
            .unwrap();
        s
    }

    #[tokio::test]
    async fn put_then_open_then_size_round_trip() {
        let tmp = TempDir::new();
        let store = FileArtifactBlobStore::new(tmp.0.join("store")).unwrap();
        let file = write(&tmp.0, "src.bin", "hello world");
        store.put("fn1", &digest('a'), &file).await.unwrap();
        assert!(store.exists("fn1", &digest('a')).await.unwrap());
        assert_eq!(store.size("fn1", &digest('a')).await.unwrap(), 11);
        assert_eq!(read(&store, "fn1", &digest('a')).await, "hello world");
        // Layout: <dir>/<functionId>/<hex>.
        assert!(store.root().join("fn1").join("a".repeat(64)).is_file());
        // No temp file left beside it.
        let names: Vec<_> = std::fs::read_dir(store.root().join("fn1"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 1, "{names:?}");
    }

    #[tokio::test]
    async fn put_is_idempotent_an_existing_blob_is_left_as_is() {
        let tmp = TempDir::new();
        let store = FileArtifactBlobStore::new(tmp.0.join("store")).unwrap();
        let first = write(&tmp.0, "first.bin", "first");
        let second = write(&tmp.0, "second.bin", "second-but-never-written");
        store.put("fn1", &digest('a'), &first).await.unwrap();
        store.put("fn1", &digest('a'), &second).await.unwrap();
        assert_eq!(read(&store, "fn1", &digest('a')).await, "first");
    }

    #[tokio::test]
    async fn open_and_size_of_an_absent_blob_are_not_found() {
        let tmp = TempDir::new();
        let store = FileArtifactBlobStore::new(tmp.0.join("store")).unwrap();
        assert!(matches!(
            store.open("fn1", &digest('a')).await,
            Err(ArtifactError::NotFound)
        ));
        assert_eq!(
            store.size("fn1", &digest('a')).await.unwrap_err(),
            ArtifactError::NotFound
        );
        assert!(!store.exists("fn1", &digest('a')).await.unwrap());
    }

    #[tokio::test]
    async fn delete_all_removes_every_blob_of_the_function_and_leaves_others_alone() {
        let tmp = TempDir::new();
        let store = FileArtifactBlobStore::new(tmp.0.join("store")).unwrap();
        let a = write(&tmp.0, "a.bin", "a");
        let b = write(&tmp.0, "b.bin", "b");
        store.put("fn1", &digest('a'), &a).await.unwrap();
        store.put("fn1", &digest('b'), &b).await.unwrap();
        store.put("fn2", &digest('a'), &a).await.unwrap();
        store.delete_all("fn1").await.unwrap();
        assert!(!store.exists("fn1", &digest('a')).await.unwrap());
        assert!(!store.exists("fn1", &digest('b')).await.unwrap());
        assert!(store.exists("fn2", &digest('a')).await.unwrap());
        // A function with no blobs is a no-op.
        store.delete_all("neverUploaded").await.unwrap();
    }

    /// U9: the store validates its keys itself.
    #[tokio::test]
    async fn the_store_rejects_a_path_traversal_id_and_a_short_hex() {
        let tmp = TempDir::new();
        let store = FileArtifactBlobStore::new(tmp.0.join("store")).unwrap();
        let file = write(&tmp.0, "src.bin", "x");
        assert!(matches!(
            store.put("../x", &digest('a'), &file).await,
            Err(ArtifactError::BadRef(_))
        ));
        let short = Digest::unchecked(&format!("sha256:{}", "a".repeat(63)));
        assert!(matches!(
            store.exists("fn1", &short).await,
            Err(ArtifactError::BadRef(_))
        ));
        assert!(matches!(
            store.delete_all("../x").await,
            Err(ArtifactError::BadRef(_))
        ));
    }
}
