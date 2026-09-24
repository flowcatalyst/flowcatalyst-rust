//! Precompiled components (`.cwasm`), cached next to the artifact cache so a
//! restart, an idle reload or a promote of the same artifact loads in about
//! 0.2 ms instead of compiling again (15–45 ms; F0 §2.1).
//!
//! Layout: `<cache>/cwasm/<engine fingerprint>/<artifact sha256>.cwasm`,
//! plus `<…>.cwasm.sha256` holding the sha256 of the `.cwasm` bytes.
//!
//! # The invariant
//!
//! A `.cwasm` is native code that wasmtime maps and runs without verifying
//! it: `Component::deserialize_file` is `unsafe` for exactly that reason. So
//! this cache only ever deserializes a file it wrote itself:
//!
//! - the input is always an artifact the artifact cache has just verified
//!   against its sha256 digest, and the file is named by that digest;
//! - it is compiled by this host's own engine, and lives under that
//!   engine's [fingerprint](super::engine::fingerprint), so another wasmtime
//!   release or another codegen setting never looks at it;
//! - it is written to a temporary name and renamed into place, with a
//!   sidecar sha256 that is checked before every load, so a truncated or
//!   corrupted file is recompiled rather than mapped;
//! - the directory is created `0700`.
//!
//! The sidecar is an integrity check, not a defence against someone who can
//! write the cache directory: anyone with that access can already replace
//! the host's artifacts before they are verified, or the host binary. The
//! cache directory must be private to the host process.

use std::io::Write;
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};
use wasmtime::component::Component;
use wasmtime::Engine;

/// How a component was obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// A verified `.cwasm` was already there.
    Hit,
    /// Compiled now (and written for next time, when the cache is writable).
    Compiled,
    /// A `.cwasm` was there but failed its check or did not load: removed,
    /// compiled again and rewritten.
    Replaced,
}

pub struct CwasmCache {
    dir: PathBuf,
}

impl CwasmCache {
    /// `<cache_root>/cwasm/<fingerprint>`, created (`0700`) on first write.
    pub fn new(cache_root: &Path, fingerprint: &str) -> Self {
        Self {
            dir: cache_root.join("cwasm").join(fingerprint),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn path_for(&self, digest_hex: &str) -> PathBuf {
        self.dir.join(format!("{digest_hex}.cwasm"))
    }

    fn sum_path(&self, digest_hex: &str) -> PathBuf {
        self.dir.join(format!("{digest_hex}.cwasm.sha256"))
    }

    /// The component for the verified artifact `bytes` whose sha256 is
    /// `digest_hex`: from the cache when a good `.cwasm` is there, compiled
    /// (and cached) otherwise. `Err` is a compile failure.
    pub fn load(
        &self,
        engine: &Engine,
        digest_hex: &str,
        bytes: &[u8],
    ) -> Result<(Component, Source), String> {
        let path = self.path_for(digest_hex);
        let mut replaced = false;
        if path.exists() {
            match self.load_cached(engine, digest_hex, &path) {
                Ok(component) => return Ok((component, Source::Hit)),
                Err(why) => {
                    tracing::warn!(path = %path.display(), reason = %why, "discarding a cached .cwasm; compiling the component again");
                    let _ = std::fs::remove_file(&path);
                    let _ = std::fs::remove_file(self.sum_path(digest_hex));
                    replaced = true;
                }
            }
        }
        let compiled = engine
            .precompile_component(bytes)
            .map_err(|e| format!("does not compile: {e:#}"))?;
        let source = if replaced {
            Source::Replaced
        } else {
            Source::Compiled
        };
        match self.store(digest_hex, &compiled) {
            // SAFETY: the file was written just now by `store`, from bytes
            // this engine produced from a verified artifact (module docs).
            Ok(()) => match unsafe { Component::deserialize_file(engine, &path) } {
                Ok(component) => return Ok((component, source)),
                Err(e) => {
                    tracing::warn!(path = %path.display(), err = %format!("{e:#}"), "a freshly written .cwasm did not load; using the in-memory copy");
                }
            },
            Err(e) => {
                tracing::warn!(dir = %self.dir.display(), err = %e, "cannot write the .cwasm cache; the component will compile again next load");
            }
        }
        // SAFETY: `compiled` came from this engine's `precompile_component`
        // a moment ago.
        let component = unsafe { Component::deserialize(engine, &compiled) }
            .map_err(|e| format!("does not load after compiling: {e:#}"))?;
        Ok((component, source))
    }

    fn load_cached(
        &self,
        engine: &Engine,
        digest_hex: &str,
        path: &Path,
    ) -> Result<Component, String> {
        let expected = std::fs::read_to_string(self.sum_path(digest_hex))
            .map_err(|_| "no checksum beside it".to_owned())?;
        let bytes = std::fs::read(path).map_err(|e| format!("unreadable: {e}"))?;
        let actual = hex::encode(Sha256::digest(&bytes));
        if actual != expected.trim() {
            return Err("its checksum does not match".into());
        }
        drop(bytes);
        // SAFETY: the file is one this cache wrote (module docs), and its
        // bytes still hash to what was written.
        unsafe { Component::deserialize_file(engine, path) }
            .map_err(|e| format!("does not deserialize: {e:#}"))
    }

    fn store(&self, digest_hex: &str, compiled: &[u8]) -> std::io::Result<()> {
        create_private_dir(&self.dir)?;
        let sum = hex::encode(Sha256::digest(compiled));
        let suffix: u64 = rand::random();
        let tmp = self.dir.join(format!(".{digest_hex}.{suffix:016x}.tmp"));
        let tmp_sum = self
            .dir
            .join(format!(".{digest_hex}.{suffix:016x}.sha256.tmp"));
        let result = (|| {
            write_synced(&tmp, compiled)?;
            write_synced(&tmp_sum, sum.as_bytes())?;
            std::fs::rename(&tmp_sum, self.sum_path(digest_hex))?;
            std::fs::rename(&tmp, self.path_for(digest_hex))
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
            let _ = std::fs::remove_file(&tmp_sum);
        }
        result
    }
}

fn write_synced(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir)
    }
}
