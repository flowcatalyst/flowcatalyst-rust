//! Runs the full Go-vs-Rust harness. `#[ignore]`d: it needs Docker, a Go
//! toolchain (or `PARITY_GO_BIN_DIR`) and a release build of `fc-server`
//! (or `PARITY_RUST_BIN_DIR`). Run with:
//!
//! ```text
//! cargo test -p fc-parity --test parity_run -- --ignored --nocapture
//! ```
//!
//! `PARITY_ONLY=<glob>` narrows the run to one group (e.g. `smoke/*`).
//!
//! As Java's `ParityRunTest`, the pinned assertion is that no step is an
//! `ERROR` (a failed `expect.status`, a missing capture, a transport
//! failure on either side): a `DIFF` is a finding reported in
//! `target/parity-report/report.md`, not asserted away here. Against Rust an
//! `ERROR` is often a finding too (Rust missing an `expect` Go meets), so a
//! red run means "read the report", not necessarily "the harness broke".

use fc_parity::report::StepStatus;
use fc_parity::Config;

#[tokio::test]
#[ignore = "needs Docker, Go and a release fc-server; see the file doc"]
async fn the_harness_runs_go_against_rust() {
    let defaults = Config::defaults();
    let config = Config {
        go_bin_dir: std::env::var_os("PARITY_GO_BIN_DIR").map(Into::into),
        rust_bin_dir: std::env::var_os("PARITY_RUST_BIN_DIR").map(Into::into),
        go_src: std::env::var_os("PARITY_GO_SRC").map_or(defaults.go_src.clone(), Into::into),
        only: std::env::var("PARITY_ONLY").ok(),
        ..defaults
    };
    let report = fc_parity::run(&config).await.expect("the harness ran");
    println!("{}", report.summary_line());
    assert!(!report.scenarios.is_empty(), "at least one scenario ran");
    let errors: Vec<String> = report
        .scenarios
        .iter()
        .flat_map(|s| {
            s.steps
                .iter()
                .filter(|st| st.status == StepStatus::Error)
                .map(move |st| {
                    format!(
                        "{} / {}: {}",
                        s.file,
                        st.step_id,
                        st.error.clone().unwrap_or_default()
                    )
                })
        })
        .collect();
    for e in &errors {
        eprintln!("ERROR {e}");
    }
    assert!(
        errors.is_empty(),
        "{} step(s) ERROR on a side; see target/parity-report/report.md",
        errors.len()
    );
}
