//! Role-name and permission shape (Go F18, `52c51e68`; Go
//! `internal/platform/role/name.go`).
//!
//! A role name is the canonical `{applicationCode}:{roleName}`: two or more
//! colon-separated, non-empty segments. A multi-colon name such as
//! `logistics_portal:dashboard:user` is tolerated so existing definitions stay
//! valid. Each segment is ASCII letters, digits and `_`, `.` and `-`: no
//! whitespace of any kind (ASCII or Unicode), no control or zero-width
//! characters, nothing that can render as a look-alike of a real name. Case is
//! not constrained (the SDK sync lower-cases, the admin create did not, and
//! both exist in stored data); the rule only has to make a name unambiguous.
//!
//! A permission is the same with one or more segments, and `*` allowed.
//!
//! A value that fails is REJECTED, never trimmed: silently repairing
//! `example: dispatcher` would hide the caller's bug.
//!
//! Go holds an assignment's role in a `role.Name` type only the domain can
//! build; Rust keeps `String` in `RoleAssignment`, so the write boundary is
//! these functions, called at every assignment write.

use crate::role::repository::RoleRepository;
use fc_platform_core::usecase::UseCaseError;

/// The 400 code for a role name of the wrong shape.
pub const INVALID_ROLE_NAME: &str = "INVALID_ROLE_NAME";

/// The 400 code for a permission string of the wrong shape.
pub const INVALID_PERMISSION: &str = "INVALID_PERMISSION";

/// The 400 code for a well-formed name that no role bears.
pub const ROLE_NOT_FOUND: &str = "ROLE_NOT_FOUND";

fn valid_segments(s: &str, min_segments: usize, allow_star: bool) -> bool {
    let segments: Vec<&str> = s.split(':').collect();
    if segments.len() < min_segments {
        return false;
    }
    segments.iter().all(|seg| {
        !seg.is_empty()
            && seg.bytes().all(|c| {
                c.is_ascii_alphanumeric()
                    || matches!(c, b'_' | b'.' | b'-')
                    || (allow_star && c == b'*')
            })
    })
}

/// Whether `name` is a well-formed canonical role name.
pub fn is_valid_name(name: &str) -> bool {
    valid_segments(name, 2, false)
}

/// 400 `INVALID_ROLE_NAME`, quoting the value, when `name` is malformed.
pub fn validate_name(name: &str) -> Result<(), UseCaseError> {
    if is_valid_name(name) {
        return Ok(());
    }
    Err(UseCaseError::validation(
        INVALID_ROLE_NAME,
        format!(
            "Invalid role name {name:?}: expected '{{application}}:{{role}}' with letters, digits, '_', '.' and '-' only, and no spaces"
        ),
    ))
}

/// [`validate_name`] over every name, the first offender's error.
pub fn validate_names<S: AsRef<str>>(names: &[S]) -> Result<(), UseCaseError> {
    names.iter().try_for_each(|n| validate_name(n.as_ref()))
}

/// 400 `INVALID_PERMISSION`, quoting the value, when `p` is malformed.
pub fn validate_permission(p: &str) -> Result<(), UseCaseError> {
    if valid_segments(p, 1, true) {
        return Ok(());
    }
    Err(UseCaseError::validation(
        INVALID_PERMISSION,
        format!(
            "Invalid permission {p:?}: colon-separated segments of letters, digits, '_', '.', '-' or '*', and no spaces"
        ),
    ))
}

/// [`validate_permission`] over every permission, the first offender's error.
pub fn validate_permissions<S: AsRef<str>>(permissions: &[S]) -> Result<(), UseCaseError> {
    permissions
        .iter()
        .try_for_each(|p| validate_permission(p.as_ref()))
}

/// Go `role.ResolveNames`: every name shape-checked (400 `INVALID_ROLE_NAME`)
/// and looked up (400 `ROLE_NOT_FOUND`). Returns the stored roles' own names,
/// in order. Shape first for every name, so a malformed value later in the
/// list wins over an unknown earlier one, as a validate phase would.
pub async fn resolve_names<S: AsRef<str>>(
    repo: &RoleRepository,
    names: &[S],
) -> Result<Vec<String>, UseCaseError> {
    let mut resolved = Vec::with_capacity(names.len());
    for name in names {
        let name = name.as_ref();
        validate_name(name)?;
        let role = repo.find_by_name(name).await?;
        match role {
            Some(r) => resolved.push(r.name),
            None => {
                return Err(UseCaseError::validation(
                    ROLE_NOT_FOUND,
                    format!("Role not found: {name}"),
                ))
            }
        }
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_names() {
        for n in [
            "platform:admin",
            "platform:super-admin",
            "example:dispatcher",
            "logistics_portal:dashboard:user",
            "orders:order.admin",
            "Orders:Admin",
            "a1:b2",
        ] {
            assert!(validate_name(n).is_ok(), "{n}");
        }
    }

    #[test]
    fn invalid_names_are_rejected_with_the_code_and_the_value() {
        for n in [
            "",
            " ",
            "example: dispatcher",
            "example :dispatcher",
            " example:dispatcher",
            "example:dispatcher ",
            "example:dis patcher",
            "example:dispatcher\n",
            "example:\tdispatcher",
            "example:\u{00a0}dispatcher",
            "example:\u{200b}dispatcher",
            "dispatcher",
            ":dispatcher",
            "example:",
            "example::dispatcher",
            "example:dispatcher:",
            "example:dis;patcher",
            "example:diß",
            "exa mple:dispatcher",
        ] {
            let err = validate_name(n).expect_err(n);
            assert_eq!(err.code(), INVALID_ROLE_NAME, "{n:?}");
            assert!(
                err.message().contains(&format!("{n:?}")),
                "{n:?}: {}",
                err.message()
            );
        }
    }

    #[test]
    fn permissions() {
        for p in [
            "platform:*:*:*",
            "orders:order:create",
            "platform:iam:user:view",
            "x",
        ] {
            assert!(validate_permission(p).is_ok(), "{p}");
        }
        for p in [
            "",
            " ",
            "orders: order:create",
            "orders:order:create ",
            "orders::create",
            ":a",
            "a:",
            "a:b c",
        ] {
            let err = validate_permission(p).expect_err(p);
            assert_eq!(err.code(), INVALID_PERMISSION, "{p:?}");
        }
    }

    #[test]
    fn a_list_reports_the_first_offender() {
        assert!(validate_names(&["a:b", "c:d"]).is_ok());
        let err = validate_names(&["a:b", "example: dispatcher"]).expect_err("offender");
        assert!(err.message().contains("\"example: dispatcher\""));
        let err = validate_permissions(&["a:b", "a: b"]).expect_err("offender");
        assert_eq!(err.code(), INVALID_PERMISSION);
    }
}
