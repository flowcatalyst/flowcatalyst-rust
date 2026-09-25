//! `fc-parity`: run the Go-vs-Rust API parity harness, or refresh the
//! vendored Go lockfile extract. See `harness/parity/README.md`.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use fc_parity::coverage;
use fc_parity::Config;
use std::path::PathBuf;
use std::process::Command;

#[derive(Parser)]
#[command(
    name = "fc-parity",
    about = "Runs the API parity scenarios against Go's and Rust's fc-server on cloned seed databases"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
    #[command(flatten)]
    run: RunArgs,
}

#[derive(clap::Args)]
struct RunArgs {
    /// Go source tree to build fcdev/fc-server from (default: ../flowcatalyst-go beside this workspace).
    #[arg(long, env = "PARITY_GO_SRC")]
    go_src: Option<PathBuf>,
    /// Directory with prebuilt Go `fcdev` and `fc-server` (skips the Go build).
    #[arg(long, env = "PARITY_GO_BIN_DIR")]
    go_bin_dir: Option<PathBuf>,
    /// Directory with a prebuilt Rust `fc-server` (skips `cargo build --release -p fc-server`).
    #[arg(long, env = "PARITY_RUST_BIN_DIR")]
    rust_bin_dir: Option<PathBuf>,
    /// Scenario directory (default: the vendored harness/parity/scenarios).
    #[arg(long)]
    scenarios: Option<PathBuf>,
    /// Glob over scenario paths relative to --scenarios, e.g. `smoke/*` or `{auth,webauthn}/*`.
    #[arg(long, env = "PARITY_ONLY")]
    only: Option<String>,
    /// Report directory (default: target/parity-report in this workspace).
    #[arg(long)]
    report: Option<PathBuf>,
    /// Allow-list (default: harness/parity/expected-diffs.json).
    #[arg(long)]
    expected_diffs: Option<PathBuf>,
    /// PostgreSQL image for the run's database container.
    #[arg(long, env = "PARITY_PG_IMAGE", default_value = "postgres:18")]
    pg_image: String,
}

#[derive(Subcommand)]
enum Cmd {
    /// Regenerate lockfile-operations.json from Go's api/openapi.lock.json.
    ExtractLockfile {
        #[arg(long, env = "PARITY_GO_SRC")]
        go_src: Option<PathBuf>,
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("PARITY_LOG")
                .unwrap_or_else(|_| "fc_parity=info".into()),
        )
        .init();
    let cli = Cli::parse();
    let defaults = Config::defaults();
    if let Some(Cmd::ExtractLockfile { go_src, out }) = cli.command {
        return extract_lockfile(
            go_src.unwrap_or(defaults.go_src),
            out.unwrap_or(defaults.lockfile_file),
        );
    }
    let a = cli.run;
    let config = Config {
        go_src: a.go_src.unwrap_or(defaults.go_src),
        go_bin_dir: a.go_bin_dir,
        rust_bin_dir: a.rust_bin_dir,
        scenarios_dir: a.scenarios.unwrap_or(defaults.scenarios_dir),
        only: a.only,
        report_dir: a.report.unwrap_or(defaults.report_dir),
        expected_diffs_file: a.expected_diffs.unwrap_or(defaults.expected_diffs_file),
        pg_image: a.pg_image,
        ..defaults
    };
    let report = tokio::select! {
        r = fc_parity::run(&config) => r?,
        _ = tokio::signal::ctrl_c() => anyhow::bail!("interrupted; sides and database container removed"),
    };
    println!(
        "parity report written to {}",
        config.report_dir.join("report.md").display()
    );
    println!("{}", report.summary_line());
    std::process::exit(report.exit_code());
}

fn extract_lockfile(go_src: PathBuf, out: PathBuf) -> Result<()> {
    let lock = go_src.join("api/openapi.lock.json");
    let doc: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&lock).with_context(|| format!("read {}", lock.display()))?,
    )?;
    let commit = Command::new("git")
        .arg("-C")
        .arg(&go_src)
        .args(["rev-parse", "HEAD"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let ops = coverage::extract_lockfile(&doc, "flowcatalyst-go api/openapi.lock.json", &commit)?;
    std::fs::write(&out, serde_json::to_string_pretty(&ops)? + "\n")?;
    println!(
        "{} operations from {} @ {commit} → {}",
        ops.operations.len(),
        lock.display(),
        out.display()
    );
    Ok(())
}
