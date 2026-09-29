//! The platform's source tree, for the convention tests that read it.
//!
//! The platform is several crates (`crates/fc-platform-*` and the
//! `fc-platform` assembly), each keeping its modules at their old paths:
//! `principal/api.rs` is `crate::principal::api` in whichever crate holds it.
//! So the tests read every `crates/fc-platform*/src` as one tree, each file
//! under its path relative to its crate's `src/`. A few paths exist in more
//! than one crate (each crate's `lib.rs`, a module split across crates such
//! as `shared/mod.rs` or `shared/authorization_service.rs`); every copy is
//! listed.

use std::fs;
use std::path::{Path, PathBuf, StripPrefixError};

/// Every platform crate's `src/`: the assembly (`fc-platform`) first, then
/// the others by name.
pub fn src_roots() -> Vec<PathBuf> {
    let crates = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut roots = vec![PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")];
    let mut others: Vec<PathBuf> = fs::read_dir(&crates)
        .expect("crates/")
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(is_platform_crate)
        })
        .map(|p| p.join("src"))
        .filter(|p| p.is_dir())
        .collect();
    others.sort();
    roots.extend(others);
    roots
}

/// `fc-platform-<domain>`: the crates the platform was split into. Not
/// `fc-platform-jwks`, the token verifier the router shares.
fn is_platform_crate(name: &str) -> bool {
    name.strip_prefix("fc-platform-")
        .is_some_and(|domain| domain != "jwks")
}

/// Every `.rs` file of every platform crate.
pub fn rs_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in src_roots() {
        walk(&root, &mut out);
    }
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(p);
        }
    }
}

/// `path` relative to its crate's `src/` (`principal/api.rs`).
pub fn strip_src(path: &Path) -> Result<&Path, StripPrefixError> {
    let roots = src_roots();
    let mut last = None;
    for root in &roots {
        match path.strip_prefix(root) {
            Ok(rel) => return Ok(rel),
            Err(e) => last = Some(e),
        }
    }
    Err(last.expect("at least one source root"))
}

/// The one file at `rel` (e.g. `router.rs`); panics unless exactly one
/// crate has it.
pub fn file(rel: &str) -> PathBuf {
    let hits: Vec<PathBuf> = src_roots()
        .into_iter()
        .map(|root| root.join(rel))
        .filter(|p| p.is_file())
        .collect();
    assert_eq!(hits.len(), 1, "{rel} in {} platform crates", hits.len());
    hits.into_iter().next().unwrap()
}
