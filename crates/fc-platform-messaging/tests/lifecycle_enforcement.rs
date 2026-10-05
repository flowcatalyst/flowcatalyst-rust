//! Enforcement: nothing but the dispatch-job lifecycle writes
//! `msg_dispatch_jobs` or its queue table `msg_dispatch_queue`.
//!
//! Scans the production source of every crate and binary in the workspace
//! (everything under `crates/*/src` and `bin/*/src`, minus a file's trailing
//! `#[cfg(test)]` section) for SQL that inserts into, updates, deletes from
//! or truncates the table, and fails on any file that is not the lifecycle
//! or a named exception. No database needed.
//!
//! Adding a write elsewhere means adding a lifecycle operation (and a row in
//! its table), not an exception.

use std::fs;
use std::path::{Path, PathBuf};

/// The one file that owns the writes.
const LIFECYCLE: &str = "crates/fc-common/src/dispatch_lifecycle.rs";

/// Deliberate exceptions, each with its reason. Every entry must still
/// exist and still mention the table (a stale exception fails the test).
const EXCEPTIONS: &[(&str, &str)] = &[
    (
        "crates/fc-stream/src/dispatch_job_projection.rs",
        "the projector stamps projected_at (a projection bookkeeping column, never status)",
    ),
    (
        "crates/fc-stream/src/partition_manager.rs",
        "partition DDL (CREATE/DROP of month partitions); it does not write job rows",
    ),
];

/// The queue table's deliberate exceptions, each with its reason.
const QUEUE_EXCEPTIONS: &[(&str, &str)] = &[(
    "crates/fc-stream/src/partition_manager.rs",
    "dropping a msg_dispatch_jobs partition deletes that range's queue rows (no foreign key)",
)];
// Not scanned, by construction: migrations (.sql), and `fcdev fresh`, which
// drops the whole schema and names no table.

/// The statements that write the table, found in `src` (normalised to
/// lower-case single spaces). Returns one description per hit.
fn writes_in(src: &str) -> Vec<String> {
    writes_to(src, "msg_dispatch_jobs")
}

