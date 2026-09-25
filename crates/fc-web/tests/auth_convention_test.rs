//! Convention test: every UI entry point is authenticated *and* authorized.
//!
//! Topcoat pages, routes, shards and procedures are each an HTTP endpoint.
//! Shards and procedures in particular get their own POST endpoint that runs
//! without the page or layout around them, so nothing a page checks protects
//! them. The UI's rule (see `src/auth.rs`):
//!
//! 1. Every handler declares an explicit path under `/ui/(app)`, which puts
//!    it inside the authentication layer. A shard or procedure without a
//!    path would get a generated `/_topcoat/runtime/...` path outside it.
//! 2. Every handler body resolves the caller (`auth(cx)`) and runs a
//!    permission check (`permit(checks::...)` or a helper that does).
//!
//! Public entry points (the login form, logout) go on the allowlist with a
//! reason.

use std::fs;
use std::path::{Path, PathBuf};

/// Matched at the start of a line, so doc comments mentioning them don't count.
const HANDLER_ATTRS: &[&str] = &["\n#[page", "\n#[route", "\n#[shard", "\n#[procedure"];

/// Any of these in a body counts as the permission check.
const PERMISSION_PATTERNS: &[&str] = &["permit(checks::", "load_for_write("];

/// `file::fn` entries that are deliberately public, with why.
const PUBLIC: &[(&str, &str)] = &[
    ("login.rs::login", "the sign-in form itself"),
    (
        "shell.rs::logout",
        "clears the caller's own cookie; origin-checked POST",
    ),
];

/// `file::fn` entries that need a session but no permission.
const SESSION_ONLY: &[(&str, &str)] = &[(
    "shell.rs::home",
    "redirects to the first page the caller may open; reads nothing",
)];

struct Handler {
    key: String,
    attr: String,
    body: String,
}

fn app_files() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/app");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("src/app exists")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
        .collect();
    files.sort();
    files
}

/// The body of the first `fn` after `from`, by brace matching.
fn fn_after(src: &str, from: usize) -> Option<(String, String)> {
    let fn_at = from + src[from..].find("fn ")?;
    let name: String = src[fn_at + 3..]
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    let open = fn_at + src[fn_at..].find('{')?;
    let mut depth = 0usize;
    for (i, ch) in src[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((name, src[open..=open + i].to_owned()));
                }
            }
            _ => {}
        }
    }
    None
}

fn handlers() -> Vec<Handler> {
    let mut out = Vec::new();
    for path in app_files() {
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        let src = fs::read_to_string(&path).unwrap();
        let mut at = 0;
        while let Some(rel) = HANDLER_ATTRS.iter().filter_map(|a| src[at..].find(a)).min() {
            let start = at + rel + 1; // skip the leading newline
            let line_end = start + src[start..].find('\n').unwrap_or(src.len() - start);
            let attr = src[start..line_end].trim().to_owned();
            let (name, body) = fn_after(&src, line_end).expect("handler fn follows attribute");
            out.push(Handler {
                key: format!("{file}::{name}"),
                attr,
                body,
            });
            at = line_end;
        }
    }
    out
}

#[test]
fn finds_handlers() {
    // Guard against the scanner silently matching nothing.
    assert!(handlers().len() >= 10, "scanner found too few handlers");
}

#[test]
fn every_handler_has_an_explicit_path_under_the_auth_layer() {
    let mut bad = Vec::new();
    for h in handlers() {
        if PUBLIC.iter().any(|(k, _)| *k == h.key) {
            continue;
        }
        if !h.attr.contains("\"/ui/(app)") {
            bad.push(format!("{} — {}", h.key, h.attr));
        }
    }
    assert!(
        bad.is_empty(),
        "handlers outside `/ui/(app)` (no authentication layer):\n  {}",
        bad.join("\n  ")
    );
}

#[test]
fn every_handler_authenticates_and_checks_a_permission() {
    let mut bad = Vec::new();
    for h in handlers() {
        if PUBLIC.iter().any(|(k, _)| *k == h.key) {
            continue;
        }
        if !h.body.contains("auth(cx)") {
            bad.push(format!("{}: never resolves the caller (`auth(cx)`)", h.key));
        }
        if SESSION_ONLY.iter().any(|(k, _)| *k == h.key) {
            continue;
        }
        if !PERMISSION_PATTERNS.iter().any(|p| h.body.contains(p)) {
            bad.push(format!("{}: no permission check", h.key));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn allowlists_name_real_handlers() {
    let keys: Vec<String> = handlers().into_iter().map(|h| h.key).collect();
    for (k, _) in PUBLIC.iter().chain(SESSION_ONLY) {
        assert!(keys.iter().any(|h| h == k), "stale allowlist entry {k}");
    }
}
