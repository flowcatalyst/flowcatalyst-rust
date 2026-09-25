//! The full Go-vs-Rust run, as an ignored test (Docker, a Go toolchain and
//! the Rust binaries are prerequisites; see README.md):
//!
//! ```text
//! cargo build -p fc-server -p fc-router-bin -p fc-outbox-processor
//! cargo test -p fc-delivery-harness --test delivery_run -- --ignored --nocapture
//! ```
//!
//! `HARNESS_ONLY=<name|prefix*>` runs a subset; `HARNESS_GO_SRC`,
//! `HARNESS_GO_BIN_DIR`, `HARNESS_RUST_BIN_DIR` as for the CLI. The test
//! asserts that the harness itself worked (both stacks booted, every
//! scenario ran); a DIFF is a finding written to the report, not a test
//! failure.

use std::path::PathBuf;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Docker, a Go toolchain and the Rust binaries"]
async fn delivery_parity_run() {
    let mut opts = fc_delivery_harness::Options {
        only: std::env::var("HARNESS_ONLY").ok(),
        ..Default::default()
    };
    if let Ok(p) = std::env::var("HARNESS_GO_SRC") {
        opts.go_src = PathBuf::from(p);
    }
    opts.go_bin_dir = std::env::var("HARNESS_GO_BIN_DIR").ok().map(PathBuf::from);
    if let Ok(p) = std::env::var("HARNESS_RUST_BIN_DIR") {
        opts.rust_bin_dir = PathBuf::from(p);
    }
    let report = fc_delivery_harness::run(opts).await.expect("harness run");
    assert!(
        report.harness_ok(),
        "a stack failed to boot: {:?}",
        report
            .stacks
            .iter()
            .filter_map(|s| s.boot_error.clone())
            .collect::<Vec<_>>()
    );
}

#[test]
fn scenarios_parse() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios");
    let all = fc_delivery_harness::scenario::load_dir(&dir).expect("scenarios parse");
    assert!(!all.is_empty());
    let mut names = std::collections::HashSet::new();
    for s in &all {
        assert!(
            names.insert(s.name.clone()),
            "duplicate scenario {}",
            s.name
        );
        assert!(
            s.name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "scenario name {} must be [a-z0-9-]",
            s.name
        );
        for st in &s.stimuli {
            if let Some(t) = st.target() {
                assert!(
                    s.targets.iter().any(|x| x.name == t),
                    "{}: unknown target {t}",
                    s.name
                );
            }
        }
    }
    let expected = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("expected-diffs.json");
    fc_delivery_harness::compare::load_expected(&expected).expect("expected-diffs parse");
}