/// As [`writes_in`], for any table.
fn writes_to(src: &str, table: &str) -> Vec<String> {
    // A file's trailing test module is not production code.
    let prod = src.split("#[cfg(test)]").next().unwrap_or(src);
    let flat = prod
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let mut hits = Vec::new();
    let mut from = 0;
    while let Some(pos) = flat[from..].find(table) {
        let at = from + pos;
        from = at + table.len();
        // `msg_dispatch_jobs_read`, `msg_dispatch_jobs_2026_05`: other tables.
        let next = flat[from..].chars().next();
        if next.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let before = &flat[..at];
        for verb in [
            "insert into ",
            "update ",
            "update only ",
            "delete from ",
            "truncate ",
            "truncate table ",
        ] {
            if before.ends_with(verb) {
                hits.push(format!("`{} {table}`", verb.trim_end()));
            }
        }
    }
    hits
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn production_sources(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for group in ["crates", "bin"] {
        let Ok(entries) = fs::read_dir(root.join(group)) else {
            continue;
        };
        for e in entries.flatten() {
            rust_files(&e.path().join("src"), &mut files);
        }
    }
    files.sort();
    assert!(
        files.len() > 100,
        "the scan found only {} files: wrong root?",
        files.len()
    );
    files
}

fn relative(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/")
}

#[test]
fn only_the_lifecycle_writes_msg_dispatch_jobs() {
    let root = workspace_root();
    let mut violations = Vec::new();
    let mut lifecycle_writes = 0;
    for file in production_sources(&root) {
        let rel = relative(&root, &file);
        let hits = writes_in(&fs::read_to_string(&file).unwrap());
        if rel == LIFECYCLE {
            lifecycle_writes = hits.len();
            continue;
        }
        if hits.is_empty() || EXCEPTIONS.iter().any(|(f, _)| *f == rel) {
            continue;
        }
        violations.push(format!("{rel}: {}", hits.join(", ")));
    }
    assert!(
        lifecycle_writes > 0,
        "the scanner found no write in {LIFECYCLE}: it is not detecting anything"
    );
    assert!(
        violations.is_empty(),
        "msg_dispatch_jobs is written outside the dispatch-job lifecycle \
         ({LIFECYCLE}); add a lifecycle operation instead:\n  {}",
        violations.join("\n  ")
    );
}

#[test]
fn only_the_lifecycle_writes_msg_dispatch_queue() {
    let root = workspace_root();
    let mut violations = Vec::new();
    let mut lifecycle_writes = 0;
    for file in production_sources(&root) {
        let rel = relative(&root, &file);
        let hits = writes_to(&fs::read_to_string(&file).unwrap(), "msg_dispatch_queue");
        if rel == LIFECYCLE {
            lifecycle_writes = hits.len();
            continue;
        }
        if hits.is_empty() || QUEUE_EXCEPTIONS.iter().any(|(f, _)| *f == rel) {
            continue;
        }
        violations.push(format!("{rel}: {}", hits.join(", ")));
    }
    // enter (upsert), leave (two deletes) and create (insert) at least.
    assert!(
        lifecycle_writes >= 4,
        "the scanner found {lifecycle_writes} queue writes in {LIFECYCLE}: it is not detecting them"
    );
    assert!(
        violations.is_empty(),
        "msg_dispatch_queue is written outside the dispatch-job lifecycle \
         ({LIFECYCLE}); add a lifecycle operation instead:\n  {}",
        violations.join("\n  ")
    );
}

/// Production code reads the queue of PENDING jobs from `msg_dispatch_queue`
/// (through the lifecycle's claim, its hold-back lookups and its reconcile
/// sweep), never by asking `msg_dispatch_jobs` for `status = 'PENDING'`: the
/// partial indexes that made that cheap are gone (migration 065), and a read
/// that comes back is a seq scan or a slow plan under load. The lifecycle
/// itself reads the table for PENDING in its reconcile statements.
#[test]
fn nothing_outside_the_lifecycle_reads_pending_jobs_from_the_jobs_table() {
    let root = workspace_root();
    let mut violations = Vec::new();
    for file in production_sources(&root) {
        let rel = relative(&root, &file);
        if rel == LIFECYCLE {
            continue;
        }
        let src = fs::read_to_string(&file).unwrap();
        for hit in pending_reads_in(&src) {
            violations.push(format!("{rel}: {hit}"));
        }
    }
    assert!(
        violations.is_empty(),
        "msg_dispatch_jobs is read for status = 'PENDING' outside the lifecycle; \
         read msg_dispatch_queue (a lifecycle operation) instead:\n  {}",
        violations.join("\n  ")
    );
}

/// The three indexes migration 065 dropped are not named by any production
/// code (an index hint or a comment promising a plan that no longer exists).
#[test]
fn no_production_code_names_a_dropped_dispatch_index() {
    let root = workspace_root();
    for file in production_sources(&root) {
        let src = fs::read_to_string(&file).unwrap();
        let prod = src.split("#[cfg(test)]").next().unwrap_or(&src);
        for gone in [
            "idx_dispatch_jobs_pending_poll",
            "idx_dispatch_jobs_group_holders",
            "idx_dispatch_jobs_in_flight",
        ] {
            assert!(
                !prod.contains(gone),
                "{} names {gone}, which migration 065 dropped",
                relative(&root, &file)
            );
        }
    }
}

/// Statements in `src` (normalised) that select from `msg_dispatch_jobs` with
/// `status = 'PENDING'` in the same statement.
fn pending_reads_in(src: &str) -> Vec<String> {
    let prod = src.split("#[cfg(test)]").next().unwrap_or(src);
    let flat = prod
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let mut hits = Vec::new();
    let mut from = 0;
    while let Some(pos) = flat[from..].find("status = 'pending'") {
        let at = from + pos;
        from = at + "status = 'pending'".len();
        // The statement around it: back to the previous quote that opens the
        // string, forward to the next one.
        let start = flat[..at].rfind('"').map_or(0, |i| i + 1);
        let end = flat[from..].find('"').map_or(flat.len(), |i| from + i);
        let statement = &flat[start..end];
        let mut rest = statement;
        while let Some(i) = rest.find("msg_dispatch_jobs") {
            let after = rest[i + "msg_dispatch_jobs".len()..].chars().next();
            if !after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
                hits.push(statement.chars().take(100).collect::<String>());
                break;
            }
            rest = &rest[i + "msg_dispatch_jobs".len()..];
        }
    }
    hits
}

