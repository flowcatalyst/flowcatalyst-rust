//! Go-vs-Rust delivery parity harness (owner decisions #28/#29).
//!
//! Drives identical scenarios through the Go stack and the Rust stack end
//! to end — producer (events / dispatch jobs through the platform API, or
//! rows in the SDK outbox table forwarded by the outbox processor) →
//! ingest / fan-out → scheduler → SQS (LocalStack, FIFO) → router →
//! `/api/dispatch/process` → webhook — and compares what each delivered.
//! See `README.md` for usage and the scenario format.

pub mod api;
pub mod compare;
pub mod infra;
pub mod receiver;
pub mod report;
pub mod scenario;
pub mod side;
pub mod stack;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use anyhow::{bail, Context};

use crate::compare::ExpectedDiff;
use crate::infra::Infra;
use crate::report::{Report, ScenarioResult};
use crate::scenario::Scenario;
use crate::side::{Binaries, Side};
use crate::stack::SideKind;

#[derive(Debug, Clone)]
pub struct Options {
    pub sides: Vec<SideKind>,
    /// Run only scenarios whose name matches (prefix ending in `*`, or
    /// exact). The stale-entry check on `expected-diffs.json` is skipped.
    pub only: Option<String>,
    pub scenarios_dir: PathBuf,
    pub expected_diffs: PathBuf,
    pub report_dir: Option<PathBuf>,
    pub go_src: PathBuf,
    /// Prebuilt Go `fc-server`; skips the Go build.
    pub go_bin_dir: Option<PathBuf>,
    /// Directory holding `fc-server` (every role, the router included) and
    /// `fc-outbox-processor`.
    pub rust_bin_dir: PathBuf,
    pub keep_infra: bool,
    /// Rust's database: `go` (default) = migrated and seeded by Go's
    /// fc-server first, then adopted by Rust, as at cutover; `own` = a
    /// fresh database migrated by Rust alone.
    pub rust_schema: RustSchema,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RustSchema {
    Go,
    Own,
}

pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

impl Default for Options {
    fn default() -> Self {
        let root = workspace_root();
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        Options {
            sides: vec![SideKind::Go, SideKind::Rust],
            only: None,
            scenarios_dir: manifest.join("scenarios"),
            expected_diffs: manifest.join("expected-diffs.json"),
            report_dir: None,
            go_src: root.join("../flowcatalyst-go"),
            go_bin_dir: None,
            rust_bin_dir: root.join("target/debug"),
            keep_infra: false,
            rust_schema: RustSchema::Go,
        }
    }
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Build Go's `fc-server` from `go_src` into `out_dir` without touching
/// the Go repo: `-mod=readonly`, output outside the repo, and the repo's
/// `git status --porcelain` must be identical before and after.
pub fn build_go(go_src: &Path, out_dir: &Path) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(out_dir)?;
    let before = git(go_src, &["status", "--porcelain"])
        .with_context(|| format!("{} is not a git checkout", go_src.display()))?;
    let out = out_dir.join("fc-server");
    let status = Command::new("go")
        .args(["build", "-mod=readonly", "-o"])
        .arg(&out)
        .arg("./cmd/fc-server")
        .current_dir(go_src)
        .status()
        .context("running go build (is a Go toolchain on PATH?)")?;
    let after = git(go_src, &["status", "--porcelain"]).unwrap_or_default();
    if before != after {
        bail!(
            "the Go build changed {} (git status before:\n{before}\nafter:\n{after})",
            go_src.display()
        );
    }
    if !status.success() {
        bail!("go build failed");
    }
    Ok(out)
}

fn rust_binaries(dir: &Path) -> anyhow::Result<Binaries> {
    let dir = &dir
        .canonicalize()
        .with_context(|| format!("{} does not exist", dir.display()))?;
    let pick = |names: &[&str]| -> anyhow::Result<PathBuf> {
        names
            .iter()
            .map(|n| dir.join(n))
            .find(|p| p.exists())
            .with_context(|| {
                format!(
                    "none of {names:?} in {} — build with `cargo build -p fc-server -p fc-outbox-processor` or pass --rust-bin-dir",
                    dir.display()
                )
            })
    };
    let server = pick(&["fc-server"])?;
    Ok(Binaries {
        platform: server.clone(),
        worker: server.clone(),
        // Production runs the router as fc-server in its router role
        // (MESSAGE_ROUTER_ENABLED=true, PLATFORM_ENABLED=false), as Go does.
        router: server,
        outbox: pick(&["fc-outbox-processor"])?,
    })
}

fn run_id() -> String {
    const A: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let suffix: String = (0..5)
        .map(|_| A[rand::random_range(0..A.len())] as char)
        .collect();
    format!("{}-{suffix}", chrono::Utc::now().format("%Y%m%d-%H%M%S"))
}

fn openssl(args: &[&str]) -> anyhow::Result<String> {
    let out = Command::new("openssl")
        .args(args)
        .output()
        .context("openssl")?;
    if !out.status.success() {
        bail!("openssl {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn select(scenarios: Vec<Scenario>, only: &Option<String>) -> Vec<Scenario> {
    match only {
        None => scenarios,
        Some(pat) => scenarios
            .into_iter()
            .filter(|s| match pat.strip_suffix('*') {
                Some(p) => s.name.starts_with(p),
                None => pat.split(',').any(|p| p.trim() == s.name),
            })
            .collect(),
    }
}

/// Create the run directory and return it **absolute**. Every path derived
/// from it (the JWT key pair, each side's log directory) is handed to
/// processes that run with their side's directory as the working directory,
/// so a relative one resolves somewhere else there. Run 3 was started with
/// a relative `--report`: Go's platform could not read
/// `FC_JWT_SIGNING_KEY_PATH`, signed with an ephemeral key, and every bearer
/// the harness held died with the platform restart of `platform-down` —
/// Go's `outbox-events` and `slow-target-timeout` then failed on 401
/// `invalid_token` (Rust generated a key pair at the relative path under its
/// own directory and reloaded it, so it survived).
fn create_report_dir(dir: PathBuf) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(&dir)?;
    dir.canonicalize()
        .with_context(|| format!("resolve {}", dir.display()))
}

/// Run the harness; returns the report (already written to disk).
pub async fn run(opts: Options) -> anyhow::Result<Report> {
    let scenarios = select(scenario::load_dir(&opts.scenarios_dir)?, &opts.only);
    if scenarios.is_empty() {
        bail!("no scenarios selected");
    }
    let expected: Vec<ExpectedDiff> = compare::load_expected(&opts.expected_diffs)?;
    let id = run_id();
    let root = workspace_root();
    let report_dir = create_report_dir(
        opts.report_dir
            .clone()
            .unwrap_or_else(|| root.join("target/delivery-harness").join(&id)),
    )?;
    eprintln!("delivery harness run {id} → {}", report_dir.display());

    // Binaries first: a missing binary is a side error, not a crash.
    let needs_go = opts.sides.contains(&SideKind::Go)
        || (opts.sides.contains(&SideKind::Rust) && opts.rust_schema == RustSchema::Go);
    let go_bin: Option<anyhow::Result<PathBuf>> = needs_go.then(|| {
        let bin = match &opts.go_bin_dir {
            Some(d) => d.join("fc-server"),
            None => build_go(&opts.go_src, &root.join("target/go-bin"))?,
        };
        bin.canonicalize()
            .with_context(|| format!("{} does not exist", bin.display()))
    });
    let go_err = |e: &anyhow::Error| anyhow::anyhow!("Go fc-server: {e:#}");
    let mut side_bins: Vec<(SideKind, anyhow::Result<Binaries>)> = Vec::new();
    for &k in &opts.sides {
        let b = match k {
            SideKind::Go => match go_bin.as_ref().expect("go binary resolved") {
                Ok(bin) => Ok(Binaries {
                    platform: bin.clone(),
                    worker: bin.clone(),
                    router: bin.clone(),
                    outbox: bin.clone(),
                }),
                Err(e) => Err(go_err(e)),
            },
            SideKind::Rust => rust_binaries(&opts.rust_bin_dir),
        };
        side_bins.push((k, b));
    }

    let infra = Infra::start(&id, opts.keep_infra)?;
    let keys = report_dir.join("keys");
    std::fs::create_dir_all(&keys)?;
    let private = keys.join("jwt-private.pem");
    let public = keys.join("jwt-public.pem");
    openssl(&[
        "genpkey",
        "-algorithm",
        "RSA",
        "-pkeyopt",
        "rsa_keygen_bits:2048",
        "-out",
        &private.to_string_lossy(),
    ])?;
    openssl(&[
        "pkey",
        "-in",
        &private.to_string_lossy(),
        "-pubout",
        "-out",
        &public.to_string_lossy(),
    ])?;
    let app_key = openssl(&["rand", "-base64", "32"])?;

    let mut sides: Vec<Side> = Vec::new();
    let mut side_errors = Vec::new();
    for (k, bins) in side_bins {
        match bins {
            Ok(b) => {
                let s = Side::new(
                    k,
                    report_dir.join(k.label()),
                    &infra,
                    b,
                    &app_key,
                    private.clone(),
                    public.clone(),
                )
                .await?;
                let mut s = s;
                if k == SideKind::Rust && opts.rust_schema == RustSchema::Go {
                    match go_bin.as_ref().expect("go binary resolved") {
                        Ok(bin) => s.adopt_go_schema = Some(bin.clone()),
                        Err(e) => {
                            side_errors.push((k, format!("{:#}", go_err(e))));
                            continue;
                        }
                    }
                }
                sides.push(s);
            }
            Err(e) => side_errors.push((k, format!("{e:#}"))),
        }
    }
    // Boot both sides concurrently.
    {
        let futs = sides.iter_mut().map(|s| s.boot(&scenarios));
        futures::future::join_all(futs).await;
    }
    let sides: Vec<Arc<Side>> = sides.into_iter().map(Arc::new).collect();

    let mut results = Vec::new();
    for sc in &scenarios {
        eprintln!("── scenario {} ──", sc.name);
        let runs = futures::future::join_all(sides.iter().map(|s| s.run_scenario(sc))).await;
        for s in &sides {
            s.receiver.uninstall_scenario(&sc.name);
        }
        let result = ScenarioResult::build(sc, runs, &sides, &side_errors, &expected);
        eprintln!("   {} — {}", sc.name, result.verdict);
        results.push(result);
    }

    for s in &sides {
        s.shutdown().await;
    }
    drop(infra);

    let report = Report::build(
        &id,
        &opts,
        results,
        &sides,
        &side_errors,
        &expected,
        git(&opts.go_src, &["rev-parse", "--short", "HEAD"]),
        git(&root, &["rev-parse", "--short", "HEAD"]),
    );
    report.write(&report_dir)?;
    eprintln!("report: {}", report_dir.join("report.md").display());
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A relative `--report` comes back absolute, so the key pair and log
    /// paths handed to the side processes resolve from any working
    /// directory.
    #[test]
    fn the_report_dir_is_made_absolute() {
        let rel = PathBuf::from(format!(
            "target/delivery-harness-test-{}",
            std::process::id()
        ));
        let abs = create_report_dir(rel.clone()).unwrap();
        assert!(abs.is_absolute(), "{}", abs.display());
        assert!(abs.ends_with(&rel));
        std::fs::remove_dir_all(&abs).unwrap();
    }
}
