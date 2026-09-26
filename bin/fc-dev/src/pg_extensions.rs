//! PostGIS for the shared cluster, whichever binary serves it.
//!
//! The port of Java's `PgExtensions`. The embedded cluster is shared with
//! Go's and Java's fcdev, and PostGIS is hand-transplanted into Go's binary
//! tree only (`flowcatalyst-go/docs/embedded-postgres-postgis.md`). A
//! database with `CREATE EXTENSION postgis` in it starts fine under another
//! tree, then fails the moment a query touches a PostGIS object, because
//! `$libdir/postgis-3` (or the `.control` file) is missing there.
//!
//! PostGIS is "just files": loadable modules plus `.control`/`.sql` extension
//! files. Before starting the server, fc-dev copies the PostGIS family from
//! the first tree that has it into the tree it runs (its own, under
//! `<userCacheDir>/flowcatalyst/embedded-pg/theseus/<version>`). It never
//! overwrites a file, so it is idempotent and leaves a tree that already
//! carries PostGIS untouched. After the start, every extension installed in
//! any database of the cluster is checked against the tree, and a missing one
//! is logged as an error naming the database and what to install.
//!
//! Pure filesystem work, no logging: the caller decides what is worth a line.

use std::path::{Path, PathBuf};

/// The families mirrored: PostGIS and its companions. A file belongs when
/// its name starts with one of these (`rtpostgis` is the raster module's
/// name on some Linux packages).
const FAMILY_PREFIXES: &[&str] = &["postgis", "rtpostgis", "address_standardizer"];
const MODULE_SUFFIXES: &[&str] = &[".dylib", ".so", ".dll"];
const EXTENSION_SUFFIXES: &[&str] = &[".control", ".sql"];

/// Built into the server, no `.control` file to find.
const NO_CONTROL_FILE_REQUIRED: &[&str] = &["plpgsql"];

/// A PostgreSQL tree's module directory (`$libdir`) and extension directory
/// (`<sharedir>/extension`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tree {
    pub modules: PathBuf,
    pub extensions: PathBuf,
}

/// Locate `$libdir` and the extension directory under an installation root,
/// anchored on a file that must be there: `plpgsql` for a server tree,
/// `postgis` for a donor. Handles the layouts in use: theseus
/// (`lib/`, `share/extension/`), zonky/Go/Debian-style
/// (`lib/postgresql/`, `share/postgresql/extension/`) and packaged roots that
/// are already the module directory.
pub fn locate(root: &Path, anchor: &str) -> Option<Tree> {
    let modules = [
        root.join("lib").join("postgresql"),
        root.join("lib"),
        root.to_path_buf(),
    ]
    .into_iter()
    .find(|d| has_module(d, anchor))?;
    let extensions = [
        root.join("share").join("postgresql").join("extension"),
        root.join("share").join("extension"),
        root.join("extension"),
    ]
    .into_iter()
    .find(|d| d.join(format!("{anchor}.control")).is_file())?;
    Some(Tree {
        modules,
        extensions,
    })
}

fn has_module(dir: &Path, anchor: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        let name = e.file_name().to_string_lossy().to_string();
        name.starts_with(anchor) && has_suffix(&name, MODULE_SUFFIXES)
    })
}