#[test]
fn the_pending_read_detector_detects() {
    assert_eq!(
        pending_reads_in(
            "sqlx::query(\"SELECT 1 FROM msg_dispatch_jobs WHERE status = 'PENDING'\")"
        )
        .len(),
        1
    );
    // Other tables, the read table and other statuses are not it.
    assert!(pending_reads_in("\"SELECT 1 FROM iam_requests WHERE status = 'PENDING'\"").is_empty());
    assert!(
        pending_reads_in("\"SELECT 1 FROM msg_dispatch_jobs_read WHERE status = 'PENDING'\"")
            .is_empty()
    );
    assert!(
        pending_reads_in("\"SELECT 1 FROM msg_dispatch_jobs WHERE status = 'QUEUED'\"").is_empty()
    );
    assert!(pending_reads_in(
        "fn a() {}\n#[cfg(test)]\nmod t { \"FROM msg_dispatch_jobs WHERE status = 'PENDING'\" }"
    )
    .is_empty());
}

#[test]
fn the_named_queue_exceptions_are_still_real() {
    let root = workspace_root();
    for (file, reason) in QUEUE_EXCEPTIONS {
        let src = fs::read_to_string(root.join(file))
            .unwrap_or_else(|_| panic!("exception {file} ({reason}) no longer exists"));
        assert!(
            !writes_to(&src, "msg_dispatch_queue").is_empty(),
            "exception {file} ({reason}) no longer writes the queue table: drop it"
        );
    }
}

#[test]
fn the_named_exceptions_are_still_real() {
    let root = workspace_root();
    for (file, reason) in EXCEPTIONS {
        let src = fs::read_to_string(root.join(file))
            .unwrap_or_else(|_| panic!("exception {file} ({reason}) no longer exists"));
        assert!(
            src.contains("msg_dispatch_jobs"),
            "exception {file} ({reason}) no longer mentions the table: drop it"
        );
    }
}

/// The detector itself: it sees every verb, across line breaks and case,
/// and ignores reads, other tables and test modules.
#[test]
fn the_detector_detects() {
    assert_eq!(
        writes_in("sqlx::query(\"UPDATE msg_dispatch_jobs SET x\")").len(),
        1
    );
    assert_eq!(writes_in("INSERT INTO\n   msg_dispatch_jobs (id)").len(), 1);
    assert_eq!(writes_in("delete  from msg_dispatch_jobs where").len(), 1);
    assert_eq!(writes_in("TRUNCATE TABLE msg_dispatch_jobs").len(), 1);
    assert_eq!(writes_in("UPDATE msg_dispatch_jobs_read SET").len(), 0);
    assert_eq!(writes_in("SELECT 1 FROM msg_dispatch_jobs WHERE").len(), 0);
    assert_eq!(
        writes_in("DROP TABLE IF EXISTS msg_dispatch_jobs_2026_05").len(),
        0
    );
    // The queue table: every verb, and not a longer name.
    assert_eq!(
        writes_to(
            "INSERT INTO msg_dispatch_queue (job_id)",
            "msg_dispatch_queue"
        )
        .len(),
        1
    );
    assert_eq!(
        writes_to(
            "DELETE FROM msg_dispatch_queue q USING",
            "msg_dispatch_queue"
        )
        .len(),
        1
    );
    assert_eq!(
        writes_to("UPDATE msg_dispatch_queue SET", "msg_dispatch_queue").len(),
        1
    );
    assert_eq!(
        writes_to("SELECT 1 FROM msg_dispatch_queue q", "msg_dispatch_queue").len(),
        0
    );
    assert_eq!(
        writes_to("INSERT INTO msg_dispatch_queue_x (a)", "msg_dispatch_queue").len(),
        0
    );
    assert_eq!(
        writes_in("fn a() {}\n#[cfg(test)]\nmod t { \"UPDATE msg_dispatch_jobs SET\" }").len(),
        0
    );
}
