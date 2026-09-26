//! Where fc-dev keeps the state it shares with Go's and Java's `fcdev`.
//!
//! The Rust reading of Go's `userDataDir`, `defaultEmbeddedPath`,
//! `embeddedPGCacheDir` and `pidFilePath` (`flowcatalyst-go/cmd/fcdev`), which
//! Java's `DevPaths` mirrors too. Same directories, so a developer switching
//! between the three binaries keeps one embedded cluster, one field-encryption
//! key, one signing key and one PID file:
//!
//! ```text
//! <userDataDir>/flowcatalyst/            persistent, per-user (never /tmp)
//! ├── embedded-pg/                        FC_EMBEDDED_DB_PATH (--embedded-db-path)
//! │   └── data/                           the PostgreSQL 18 cluster
//! ├── jwt-signing-key.pem                 JWT signing key (Go/Java write it)
//! ├── app-key                             FLOWCATALYST_APP_KEY (0600)
//! └── fcdev.pid                           PID of the running fcdev (FC_DEV_PID_FILE)
//!
//! <userCacheDir>/flowcatalyst/embedded-pg/   re-creatable PostgreSQL binaries
//! ├── bin/                                Go's tree (zonky binaries + any PostGIS transplant)
//! ├── PG-<md5>/                           Java's tree
//! └── theseus/<version>/                  Rust's tree (extracted from the fc-dev binary)
//! ```
//!
//! `userDataDir` is `$XDG_DATA_HOME`, else Go's `os.UserConfigDir()`
//! (`~/Library/Application Support` on macOS, `%AppData%` on Windows,
//! `$XDG_CONFIG_HOME` or `~/.config` elsewhere), else `~/.local/share`.
//! `userCacheDir` is Go's `os.UserCacheDir()` (`~/Library/Caches`,
//! `%LocalAppData%`, `$XDG_CACHE_HOME` or `~/.cache`). `dirs::config_dir` and
//! `dirs::cache_dir` resolve exactly those.

use std::path::PathBuf;

/// The default embedded-Postgres port, shared with Go and Java.
pub const DEFAULT_EMBEDDED_DB_PORT: u16 = 15432;

/// Go `userDataDir`.
pub fn user_data_dir() -> PathBuf {
    if let Some(x) = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(x);
    }
    if let Some(d) = dirs::config_dir() {
        return d;
    }
    if let Some(home) = dirs::home_dir() {
        return home.join(".local").join("share");
    }
    PathBuf::from(".")
}

/// Go `os.UserCacheDir()` with its fallbacks.
pub fn user_cache_dir() -> PathBuf {
    if let Some(c) = dirs::cache_dir() {
        return c;
    }
    if let Some(home) = dirs::home_dir() {
        return home.join(".cache");
    }
    PathBuf::from(".").join(".flowcatalyst-cache")
}

/// `<userDataDir>/flowcatalyst`.
pub fn flowcatalyst_dir() -> PathBuf {
    user_data_dir().join("flowcatalyst")
}

/// Go `defaultEmbeddedPath`: `<userDataDir>/flowcatalyst/embedded-pg`. The
/// cluster itself is `<that>/data`.
pub fn default_embedded_path() -> PathBuf {
    flowcatalyst_dir().join("embedded-pg")
}

/// Go `pidFilePath`: `<userDataDir>/flowcatalyst/fcdev.pid`.
pub fn default_pid_file() -> PathBuf {
    flowcatalyst_dir().join("fcdev.pid")
}

/// Go `embeddedPGCacheDir`: `<userCacheDir>/flowcatalyst/embedded-pg`.
pub fn embedded_pg_cache_dir() -> PathBuf {
    user_cache_dir().join("flowcatalyst").join("embedded-pg")
}

/// `--embedded-db-path`, else `FC_EMBEDDED_DB_PATH`, else the shared default.
pub fn embedded_path(flag: Option<&PathBuf>) -> PathBuf {
    flag.cloned()
        .or_else(|| {
            std::env::var_os("FC_EMBEDDED_DB_PATH")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        })
        .unwrap_or_else(default_embedded_path)
}

/// The directory beside the embedded path that holds the keys, as Go's
/// `filepath.Dir(opts.EmbeddedDBPath)`.
pub fn state_dir_for(embedded_path: &std::path::Path) -> PathBuf {
    embedded_path
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(flowcatalyst_dir)
}

/// The Rust-only cluster fc-dev used before it shared Go's and Java's.
pub fn legacy_rust_cluster() -> PathBuf {
    user_cache_dir().join("flowcatalyst-dev").join("pgdata")
}

/// Go `ensureAppKeyFile`: the trimmed key in `path`, or a fresh 32-byte key
/// (base64) written there with mode 0600.
pub fn ensure_app_key_file(path: &std::path::Path) -> anyhow::Result<String> {
    use base64::Engine;
    use rand::RngCore;

    if let Ok(existing) = std::fs::read_to_string(path) {
        let key = existing.trim();
        if !key.is_empty() {
            return Ok(key.to_string());
        }
    }
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let key = base64::engine::general_purpose::STANDARD.encode(bytes);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_private(path, key.as_bytes())?;
    Ok(key)
}

/// Write `contents` to `path`, readable by the owner only on Unix.
pub fn write_private(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        f.write_all(contents)
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, contents)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cluster_and_pid_file_live_under_the_shared_flowcatalyst_dir() {
        let base = flowcatalyst_dir();
        assert_eq!(default_embedded_path(), base.join("embedded-pg"));
        assert_eq!(default_pid_file(), base.join("fcdev.pid"));
        assert_eq!(state_dir_for(&default_embedded_path()), base);
        assert!(embedded_pg_cache_dir().ends_with("flowcatalyst/embedded-pg"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_uses_application_support_like_go() {
        if std::env::var_os("XDG_DATA_HOME").is_some() {
            return;
        }
        let home = dirs::home_dir().unwrap();
        assert_eq!(
            default_embedded_path(),
            home.join("Library/Application Support/flowcatalyst/embedded-pg")
        );
        assert_eq!(
            embedded_pg_cache_dir(),
            home.join("Library/Caches/flowcatalyst/embedded-pg")
        );
    }

    #[test]
    fn the_app_key_file_is_created_once_then_reused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("app-key");
        let first = ensure_app_key_file(&path).unwrap();
        assert_eq!(first.len(), 44, "base64 of 32 bytes");
        let second = ensure_app_key_file(&path).unwrap();
        assert_eq!(first, second);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn an_existing_app_key_is_read_trimmed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app-key");
        std::fs::write(&path, "abc=\n").unwrap();
        assert_eq!(ensure_app_key_file(&path).unwrap(), "abc=");
    }
}
