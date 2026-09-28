//! The guest SDK (`crates/fc-function-pdk`) vendors the host's WIT, so it
//! builds on its own from crates.io. The repository root's
//! `wit/flowcatalyst-function` (which this host is built from) is the source
//! of truth; this test fails while the PDK's copy differs from it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let rel = path.strip_prefix(root).unwrap().to_path_buf();
                out.insert(rel, std::fs::read(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

#[test]
fn the_pdk_vendors_the_hosts_wit_unchanged() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source = repo.join("wit/flowcatalyst-function");
    let vendored = repo.join("crates/fc-function-pdk/wit/flowcatalyst-function");

    let (source_files, vendored_files) = (files(&source), files(&vendored));
    let differ: Vec<_> = source_files
        .keys()
        .chain(vendored_files.keys())
        .filter(|k| source_files.get(*k) != vendored_files.get(*k))
        .map(|k| k.display().to_string())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    assert!(
        differ.is_empty(),
        "crates/fc-function-pdk/wit/flowcatalyst-function differs from \
         wit/flowcatalyst-function in {differ:?}; refresh the copy from the repository root:\n  \
         rm -rf crates/fc-function-pdk/wit/flowcatalyst-function && \
         cp -R wit/flowcatalyst-function crates/fc-function-pdk/wit/"
    );
}
