//! `fc-dev upgrade` — replace the running binary with the latest GitHub
//! release.
//!
//! Why we don't use `self_update`'s built-in "latest" lookup:
//!   It hits `/releases/latest`, which considers all tags in the repo.
//!   Our SDK splits also tag this repo (`laravel-sdk/v…`,
//!   `typescript-sdk/v…`), so "latest" can return a non-fc-dev release.
//!   We list releases, filter for the `fc-dev/v` prefix, and pass the
//!   resulting tag as `target_version_tag`.
//!
//! Asset naming convention (must match the release workflow):
//!   `fc-dev-vX.Y.Z-{target_triple}.tar.gz`
//!     e.g. `fc-dev-v0.2.0-aarch64-apple-darwin.tar.gz`
//!
//! Atomic replacement (incl. Windows lock dance) is handled by `self_update`.

use anyhow::{anyhow, Context, Result};
use semver::Version;
use tracing::{info, warn};

use crate::UpgradeArgs;
use self_update::backends::github::Update;
use std::env::consts;
use std::time::Duration;

const REPO_OWNER: &str = "flowcatalyst";
const REPO_NAME: &str = "flowcatalyst";
const BIN_NAME: &str = "fc-dev";
const TAG_PREFIX: &str = "fc-dev/v";

pub async fn run(opts: &UpgradeArgs) -> Result<()> {
    let current = Version::parse(env!("CARGO_PKG_VERSION"))?;
    info!(current = %current, "checking for updates…");

    let latest = find_latest_release().await?;
    info!(latest = %latest.version, tag = %latest.tag, "latest release found");

    if !opts.force && latest.version <= current {
        info!("already on the latest version — nothing to do");
        return Ok(());
    }

    if opts.check {
        if latest.version > current {
            println!(
                "Update available: {} → {}\nRun `fc-dev upgrade` to install.",
                current, latest.version
            );
        } else {
            println!("fc-dev is up to date ({}).", current);
        }
        return Ok(());
    }

    if latest.version > current {
        info!(from = %current, to = %latest.version, "upgrading…");
    } else {
        warn!(version = %current, "--force: re-installing the same version");
    }

    install(&latest.tag)?;

    println!(
        "fc-dev upgraded to {}.\nRestart any running instances to pick up the new binary.",
        latest.version
    );
    Ok(())
}

struct Release {
    tag: String,
    version: Version,
}

async fn find_latest_release() -> Result<Release> {
    let url =
        format!("https://api.github.com/repos/{REPO_OWNER}/{REPO_NAME}/releases?per_page=100");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(concat!("fc-dev/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let resp = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?;

    #[derive(serde::Deserialize)]
    struct GhRelease {
        tag_name: String,
        #[serde(default)]
        draft: bool,
        #[serde(default)]
        prerelease: bool,
    }

    let releases: Vec<GhRelease> = resp.json().await?;

    releases
        .into_iter()
        .filter(|r| !r.draft && !r.prerelease)
        .filter_map(|r| {
            let v = r.tag_name.strip_prefix(TAG_PREFIX)?;
            let version = Version::parse(v).ok()?;
            Some(Release {
                tag: r.tag_name,
                version,
            })
        })
        .max_by(|a, b| a.version.cmp(&b.version))
        .ok_or_else(|| {
            anyhow!(
                "no fc-dev releases found in {REPO_OWNER}/{REPO_NAME}; \
                 check that releases tagged `{TAG_PREFIX}*` exist"
            )
        })
}

/// Download and replace the running binary. self_update's blocking call is
/// fine here — `fc-dev upgrade` is a synchronous one-shot, no event loop
/// to keep responsive.
fn install(tag: &str) -> Result<()> {
    let target = self_update::get_target();
    info!(target = %target, "installing for target");

    // Our release archives have a wrapping directory so that manual
    // `tar -xzf` / "Extract All" produces a tidy `fc-dev-vX.Y.Z-<target>/`
    // folder rather than dropping `fc-dev` directly into cwd. self_update
    // would otherwise look for `fc-dev` at the archive root and fail.
    //
    // We can't use self_update's `{{ version }}` template token here:
    // it expands to the FULL tag we pass to `target_version_tag` — for
    // us that's `fc-dev/v0.4.2`, not `0.4.2` — producing a bogus path
    // like `fc-dev-vfc-dev/v0.4.2-…/fc-dev`. Strip our `fc-dev/v` prefix
    // ourselves and format the path manually. EXE_SUFFIX handles the
    // trailing `.exe` on Windows that `{{ bin }}` would otherwise add.
    //
    // The format here MUST match the staging layout in
    // `.github/workflows/release-fc-dev.yml` ("Package archive" step,
    // `STAGE="fc-dev-v${VERSION}-${target}"`). Update both sides together.
    let stripped_version = tag.strip_prefix(TAG_PREFIX).unwrap_or(tag);
    let bin_path_in_archive = format!(
        "fc-dev-v{}-{}/{}{}",
        stripped_version,
        target,
        BIN_NAME,
        consts::EXE_SUFFIX,
    );

    let status = Update::configure()
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .bin_name(BIN_NAME)
        .bin_path_in_archive(&bin_path_in_archive)
        // self_update requires the running binary's version so it can
        // print a sensible status (and skip if equal — though we already
        // gate that ourselves in run() against the prefix-filtered tag).
        .current_version(env!("CARGO_PKG_VERSION"))
        .target(target)
        .target_version_tag(tag)
        .show_download_progress(true)
        .show_output(false)
        .no_confirm(true)
        .build()
        .context("failed to configure self_update")?
        .update()
        .context("self_update failed (binary not replaced)")?;

    info!(updated_to = %status.version(), "binary replaced");
    Ok(())
}
