//! Parity gate G3 (docs/parity-plan-2026-10-18.md section 5): a database the Rust
//! migrations create has the schema a database Go's migrations create has.
//!
//! The fingerprint is `scripts/schema-fingerprint.sql`: one sorted line per table, column,
//! index, constraint and partition key of the public schema. The Go side is the committed
//! fixture `crates/fc-migrations/tests/fixtures/go-schema-fingerprint.txt`, generated from
//! Go's goose migrations by `scripts/schema-fingerprint.sh regen-go-fixture` (its own
//! throwaway container, never another database). Regenerate it whenever Go adds a migration.
//!
//! Rules:
//! - every line of the Go fixture is in Rust's fingerprint (Rust lacks nothing Go has);
//! - every line only Rust has matches an entry of `rust-only-objects.txt`, and every entry
//!   matches something (no stale allowance).
//!
//! By default the test migrates a fresh database (Docker, or `FC_TEST_PG_BIN` /
//! `FC_TEST_DATABASE_URL`, as `support::start_db`). `FC_FINGERPRINT_FILE=<file>` compares a
//! fingerprint taken elsewhere instead: `scripts/schema-fingerprint.sh takeover` uses it to
//! check a Go-082 database after Rust migrated it (the takeover path, with its probes).

use crate::support::start_db;
use fc_platform::shared::database::{create_pool, run_migrations, MigrationProfile};
use std::collections::BTreeSet;
use std::env;
use std::fs;

const GO_FIXTURE: &str =
    include_str!("../../../fc-migrations/tests/fixtures/go-schema-fingerprint.txt");
const RUST_ONLY: &str = include_str!("../../../fc-migrations/tests/fixtures/rust-only-objects.txt");
const FINGERPRINT_SQL: &str = include_str!("../../../../scripts/schema-fingerprint.sql");

fn lines(text: &str) -> BTreeSet<String> {
    text.lines()
        .map(str::trim_end)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// Compare a Rust-side fingerprint with the Go fixture; the returned text is empty when it matches.
pub(crate) fn compare(rust_fingerprint: &BTreeSet<String>) -> String {
    let go = lines(GO_FIXTURE);
    let allowed: Vec<&str> = RUST_ONLY
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .collect();
    let mut report = String::new();

    let missing: Vec<_> = go.difference(rust_fingerprint).collect();
    if !missing.is_empty() {
        report.push_str("In Go's schema but not Rust's (a Go migration Rust lacks):\n");
        for l in &missing {
            report.push_str(&format!("  {l}\n"));
        }
    }
    let extra: Vec<_> = rust_fingerprint.difference(&go).collect();
    let unexplained: Vec<_> = extra
        .iter()
        .filter(|l| !allowed.iter().any(|p| l.starts_with(p)))
        .collect();
    if !unexplained.is_empty() {
        report.push_str("In Rust's schema but not Go's, and not in rust-only-objects.txt:\n");
        for l in &unexplained {
            report.push_str(&format!("  {l}\n"));
        }
    }
    let stale: Vec<_> = allowed
        .iter()
        .filter(|p| !extra.iter().any(|l| l.starts_with(**p)))
        .collect();
    if !stale.is_empty() {
        report.push_str("rust-only-objects.txt entries that match nothing (remove them):\n");
        for p in &stale {
            report.push_str(&format!("  {p:?}\n"));
        }
    }
    report
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn rust_schema_matches_go_fingerprint() {
    let fingerprint = if let Ok(path) = env::var("FC_FINGERPRINT_FILE") {
        lines(&fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}")))
    } else {
        let (_container, url) = start_db("fingerprint").await;
        let pool = create_pool(&url).await.unwrap();
        run_migrations(&pool, MigrationProfile::Production)
            .await
            .unwrap();
        let rows: Vec<String> = sqlx::query_scalar(FINGERPRINT_SQL)
            .fetch_all(&pool)
            .await
            .unwrap();
        rows.into_iter().collect()
    };
    let report = compare(&fingerprint);
    assert!(
        report.is_empty(),
        "Rust's schema differs from Go's fingerprint:\n{report}"
    );
}

/// The comparison itself, without a database: it fails on a missing object, an unexplained
/// extra and a stale allowance.
#[test]
fn comparison_catches_each_kind_of_drift() {
    let go = lines(GO_FIXTURE);
    // Exactly the Go schema plus the allowed Rust-only objects would pass; build the
    // allowed extras from the allowlist's own prefixes.
    let mut rust = go.clone();
    for p in RUST_ONLY
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
    {
        rust.insert(format!("{p}probe"));
    }
    assert_eq!(compare(&rust), "", "a clean fingerprint compares clean");

    let one = go.iter().next().unwrap().clone();
    let mut missing = rust.clone();
    missing.remove(&one);
    assert!(compare(&missing).contains("Go migration Rust lacks"));

    let mut extra = rust.clone();
    extra.insert("column msg_events.invented text NULL default=-".to_string());
    assert!(compare(&extra).contains("not in rust-only-objects.txt"));

    assert!(compare(&go).contains("match nothing"));
}
