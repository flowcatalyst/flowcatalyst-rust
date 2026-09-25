//! CLI for the delivery parity harness. See README.md.

use std::path::PathBuf;

use clap::Parser;
use fc_delivery_harness::stack::SideKind;
use fc_delivery_harness::{Options, RustSchema};

#[derive(Parser, Debug)]
#[command(about = "Drive identical delivery scenarios through the Go and Rust stacks and compare")]
struct Cli {
    /// Sides to run: go, rust, or both (comma-separated).
    #[arg(long, default_value = "go,rust")]
    sides: String,
    /// Only scenarios matching this name (exact, comma-separated, or a
    /// prefix ending in `*`).
    #[arg(long)]
    only: Option<String>,
    #[arg(long)]
    scenarios: Option<PathBuf>,
    #[arg(long)]
    expected_diffs: Option<PathBuf>,
    /// Report directory (default target/delivery-harness/<run-id>).
    #[arg(long)]
    report: Option<PathBuf>,
    /// Go source checkout to build fc-server from (read-only use).
    #[arg(long, env = "HARNESS_GO_SRC")]
    go_src: Option<PathBuf>,
    /// Directory with a prebuilt Go fc-server (skips the Go build).
    #[arg(long, env = "HARNESS_GO_BIN_DIR")]
    go_bin_dir: Option<PathBuf>,
    /// Directory with the Rust fc-server, fc-router-bin and
    /// fc-outbox-processor (default target/debug of this workspace).
    #[arg(long, env = "HARNESS_RUST_BIN_DIR")]
    rust_bin_dir: Option<PathBuf>,
    /// Rust's database: `go` = migrated and seeded by Go's fc-server
    /// first, then adopted by Rust (the cutover path); `own` = a fresh
    /// database migrated by Rust alone.
    #[arg(long, default_value = "go")]
    rust_schema: String,
    /// Leave the Postgres and LocalStack containers running.
    #[arg(long)]
    keep_infra: bool,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let mut opts = Options {
        sides: cli
            .sides
            .split(',')
            .filter_map(|s| match s.trim() {
                "go" => Some(SideKind::Go),
                "rust" => Some(SideKind::Rust),
                _ => None,
            })
            .collect(),
        only: cli.only,
        ..Default::default()
    };
    if let Some(p) = cli.scenarios {
        opts.scenarios_dir = p;
    }
    if let Some(p) = cli.expected_diffs {
        opts.expected_diffs = p;
    }
    opts.report_dir = cli.report;
    if let Some(p) = cli.go_src {
        opts.go_src = p;
    }
    opts.go_bin_dir = cli.go_bin_dir;
    if let Some(p) = cli.rust_bin_dir {
        opts.rust_bin_dir = p;
    }
    opts.keep_infra = cli.keep_infra;
    opts.rust_schema = match cli.rust_schema.as_str() {
        "own" => RustSchema::Own,
        _ => RustSchema::Go,
    };
    match fc_delivery_harness::run(opts).await {
        Ok(report) => std::process::exit(if report.ok() { 0 } else { 1 }),
        Err(e) => {
            eprintln!("harness error: {e:#}");
            std::process::exit(2);
        }
    }
}