/// Where PostGIS may already be on this machine, in priority order:
///
/// 1. `--embedded-db-extensions-from` / `FC_EMBEDDED_DB_EXTENSIONS_FROM`;
/// 2. Go's fcdev tree (`<cache>/bin`) — the PostGIS the shared cluster's
///    objects were created with;
/// 3. Java's fcdev trees (`<cache>/PG-*`), which mirror Go's;
/// 4. packaged PostGIS for this major: Homebrew (Apple Silicon, Intel),
///    Debian/Ubuntu PGDG, RHEL PGDG.
pub fn donor_candidates(configured: Option<&Path>, pg_cache_dir: &Path, major: &str) -> Vec<Tree> {
    let mut out = Vec::new();
    if let Some(root) = configured {
        if let Some(t) = locate(root, "postgis") {
            out.push(t);
        }
    }
    if let Some(t) = locate(&pg_cache_dir.join("bin"), "postgis") {
        out.push(t);
    }
    if let Ok(entries) = std::fs::read_dir(pg_cache_dir) {
        let mut java: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("PG-"))
            })
            .collect();
        java.sort();
        out.extend(java.iter().filter_map(|p| locate(p, "postgis")));
    }
    for prefix in ["/opt/homebrew/opt/postgis", "/usr/local/opt/postgis"] {
        out.push(Tree {
            modules: PathBuf::from(format!("{prefix}/lib/postgresql@{major}")),
            extensions: PathBuf::from(format!("{prefix}/share/postgresql@{major}/extension")),
        });
    }
    out.push(Tree {
        modules: PathBuf::from(format!("/usr/lib/postgresql/{major}/lib")),
        extensions: PathBuf::from(format!("/usr/share/postgresql/{major}/extension")),
    });
    out.push(Tree {
        modules: PathBuf::from(format!("/usr/pgsql-{major}/lib")),
        extensions: PathBuf::from(format!("/usr/pgsql-{major}/share/extension")),
    });
    out
}

/// The first candidate that can donate: both directories exist and
/// `postgis.control` is there.
pub fn first_usable(candidates: &[Tree]) -> Option<&Tree> {
    candidates.iter().find(|t| {
        t.modules.is_dir()
            && t.extensions.is_dir()
            && t.extensions.join("postgis.control").is_file()
    })
}

/// Whether `tree` already provides PostGIS.
pub fn has_postgis(tree: &Tree) -> bool {
    tree.extensions.join("postgis.control").is_file()
}

/// Copy the family from `donor` into `target`. Never overwrites; returns the
/// sorted names actually copied.
pub fn mirror(donor: &Tree, target: &Tree) -> std::io::Result<Vec<String>> {
    std::fs::create_dir_all(&target.modules)?;
    std::fs::create_dir_all(&target.extensions)?;
    let mut copied = mirror_dir(&donor.modules, &target.modules, MODULE_SUFFIXES)?;
    copied.extend(mirror_dir(
        &donor.extensions,
        &target.extensions,
        EXTENSION_SUFFIXES,
    )?);
    copied.sort();
    Ok(copied)
}

fn mirror_dir(src: &Path, dst: &Path, suffixes: &[&str]) -> std::io::Result<Vec<String>> {
    let mut copied = Vec::new();
    let Ok(entries) = std::fs::read_dir(src) else {
        return Ok(copied);
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !is_family(&name) || !has_suffix(&name, suffixes) {
            continue;
        }
        // Follow symlinks (Homebrew's opt/ paths are links into the Cellar).
        let source = entry.path();
        if !source.is_file() {
            continue;
        }
        let target = dst.join(&name);
        if target.exists() {
            continue;
        }
        // std::fs::copy carries the permission bits (the modules' exec bit).
        std::fs::copy(&source, &target)?;
        copied.push(name);
    }
    Ok(copied)
}

fn is_family(name: &str) -> bool {
    FAMILY_PREFIXES.iter().any(|p| name.starts_with(p))
}

fn has_suffix(name: &str, suffixes: &[&str]) -> bool {
    let lower = name.to_ascii_lowercase();
    suffixes.iter().any(|s| lower.ends_with(s))
}

