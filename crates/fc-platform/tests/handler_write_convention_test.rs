//! Convention test: handler files (`*_api.rs`) must not perform direct
//! writes — no `.insert(` / `.update(` / `.delete(` / `.delete_req(` on
//! repository fields, no inline `sqlx::query("INSERT|UPDATE|DELETE")`.
//!
//! Writes belong in `*/operations/*.rs` use cases routed through a
//! `UnitOfWork`. Handlers build a command, call `use_case.run(cmd, ctx)`,
//! convert the result to HTTP.
//!
//! Complements:
//!   - `uow_convention_test.rs` — use cases must terminate through UoW
//!   - `junction_cascade_convention_test.rs` — cascade SQL confined to the
//!     owning repository
//!
//! ## Allowlist
//!
//! Some handlers still bypass this rule. They're listed below with the
//! specific anti-pattern so (a) the test remains green while we migrate,
//! and (b) the known gap is visible in tree. Fix the handler, remove from
//! the allowlist, and the test will enforce the invariant going forward.
//!
//! **Legitimate exceptions** (per CLAUDE.md):
//! - Event ingest (`event/api.rs::create_event`, `batch_api.rs`)
//! - Dispatch-job ingest (`dispatch_job/api.rs::create_dispatch_job` etc.)
//! - Stream projection (`projections_service.rs`)
//! - Dispatch process lifecycle
//! - Outbox forwarding
//!
//! These are platform-infrastructure-processing paths that cannot emit
//! domain events without recursion (events about event ingest would emit
//! events), so they write directly and skip UoW by design.

use std::fs;
use std::path::{Path, PathBuf};

/// File patterns to scan — handler files.
const HANDLER_FILE_SUFFIXES: &[&str] = &["_api.rs", "/api.rs"];

/// Write-call patterns. Each is checked against trimmed lines to avoid
/// false positives in comments. The patterns all include `_repo.` or
/// `repo.` to avoid matching unrelated `HashMap::insert` / `Vec::insert`
/// calls.
const FORBIDDEN_PATTERNS: &[&str] = &[
    "_repo.insert(",
    "_repo.update(",
    "_repo.delete(",
    "_repo.delete_req(",
    "_repo.insert_batch(",
    "_repo.upsert(",
    "repo.insert(",
    "repo.update(",
    "repo.delete(",
    "repo.delete_req(",
    "repo.insert_batch(",
    "repo.upsert(",
];

/// File-suffix allowlist — platform-infrastructure paths per CLAUDE.md.
/// These files are expected to write directly and are NOT flagged.
const FILE_ALLOWLIST: &[&str] = &[
    // Platform-infrastructure processing — see CLAUDE.md "Exceptions" section.
    "event/api.rs",
    "dispatch_job/api.rs",
    "shared/batch_api.rs",
    "shared/dispatch_process_api.rs",
    "shared/sdk_sync_api.rs",
    "shared/sdk_audit_batch_api.rs",
    "shared/sdk_dispatch_jobs_api.rs",
    // Application-roles SDK sync — already reviewed; cascade-guarded.
    "shared/application_roles_sdk_api.rs",
    // BFF roles API — delegates to role use cases internally; any remaining
    // repo calls are reads, but the test's naive pattern-match can flag them.
    "shared/bff_roles_api.rs",
    // Protocol-level auth token storage — refresh tokens, auth codes, OIDC
    // login state, pending-auth rows. Same category as "platform-
    // infrastructure processing" in CLAUDE.md: each token issuance is a
    // protocol side-effect, not a business operation. These paths
    // deliberately skip UseCase/UoW. Admin CRUD on auth config lives in
    // config_api.rs / oauth_clients_api.rs and is tracked line-by-line.
    "auth/auth_api.rs",
    "auth/oauth_api.rs",
    "auth/oidc_login_api.rs",
    "auth/password_reset_api.rs",
];

/// Line-level allowlist: `("suffix/of/file.rs", "substring of line")`.
/// The test ignores any flagged line whose file suffix and line-content
/// both match an entry here. Empty today — add only with a comment pointing
/// at the tracking issue.
const LINE_ALLOWLIST: &[(&str, &str)] = &[];

fn src_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn walk_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            walk_rs_files(&p, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(p);
        }
    }
}

fn is_handler_file(rel: &str) -> bool {
    HANDLER_FILE_SUFFIXES.iter().any(|s| rel.ends_with(s))
}

fn file_allowlisted(rel: &str) -> bool {
    FILE_ALLOWLIST.iter().any(|s| rel.ends_with(s))
}

fn line_allowlisted(rel: &str, line: &str) -> bool {
    LINE_ALLOWLIST.iter().any(|(file_suffix, substring)| {
        rel.ends_with(file_suffix) && line.contains(substring)
    })
}

fn matches_forbidden(line: &str) -> Option<&'static str> {
    // Skip comment-only lines.
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") || trimmed.starts_with("*") || trimmed.starts_with("///") {
        return None;
    }
    // Skip the utoipa `#[utoipa::path(delete, …)]` pattern — that's the
    // HTTP method declaration, not a DELETE call.
    if trimmed.starts_with("#[utoipa::") || trimmed.contains("utoipa::path") {
        return None;
    }
    // Skip axum `.route(".../{id}", delete(…))` — that's router wiring,
    // not a DB call. `axum::routing::delete` is the common shape.
    if trimmed.contains(".route(") || trimmed.contains("routing::delete") {
        return None;
    }

    FORBIDDEN_PATTERNS.iter().find(|p| line.contains(*p)).copied()
}

#[test]
fn handlers_must_not_perform_direct_repo_writes() {
    let mut files = Vec::new();
    walk_rs_files(&src_root(), &mut files);

    let mut violations = Vec::new();

    for file in &files {
        let rel = file
            .strip_prefix(src_root())
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");

        if !is_handler_file(&rel) || file_allowlisted(&rel) {
            continue;
        }

        let Ok(content) = fs::read_to_string(file) else { continue };
        for (lineno, line) in content.lines().enumerate() {
            if let Some(pattern) = matches_forbidden(line) {
                if line_allowlisted(&rel, line) {
                    continue;
                }
                violations.push(format!(
                    "  {rel}:{ln} — `{pattern}` in handler\n    {body}",
                    ln = lineno + 1,
                    body = line.trim(),
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Handler files are performing direct repo writes — route them through a use case:\n{}\n\n\
         If a handler legitimately needs a direct write (platform-infrastructure processing per \
         CLAUDE.md), add its file suffix to FILE_ALLOWLIST. For known TODOs, add a LINE_ALLOWLIST \
         entry so the backlog stays visible.",
        violations.join("\n"),
    );
}
