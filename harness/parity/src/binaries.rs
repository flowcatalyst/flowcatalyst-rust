//! The binaries both sides run.
//!
//! **Go** (`fcdev` + `fc-server`): taken from `--go-bin-dir`, or built from
//! `--go-src` with `go build -mod=readonly -o <out>`, where `<out>` is outside
//! the Go tree. The Go repository is read-only to this harness: its
//! `git status --porcelain` is recorded before the build and must be
//! unchanged after it, or the run stops.
//!
//! **Rust** (`fc-server`): taken from `--rust-bin-dir`, or built from this
//! workspace with `cargo build --release -p fc-server` (release, because
//! debug-build password hashing makes every login step crawl).

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct GoBinaries {
    pub fcdev: PathBuf,
    pub fc_server: PathBuf,
    /// `go <commit>[ (dirty)]` or `prebuilt <dir>`, for the report.
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct RustBinaries {
    pub fc_server: PathBuf,
    pub label: String,
}

pub fn resolve_go(go_src: &Path, go_bin_dir: Option<&Path>, out_dir: &Path) -> Result<GoBinaries> {
    if let Some(dir) = go_bin_dir {
        let b = GoBinaries {
            fcdev: dir.join("fcdev"),
            fc_server: dir.join("fc-server"),
            label: format!("prebuilt {}", dir.display()),
        };
        for p in [&b.fcdev, &b.fc_server] {
            if !p.is_file() {
                bail!("--go-bin-dir has no {}", p.display());
            }
        }
        tracing::info!(dir = %dir.display(), "using prebuilt Go binaries");
        return Ok(b);
    }
    let src = std::fs::canonicalize(go_src)
        .with_context(|| format!("Go source tree {} not found", go_src.display()))?;
    std::fs::create_dir_all(out_dir)?;
    let before = git(&src, &["status", "--porcelain"])?;
    let commit = git(&src, &["rev-parse", "--short", "HEAD"])?;
    let t0 = Instant::now();
    let fcdev = out_dir.join("fcdev");
    let fc_server = out_dir.join("fc-server");
    go_build(&src, "./cmd/fcdev", &fcdev)?;
    go_build(&src, "./cmd/fc-server", &fc_server)?;
    let after = git(&src, &["status", "--porcelain"])?;
    if before != after {
        bail!(
            "the Go build changed the Go repository {} (git status before:\n{before}\nafter:\n{after})",
            src.display()
        );
    }
    tracing::info!(elapsed = ?t0.elapsed(), src = %src.display(), "built Go fcdev + fc-server (Go tree unchanged)");
    Ok(GoBinaries {
        fcdev,
        fc_server,
        label: format!(
            "go {}{}",
            commit.trim(),
            if before.trim().is_empty() {
                ""
            } else {
                " (dirty)"
            }
        ),
    })
}

fn go_build(src: &Path, pkg: &str, out: &Path) -> Result<()> {
    let output = Command::new("go")
        .args(["build", "-mod=readonly", "-o"])
        .arg(out)
        .arg(pkg)
        .current_dir(src)
        .output()
        .with_context(|| {
            format!(
                "start `go build {pkg}` in {} (is Go installed?)",
                src.display()
            )
        })?;
    if !output.status.success() {
        bail!(
            "`go build {pkg}` failed:\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

pub fn resolve_rust(workspace: &Path, rust_bin_dir: Option<&Path>) -> Result<RustBinaries> {
    let commit = git(workspace, &["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    let dirty = git(workspace, &["status", "--porcelain"])
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let label = format!(
        "rust {}{}",
        commit.trim(),
        if dirty { " (dirty)" } else { "" }
    );
    if let Some(dir) = rust_bin_dir {
        let fc_server = dir.join("fc-server");
        if !fc_server.is_file() {
            bail!("--rust-bin-dir has no {}", fc_server.display());
        }
        return Ok(RustBinaries {
            fc_server,
            label: format!("{label}, prebuilt {}", dir.display()),
        });
    }
    let t0 = Instant::now();
    let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["build", "--release", "-p", "fc-server"])
        .current_dir(workspace)
        .status()
        .context("start cargo build")?;
    if !status.success() {
        bail!("cargo build --release -p fc-server failed");
    }
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace.join("target"));
    tracing::info!(elapsed = ?t0.elapsed(), "built Rust fc-server");
    Ok(RustBinaries {
        fc_server: target.join("release").join("fc-server"),
        label,
    })
}

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .context("run git")?;
    if !out.status.success() {
        bail!("git {:?} in {} failed", args, dir.display());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
