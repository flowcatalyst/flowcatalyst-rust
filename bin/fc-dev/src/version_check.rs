//! Best-effort startup check for newer fc-dev releases on GitHub.
//!
//! Runs in a detached task so it never delays boot. Result is logged once
//! at INFO/WARN and stashed in [`LATEST_AVAILABLE`] for /health to surface.
//!
//! Why a custom GitHub query instead of `/releases/latest`:
//!   GitHub's "latest" endpoint considers *all* tags in the repo. The repo
//!   also publishes `laravel-sdk/v…` and `typescript-sdk/v…` tags, which
//!   would shadow fc-dev releases. We list releases and filter for the
//!   `fc-dev/v` prefix ourselves.
//!
//! Caching:
//!   The result is written to `~/.cache/flowcatalyst-dev/update-check.json`
//!   with a 24h TTL so repeated launches don't burn the 60/hour anonymous
//!   GitHub rate limit (relevant on shared NAT / CI).
//!
//! Disable: `FC_DEV_UPDATE_CHECK=false`.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use chrono::{DateTime, Utc};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use tracing::{debug, info, warn};

/// Latest release version observed on GitHub (if newer than the running
/// binary). `None` means either no check has completed yet, or the running
/// binary is up-to-date. The /health endpoint reads this.
pub static LATEST_AVAILABLE: OnceLock<String> = OnceLock::new();

const TAG_PREFIX: &str = "fc-dev/v";
const REPO: &str = "flowcatalyst/flowcatalyst";
const CACHE_TTL_SECS: i64 = 24 * 60 * 60;
const HTTP_TIMEOUT: Duration = Duration::from_secs(5);

/// Spawn the version check. Returns immediately; the task itself swallows
/// all errors (this is a hint, not a hard requirement).
pub fn spawn() {
    if env::var("FC_DEV_UPDATE_CHECK")
        .map(|v| v.eq_ignore_ascii_case("false") || v == "0")
        .unwrap_or(false)
    {
        debug!("update check disabled via FC_DEV_UPDATE_CHECK");
        return;
    }

    tokio::spawn(async move {
        if let Err(e) = run().await {
            debug!(error = %e, "update check failed (ignored)");
        }
    });
}

async fn run() -> anyhow::Result<()> {
    let current = Version::parse(env!("CARGO_PKG_VERSION"))?;

    // Try cache first.
    if let Some(cached) = read_cache() {
        if let Ok(cached_ver) = Version::parse(&cached.latest_version) {
            apply(&current, &cached_ver, "cache");
            return Ok(());
        }
    }

    let latest = fetch_latest().await?;
    write_cache(&CacheEntry {
        latest_version: latest.to_string(),
        checked_at: Utc::now(),
    });
    apply(&current, &latest, "github");
    Ok(())
}

#[expect(
    clippy::let_underscore_must_use,
    reason = "set at most once: a later set loses to the first by design"
)]
fn apply(current: &Version, latest: &Version, source: &str) {
    if latest > current {
        warn!(
            current = %current,
            latest = %latest,
            "fc-dev update available — run `fc-dev upgrade` (source={source})"
        );
        let _ = LATEST_AVAILABLE.set(latest.to_string());
    } else {
        info!(current = %current, "fc-dev is up to date (source={source})");
    }
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

async fn fetch_latest() -> anyhow::Result<Version> {
    // 100 is GitHub's max page size; that's well over our SDK + fc-dev tag
    // count for a single page until we have hundreds of releases. Switch to
    // pagination if/when that bites.
    let url = format!("https://api.github.com/repos/{REPO}/releases?per_page=100");
    let client = reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .user_agent(concat!("fc-dev/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let resp = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?;
    let releases: Vec<GhRelease> = resp.json().await?;

    let highest = releases
        .into_iter()
        .filter(|r| !r.draft && !r.prerelease)
        .filter_map(|r| {
            r.tag_name
                .strip_prefix(TAG_PREFIX)
                .and_then(|s| Version::parse(s).ok())
        })
        .max()
        .ok_or_else(|| anyhow::anyhow!("no fc-dev releases found"))?;
    Ok(highest)
}

#[derive(Serialize, Deserialize)]
struct CacheEntry {
    latest_version: String,
    checked_at: DateTime<Utc>,
}

fn cache_path() -> Option<PathBuf> {
    Some(
        dirs::cache_dir()?
            .join("flowcatalyst-dev")
            .join("update-check.json"),
    )
}

/// Synchronous accessor for the startup banner: returns the latest version
/// observed on a previous run *if* it's newer than the running binary, else
/// `None`. Reads `~/.cache/flowcatalyst-dev/update-check.json` (24h TTL).
/// On a true first run the cache is empty — banner shows no upgrade info
/// until the spawned check populates it for next time.
pub fn cached_upgrade_available() -> Option<String> {
    let entry = read_cache()?;
    let current = Version::parse(env!("CARGO_PKG_VERSION")).ok()?;
    let latest = Version::parse(&entry.latest_version).ok()?;
    if latest > current {
        Some(latest.to_string())
    } else {
        None
    }
}

fn read_cache() -> Option<CacheEntry> {
    let path = cache_path()?;
    let bytes = fs::read(&path).ok()?;
    let entry: CacheEntry = serde_json::from_slice(&bytes).ok()?;
    let age = Utc::now().signed_duration_since(entry.checked_at);
    if age.num_seconds() < CACHE_TTL_SECS && age.num_seconds() >= 0 {
        Some(entry)
    } else {
        None
    }
}

#[expect(
    clippy::let_underscore_must_use,
    reason = "best-effort cache: a failed write means the check runs again next time"
)]
fn write_cache(entry: &CacheEntry) {
    let Some(path) = cache_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(bytes) = serde_json::to_vec(entry) {
        let _ = fs::write(&path, bytes);
    }
}
