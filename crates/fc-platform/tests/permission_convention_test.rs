//! Convention test: every write handler under `/api/*` must call an
//! authorization check.
//!
//! Since the URL-tier split (`/api/admin` vs `/api/sdk`) is gone, permissions
//! are the only thing gating write access. Missing a permission call on a
//! POST/PUT/PATCH/DELETE handler is a privilege-escalation bug. This test
//! scans every `#[utoipa::path(...)]`-annotated write handler and asserts
//! that its body contains one of the known auth-check patterns.
//!
//! If you add a legitimately-unauthenticated write handler (e.g. a platform
//! callback), add the file or handler name to one of the skip lists below
//! with a comment explaining why.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Any of these substrings in a handler body counts as a permission check.
/// Keep in sync with `shared::authorization_service::checks` and
/// `AuthorizationService` methods.
const AUTH_CHECK_PATTERNS: &[&str] = &[
    "require_anchor",
    "require_permission",
    "require_client_access",
    "is_admin(",
    "can_read_",
    "can_write_",
    "can_create_",
    "can_update_",
    "can_delete_",
    "can_retry_",
    ".authorize(",
];

/// Handler files that contain endpoints which legitimately don't need a
/// permission check (platform callbacks, public endpoints, health, etc.).
const FILE_SKIPLIST: &[&str] = &[
    // Unauthenticated public endpoints
    "shared/public_api.rs",
    "shared/well_known_api.rs",
    // OAuth 2.0 token / introspection / revocation — authenticated by client
    // credentials, not bearer JWT with permissions.
    "oauth/", // matches any oauth/* file
    // User-facing login/logout/password-reset flows. Auth is the act of
    // proving identity, not permission-gated.
    "auth/auth_api.rs",
    "auth/password_reset_api.rs",
    "auth/login_api.rs",
    "auth/oidc_login_api.rs",
    "auth/oidc_interaction_api.rs",
    "auth/client_selection_api.rs",
    // Platform infrastructure — called by internal message router, not users.
    "shared/dispatch_process_api.rs",
    // BFF surface (cookie-auth, /bff/*) — separate permission story.
    "shared/bff_",
    "shared/debug_api.rs",
    "shared/filter_options_api.rs",
    // Monitoring/observability — anchor-gated at the route layer.
    "shared/monitoring_api.rs",
    // /api/me — returns the caller's own identity; authenticated but no
    // further permission needed.
    "shared/me_api.rs",
    // The legacy SDK dispatch-jobs batch endpoint is being retired into the
    // main dispatch-jobs router; its auth is the bearer JWT of the calling
    // service account, not a granular permission.
    // TODO: add can_write_dispatch_jobs explicitly here.
    // "shared/sdk_dispatch_jobs_api.rs",
];

/// Specific handler function names to skip (by exact fn name match).
const FN_SKIPLIST: &[&str] = &[];

fn src_root() -> PathBuf {
    // Cargo sets CARGO_MANIFEST_DIR to the crate root when running tests.
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn should_skip(path: &Path) -> bool {
    let rel = path
        .strip_prefix(src_root())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    FILE_SKIPLIST.iter().any(|skip| rel.contains(skip))
}

fn walk_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            walk_rs_files(&p, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(p);
        }
    }
}