/// The installed extensions `extension_dir` has no `.control` file for.
pub fn missing_control_files(installed: &[String], extension_dir: &Path) -> Vec<String> {
    let mut missing: Vec<String> = installed
        .iter()
        .filter(|n| !NO_CONTROL_FILE_REQUIRED.contains(&n.as_str()))
        .filter(|n| !extension_dir.join(format!("{n}.control")).is_file())
        .cloned()
        .collect();
    missing.sort();
    missing.dedup();
    missing
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
    }

    /// A zonky/Go-style tree: lib/postgresql + share/postgresql/extension.
    fn go_tree(root: &Path, with_postgis: bool) {
        touch(&root.join("lib/postgresql/plpgsql.dylib"));
        touch(&root.join("share/postgresql/extension/plpgsql.control"));
        if with_postgis {
            touch(&root.join("lib/postgresql/postgis-3.dylib"));
            touch(&root.join("lib/postgresql/postgis_raster-3.dylib"));
            touch(&root.join("lib/postgresql/address_standardizer-3.dylib"));
            touch(&root.join("lib/postgresql/hstore.dylib"));
            touch(&root.join("share/postgresql/extension/postgis.control"));
            touch(&root.join("share/postgresql/extension/postgis--3.6.4.sql"));
            touch(&root.join("share/postgresql/extension/address_standardizer.control"));
            touch(&root.join("share/postgresql/extension/hstore.control"));
            touch(&root.join("share/postgresql/extension/postgis.README"));
        }
    }

    /// A theseus-style tree: lib/ + share/extension.
    fn theseus_tree(root: &Path) {
        touch(&root.join("lib/plpgsql.dylib"));
        touch(&root.join("share/extension/plpgsql.control"));
    }

    #[test]
    fn locate_handles_both_layouts() {
        let dir = tempfile::tempdir().unwrap();
        let go = dir.path().join("go");
        let rust = dir.path().join("rust");
        go_tree(&go, true);
        theseus_tree(&rust);
        assert_eq!(
            locate(&go, "postgis"),
            Some(Tree {
                modules: go.join("lib/postgresql"),
                extensions: go.join("share/postgresql/extension"),
            })
        );
        assert_eq!(
            locate(&rust, "plpgsql"),
            Some(Tree {
                modules: rust.join("lib"),
                extensions: rust.join("share/extension"),
            })
        );
        assert_eq!(locate(&rust, "postgis"), None);
    }

    #[test]
    fn mirror_copies_only_the_family_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let go = dir.path().join("cache/bin");
        let rust = dir.path().join("rust");
        go_tree(&go, true);
        theseus_tree(&rust);
        std::fs::write(rust.join("share/extension/postgis--3.6.4.sql"), b"mine").unwrap();

        let candidates = donor_candidates(None, &dir.path().join("cache"), "18");
        let donor = first_usable(&candidates)
            .expect("Go's tree donates")
            .clone();
        let target = locate(&rust, "plpgsql").unwrap();
        assert!(!has_postgis(&target));

        let copied = mirror(&donor, &target).unwrap();
        assert_eq!(
            copied,
            vec![
                "address_standardizer-3.dylib",
                "address_standardizer.control",
                "postgis-3.dylib",
                "postgis.control",
                "postgis_raster-3.dylib",
            ]
        );
        assert!(!rust.join("lib/hstore.dylib").exists());
        assert!(!rust.join("share/extension/postgis.README").exists());
        assert_eq!(
            std::fs::read(rust.join("share/extension/postgis--3.6.4.sql")).unwrap(),
            b"mine"
        );
        assert!(has_postgis(&target));
        assert!(mirror(&donor, &target).unwrap().is_empty(), "idempotent");
    }

    #[test]
    fn the_configured_source_comes_first() {
        let dir = tempfile::tempdir().unwrap();
        let configured = dir.path().join("mine");
        go_tree(&configured, true);
        go_tree(&dir.path().join("cache/bin"), true);
        let candidates = donor_candidates(Some(&configured), &dir.path().join("cache"), "18");
        assert_eq!(
            first_usable(&candidates).unwrap().modules,
            configured.join("lib/postgresql")
        );
    }

    #[test]
    fn no_donor_means_none() {
        let dir = tempfile::tempdir().unwrap();
        go_tree(&dir.path().join("cache/bin"), false);
        let candidates = donor_candidates(None, &dir.path().join("cache"), "99");
        assert!(first_usable(&candidates).is_none());
    }

    #[test]
    fn missing_control_files_ignores_plpgsql() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("hstore.control"));
        let installed = vec![
            "plpgsql".to_string(),
            "postgis".to_string(),
            "hstore".to_string(),
            "postgis".to_string(),
        ];
        assert_eq!(
            missing_control_files(&installed, dir.path()),
            vec!["postgis"]
        );
    }
}
