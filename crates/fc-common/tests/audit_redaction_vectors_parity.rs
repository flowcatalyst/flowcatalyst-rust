//! Every in-repo copy of the audit-redaction test vectors is byte-identical
//! to `docs/spec/audit-redaction-vectors.json`.
//!
//! The canonical file lives in the Java repo (`flowcatalyst-javalin`,
//! `docs/spec/audit-redaction-vectors.json`, owner spec
//! `docs/spec/audit-redaction.md`, 2026-09-24); `docs/spec/` here is this
//! repo's copy of it. Each SDK carries its own copy because the SDKs are
//! split into their own repos, so a copy that drifts would let one language
//! pass a rule the others fail.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

const CANONICAL: &str = "docs/spec/audit-redaction-vectors.json";

const COPIES: &[&str] = &[
    "crates/fc-sdk/tests/fixtures/audit-redaction-vectors.json",
    "clients/typescript-sdk/tests/fixtures/audit-redaction-vectors.json",
    "clients/laravel-sdk/tests/Fixtures/audit-redaction-vectors.json",
];

#[test]
fn every_copy_is_byte_identical_to_the_canonical_vectors() {
    let root = repo_root();
    let canonical = std::fs::read(root.join(CANONICAL)).expect("read canonical vectors");
    for copy in COPIES {
        let bytes = std::fs::read(root.join(copy)).unwrap_or_else(|e| panic!("read {copy}: {e}"));
        assert!(
            bytes == canonical,
            "{copy} differs from {CANONICAL}; copy the canonical file over it byte for byte"
        );
    }
}

#[test]
fn the_canonical_vectors_parse() {
    let bytes = std::fs::read(repo_root().join(CANONICAL)).expect("read canonical vectors");
    let cases: serde_json::Value = serde_json::from_slice(&bytes).expect("vectors are JSON");
    let cases = cases.as_array().expect("vectors are an array");
    assert!(!cases.is_empty());
    for case in cases {
        for field in ["name", "input", "masked", "expected"] {
            assert!(case.get(field).is_some(), "a case is missing `{field}`");
        }
    }
}