/// Parse write handlers out of a file. Each returned entry is
/// (fn_name, body_text, line_number).
fn extract_write_handlers(content: &str) -> Vec<(String, String, usize)> {
    let lines: Vec<&str> = content.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        // Find a `#[utoipa::path(` attribute.
        if line.trim_start().starts_with("#[utoipa::path(") {
            // Accumulate the attribute spanning multiple lines until the
            // matching `)]`.
            let mut attr = String::new();
            let attr_start = i;
            let mut depth = 0i32;
            let mut closed = false;
            while i < lines.len() {
                let l = lines[i];
                attr.push_str(l);
                attr.push('\n');
                for ch in l.chars() {
                    if ch == '(' {
                        depth += 1;
                    } else if ch == ')' {
                        depth -= 1;
                    }
                }
                i += 1;
                if depth <= 0 && attr.contains(")]") {
                    closed = true;
                    break;
                }
            }
            if !closed {
                continue;
            }

            // Check if first positional arg is a write method.
            let args_inside = attr
                .trim_start()
                .strip_prefix("#[utoipa::path(")
                .unwrap_or(&attr)
                .trim();
            let first_token = args_inside
                .split(|c: char| c == ',' || c.is_whitespace())
                .find(|t| !t.is_empty())
                .unwrap_or("");
            let is_write = matches!(first_token, "post" | "put" | "patch" | "delete");
            if !is_write {
                continue;
            }

            // Find the next `pub async fn NAME(` or `pub fn NAME(` or
            // `async fn NAME(` or `fn NAME(`.
            while i < lines.len() {
                let l = lines[i];
                if let Some(fn_name) = extract_fn_name(l) {
                    // Read the body: count braces starting from the first `{`.
                    let (body, _end) = read_balanced_body(&lines, i);
                    out.push((fn_name, body, attr_start + 1));
                    break;
                }
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    out
}

fn extract_fn_name(line: &str) -> Option<String> {
    // Match `fn NAME(` or `fn NAME<...>(`.
    let idx = line.find(" fn ")?;
    let after = &line[idx + 4..];
    let end = after
        .find(|c: char| c == '(' || c == '<' || c.is_whitespace())
        .unwrap_or(after.len());
    let name = after[..end].trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// Starting from `start_line` (which contains a `fn NAME(` signature), find
/// the matching `{ ... }` block and return its full text plus the index of
/// the line after the closing brace.
fn read_balanced_body(lines: &[&str], start_line: usize) -> (String, usize) {
    let mut depth = 0i32;
    let mut started = false;
    let mut body = String::new();
    let mut i = start_line;
    while i < lines.len() {
        let l = lines[i];
        body.push_str(l);
        body.push('\n');
        for ch in l.chars() {
            if ch == '{' {
                started = true;
                depth += 1;
            } else if ch == '}' {
                depth -= 1;
            }
        }
        i += 1;
        if started && depth == 0 {
            break;
        }
    }
    (body, i)
}

fn has_auth_check(body: &str) -> bool {
    AUTH_CHECK_PATTERNS.iter().any(|pat| body.contains(pat))
}

#[test]
fn every_write_handler_calls_an_auth_check() {
    let skip_fns: HashSet<&str> = FN_SKIPLIST.iter().copied().collect();

    let mut files = Vec::new();
    walk_rs_files(&src_root(), &mut files);

    let mut violations: Vec<String> = Vec::new();

    for file in &files {
        if should_skip(file) {
            continue;
        }
        let Ok(content) = fs::read_to_string(file) else {
            continue;
        };
        for (fn_name, body, line_no) in extract_write_handlers(&content) {
            if skip_fns.contains(fn_name.as_str()) {
                continue;
            }
            if !has_auth_check(&body) {
                let rel = file
                    .strip_prefix(src_root())
                    .unwrap_or(file)
                    .to_string_lossy()
                    .replace('\\', "/");
                violations.push(format!("{}:{} fn {}", rel, line_no, fn_name));
            }
        }
    }

    if !violations.is_empty() {
        let mut msg = String::from(
            "\n\nWrite handlers (POST/PUT/PATCH/DELETE) without an authorization check.\n\
             Every `/api/*` write handler must call one of: require_anchor, \
             require_permission, can_*, is_admin, or AuthorizationService::authorize.\n\
             If a handler legitimately needs no permission check (platform callback, \
             public endpoint, login flow), add it to FILE_SKIPLIST or FN_SKIPLIST \
             in this test with a comment explaining why.\n\n\
             Violators:\n",
        );
        for v in &violations {
            msg.push_str("  - ");
            msg.push_str(v);
            msg.push('\n');
        }
        panic!("{}", msg);
    }
}
