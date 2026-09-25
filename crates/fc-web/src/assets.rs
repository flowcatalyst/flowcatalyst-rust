//! The asset bundle (the Tailwind stylesheet, the Topcoat runtime, our
//! scripts), written by the process itself at startup.
//!
//! Topcoat's `asset!` declarations are compiled into the binary as paths to
//! the build machine's files; a bundle copies those files into a directory
//! of content-hashed names that the router serves. `topcoat asset bundle`
//! does that as a separate step, but it builds the binary itself and can't
//! pass fc-dev's `web` feature, and a manual step goes stale on every
//! rebuild. So the process bundles itself: the bundle for this exact
//! executable lives in `fc-web-assets/<key>` next to it, where `<key>`
//! derives from the executable's path, size and modification time, and is
//! written on the first start after a build.
//!
//! The source files must still exist where the build left them, which holds
//! for a `cargo build` on the same machine: the only way fc-web is built.
//! `FC_WEB_ASSETS_DIR` points at a prebuilt bundle instead.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use topcoat::asset::AssetBundle;
use topcoat_asset::{Bundler, BundlerConfig};

/// Load (or first write) the bundle. `None` means styles and scripts 404;
/// the pages still render.
pub(crate) fn load() -> Option<AssetBundle> {
    if let Some(dir) = std::env::var_os("FC_WEB_ASSETS_DIR") {
        return AssetBundle::load_dir(&dir)
            .inspect_err(|e| {
                tracing::warn!(error = %e, dir = ?dir, "FC_WEB_ASSETS_DIR has no fc-web asset bundle")
            })
            .ok();
    }
    match run_blocking(self_bundle) {
        Ok(bundle) => Some(bundle),
        Err(e) => {
            tracing::warn!(error = %e, "fc-web could not bundle its assets; styles and scripts will 404");
            None
        }
    }
}

/// Bundle blocks on file I/O; at startup that is fine, but on a
/// multi-threaded runtime let tokio move other tasks off this thread.
fn run_blocking<T>(f: impl FnOnce() -> T) -> T {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(f)
        }
        _ => f(),
    }
}

fn self_bundle() -> Result<AssetBundle, String> {
    let exe = std::env::current_exe().map_err(|e| format!("locating the executable: {e}"))?;
    let meta = std::fs::metadata(&exe).map_err(|e| format!("{}: {e}", exe.display()))?;
    let key = {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        exe.hash(&mut h);
        meta.len().hash(&mut h);
        meta.modified().ok().hash(&mut h);
        format!("{:016x}", h.finish())
    };

    let root = writable_root(&exe)?;
    let dir = root.join(&key);
    if let Ok(bundle) = AssetBundle::load_dir(&dir) {
        return Ok(bundle);
    }

    // Written under a private name and renamed into place, so a second
    // process starting from the same executable never reads half a bundle.
    let staging = root.join(format!("{key}.{}.tmp", std::process::id()));
    let binary = std::fs::read(&exe).map_err(|e| format!("{}: {e}", exe.display()))?;
    let config = BundlerConfig::new().cache_dir(root.join("cache"));
    Bundler::new(&config)
        .bundle(&binary, &staging)
        .map_err(|e| format!("bundling: {e}"))?;
    drop(binary);
    if std::fs::rename(&staging, &dir).is_err() {
        // Another process got there first; its bundle is the same.
        let _ = std::fs::remove_dir_all(&staging);
    }
    remove_stale(&root, &key);
    tracing::info!(dir = %dir.display(), "fc-web asset bundle written");
    AssetBundle::load_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))
}

/// `fc-web-assets/` next to the executable (`target/debug/` for a cargo
/// build), else under the temp dir.
fn writable_root(exe: &Path) -> Result<PathBuf, String> {
    let beside = exe.parent().map(|p| p.join("fc-web-assets"));
    for root in beside
        .into_iter()
        .chain([std::env::temp_dir().join("fc-web-assets")])
    {
        if std::fs::create_dir_all(&root).is_ok() {
            return Ok(root);
        }
    }
    Err("no writable directory for the asset bundle".to_string())
}

/// Bundles for earlier builds of this executable.
fn remove_stale(root: &Path, key: &str) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let is_bundle = name.len() == 16 && name.chars().all(|c| c.is_ascii_hexdigit());
        if is_bundle && name != key {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}
