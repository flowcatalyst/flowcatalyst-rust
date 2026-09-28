//! Convention test: `DELETE FROM iam_principal_roles WHERE role_name = ...`
//! must only appear inside `role/repository.rs`. Anywhere else is a sign that
//! someone is hand-rolling junction cleanup instead of routing through the
//! repository's delete path, which is the code-managed referential integrity
//! boundary.
//!
//! The underlying junction has no DB-level FK (by design — integrity is held
//! in code). Every role-deletion path MUST cascade this junction, and the
//! only place that should know how is the role repository.
//!
//! Complements:
//!   - `uow_convention_test.rs` — use cases must go through UoW
//!   - `permission_convention_test.rs` — handlers must check permissions
//!
//! If this test flags a file, fix the file (route the delete through
//! `RoleRepository`) — don't add to the skiplist unless there's a genuinely
//! good reason (and add a comment in the skiplist explaining why).
//!
//! The same shape applies to the other non-FK junctions; add more patterns
//! below as similar use cases emerge.

use std::fs;
use std::path::{Path, PathBuf};

/// Patterns that should be confined to a specific file. Second element is
/// the allowed file suffix (substring of the relative path from src/).
const CONFINED_PATTERNS: &[(&str, &str)] = &[
    // Role-name junction cleanup must live inside RoleRepository.
    (
        "DELETE FROM iam_principal_roles WHERE role_name",
        "role/repository.rs",
    ),
    // Application-id junction cleanup must live inside ApplicationRepository.
    (
        "DELETE FROM iam_principal_application_access WHERE application_id",
        "application/repository.rs",
    ),
    // Client-id junction cleanup must live inside ClientRepository.
    (
        "DELETE FROM iam_client_access_grants WHERE client_id",
        "client/repository.rs",
    ),
];

fn src_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
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

#[test]
fn cascade_sql_is_confined_to_the_owning_repository() {
    let mut files = Vec::new();
    walk_rs_files(&src_root(), &mut files);

    let mut violations = Vec::new();

    for file in &files {
        let rel = file
            .strip_prefix(src_root())
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");

        let Ok(content) = fs::read_to_string(file) else {
            continue;
        };

        for (pattern, allowed_suffix) in CONFINED_PATTERNS {
            if content.contains(pattern) && !rel.ends_with(allowed_suffix) {
                violations.push(format!(
                    "  {rel}: contains `{pattern}` — this cascade SQL must only live in `{allowed_suffix}`"
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Junction cascade SQL leaked outside its owning repository:\n{}\n\n\
         Route the delete through the aggregate's repository (or its Persist::delete impl) \
         so the cascade is centralised and can't drift.",
        violations.join("\n"),
    );
}
