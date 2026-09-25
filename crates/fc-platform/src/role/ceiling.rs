//! The role ceiling: nobody hands out platform authority they do not hold
//! (owner ruling 14 of 2026-09-25; Java `RoleCeiling`, 3535091d).
//!
//! - **Roles.** A caller may add or remove a role only when it holds every
//!   one of that role's platform permissions. The rule applies to the change:
//!   roles the target keeps are not checked, so an administrator can still
//!   edit the other roles of someone who holds more than they do. Removal
//!   counts, so a lesser administrator cannot strip a super-admin. Refused
//!   with 403 `ROLE_ABOVE_CALLER`, naming the roles.
//! - **Permissions.** A caller may add a permission to a role, or remove one
//!   from it, only when it holds that permission; otherwise adding
//!   `platform:*:*:*` to a role it already holds would bypass the role rule in
//!   one edit. Refused with 403 `PERMISSION_ABOVE_CALLER`.
//!
//! **Platform permissions only.** The ceiling counts permissions whose first
//! segment is `platform`: that is the authority an escalation reaches for,
//! and what `platform:*:*:*` covers. An application's own permissions are held
//! by nobody outside that application's roles, not even a super-admin, so
//! counting them would stop anyone administering application roles.
//!
//! Matching is [`AuthContext::has_permission`]: a held wildcard covers what it
//! names, and a role's own wildcard (`platform:messaging:*:*`) is covered only
//! by a held wildcard at least as wide. A role name that does not exist grants
//! nothing, so it is never above anyone.

use std::collections::HashMap;

use crate::role::entity::AuthRole;
use crate::role::repository::RoleRepository;
use crate::shared::authorization_service::AuthContext;
use crate::usecase::UseCaseError;

const PLATFORM_PREFIX: &str = "platform:";

/// What a change adds or removes: `after` minus `before`, then `before` minus
/// `after`, each in order and without repeats.
pub fn changed<S: AsRef<str>>(before: &[S], after: &[S]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: &str| {
        if !out.iter().any(|o| o == s) {
            out.push(s.to_string());
        }
    };
    for a in after {
        if !before.iter().any(|b| b.as_ref() == a.as_ref()) {
            push(a.as_ref());
        }
    }
    for b in before {
        if !after.iter().any(|a| a.as_ref() == b.as_ref()) {
            push(b.as_ref());
        }
    }
    out
}

/// The platform permissions in `permissions` that `caller` does not hold, in
/// order and without repeats. `None` (no caller) holds nothing.
pub fn permissions_above<'a>(
    caller: Option<&AuthContext>,
    permissions: impl IntoIterator<Item = &'a str>,
) -> Vec<String> {
    let mut above: Vec<String> = Vec::new();
    for p in permissions {
        if !p.starts_with(PLATFORM_PREFIX) {
            continue;
        }
        if caller.is_some_and(|c| c.has_permission(p)) {
            continue;
        }
        if !above.iter().any(|a| a == p) {
            above.push(p.to_string());
        }
    }
    above
}

/// The roles in `changed` whose permissions `caller` does not all hold, in
/// the order given. `definitions` maps a role name to its role; a name with
/// no definition grants nothing.
pub fn roles_above(
    caller: Option<&AuthContext>,
    changed: &[String],
    definitions: &HashMap<String, AuthRole>,
) -> Vec<String> {
    changed
        .iter()
        .filter(|name| {
            definitions.get(name.as_str()).is_some_and(|role| {
                !permissions_above(caller, role.permissions.iter().map(String::as_str)).is_empty()
            })
        })
        .cloned()
        .collect()
}

/// Refuses when any changed role is above `caller`: 403 `ROLE_ABOVE_CALLER`.
pub fn require_roles(
    caller: Option<&AuthContext>,
    changed: &[String],
    definitions: &HashMap<String, AuthRole>,
) -> Result<(), UseCaseError> {
    let above = roles_above(caller, changed, definitions);
    if above.is_empty() {
        Ok(())
    } else {
        Err(UseCaseError::forbidden(
            "ROLE_ABOVE_CALLER",
            format!(
                "You may only assign or remove roles whose permissions you hold yourself: {}",
                above.join(", ")
            ),
        ))
    }
}

/// Refuses when any changed permission is one `caller` does not hold: 403
/// `PERMISSION_ABOVE_CALLER`.
pub fn require_permissions<'a>(
    caller: Option<&AuthContext>,
    changed: impl IntoIterator<Item = &'a str>,
) -> Result<(), UseCaseError> {
    let above = permissions_above(caller, changed);
    if above.is_empty() {
        Ok(())
    } else {
        Err(UseCaseError::forbidden(
            "PERMISSION_ABOVE_CALLER",
            format!(
                "You may only add or remove permissions you hold yourself: {}",
                above.join(", ")
            ),
        ))
    }
}

/// The definitions of `names`, keyed by name, in one query.
pub async fn definitions(
    role_repo: &RoleRepository,
    names: &[String],
) -> Result<HashMap<String, AuthRole>, UseCaseError> {
    if names.is_empty() {
        return Ok(HashMap::new());
    }
    let roles = role_repo
        .find_by_codes(names)
        .await
        .map_err(|e| UseCaseError::commit(format!("Failed to load roles: {e}")))?;
    Ok(roles.into_iter().map(|r| (r.name.clone(), r)).collect())
}

/// [`require_roles`] for a change from `before` to `after`, loading the
/// changed roles' definitions.
pub async fn require_role_change(
    caller: &AuthContext,
    role_repo: &RoleRepository,
    before: &[String],
    after: &[String],
) -> Result<(), UseCaseError> {
    let changed = changed(before, after);
    if changed.is_empty() {
        return Ok(());
    }
    let definitions = definitions(role_repo, &changed).await?;
    require_roles(Some(caller), &changed, &definitions)
}

/// [`require_role_change`] for lists that name a role by name or by id
/// (email-domain `allowedRoleIds`). A reference that matches no role grants
/// nothing.
pub async fn require_role_ref_change(
    caller: &AuthContext,
    role_repo: &RoleRepository,
    before: &[String],
    after: &[String],
) -> Result<(), UseCaseError> {
    let changed_refs = changed(before, after);
    if changed_refs.is_empty() {
        return Ok(());
    }
    let roles = role_repo
        .find_by_names_or_ids(&changed_refs)
        .await
        .map_err(|e| UseCaseError::commit(format!("Failed to load roles: {e}")))?;
    let names: Vec<String> = changed_refs
        .iter()
        .filter_map(|r| roles.iter().find(|role| &role.name == r || &role.id == r))
        .map(|role| role.name.clone())
        .collect();
    let definitions: HashMap<String, AuthRole> =
        roles.into_iter().map(|r| (r.name.clone(), r)).collect();
    require_roles(Some(caller), &names, &definitions)
}

/// A caller for unit tests: the given tier, clients and permissions. The
/// one place these IAM tests build an [`AuthContext`] by hand.
#[cfg(test)]
pub(crate) fn test_caller(
    scope: crate::principal::entity::UserScope,
    clients: &[&str],
    perms: &[&str],
) -> AuthContext {
    AuthContext {
        principal_id: "prn_caller".to_string(),
        principal_type: crate::PrincipalType::User,
        scope,
        email: None,
        name: "Caller".to_string(),
        accessible_clients: clients.iter().map(|s| s.to_string()).collect(),
        permissions: perms.iter().map(|s| s.to_string()).collect(),
        roles: vec![],
        credential: crate::shared::authorization_service::Credential::BearerToken,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::principal::entity::UserScope;
    use crate::role::entity::roles;

    fn caller(perms: &[&str]) -> AuthContext {
        test_caller(UserScope::Anchor, &["*"], perms)
    }

    fn defs(roles: Vec<AuthRole>) -> HashMap<String, AuthRole> {
        roles.into_iter().map(|r| (r.name.clone(), r)).collect()
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn changed_is_added_then_removed() {
        assert_eq!(
            changed(&s(&["a", "b", "c"]), &s(&["b", "d", "d"])),
            s(&["d", "a", "c"])
        );
        assert!(changed(&s(&["a"]), &s(&["a"])).is_empty());
    }

    #[test]
    fn wildcards_are_honoured_and_only_as_wide() {
        let iam = caller(&["platform:iam:*:*"]);
        assert!(permissions_above(Some(&iam), ["platform:iam:user:create"]).is_empty());
        // A role's own wildcard needs one at least as wide.
        assert!(permissions_above(Some(&iam), ["platform:iam:*:*"]).is_empty());
        assert_eq!(
            permissions_above(Some(&iam), ["platform:*:*:*"]),
            s(&["platform:*:*:*"])
        );
        let user_writer = caller(&["platform:iam:user:*"]);
        assert_eq!(
            permissions_above(Some(&user_writer), ["platform:iam:*:*"]),
            s(&["platform:iam:*:*"])
        );
        // Nobody holds anything without a caller.
        assert_eq!(
            permissions_above(None, ["platform:iam:user:view"]),
            s(&["platform:iam:user:view"])
        );
    }

    #[test]
    fn application_permissions_are_not_counted() {
        let nobody = caller(&[]);
        assert!(permissions_above(Some(&nobody), ["orders:order:create", "hr:x:y:z"]).is_empty());
        let super_admin = caller(&["platform:*:*:*"]);
        let orders =
            AuthRole::new("orders", "clerk", "Clerk").with_permission("orders:order:create");
        assert!(roles_above(
            Some(&super_admin),
            &s(&["orders:clerk"]),
            &defs(vec![orders])
        )
        .is_empty());
    }

    #[test]
    fn a_role_is_above_unless_every_permission_is_held() {
        let iam_admin = caller(
            &roles::iam_admin()
                .permissions
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        );
        let all = defs(vec![
            roles::super_admin(),
            roles::iam_readonly(),
            roles::viewer(),
            roles::iam_admin(),
        ]);
        let changed = s(&[
            "platform:iam-readonly",
            "platform:super-admin",
            "platform:viewer",
            "platform:iam-admin",
            "platform:no-such-role",
        ]);
        assert_eq!(
            roles_above(Some(&iam_admin), &changed, &all),
            s(&["platform:super-admin", "platform:viewer"])
        );
        let err = require_roles(Some(&iam_admin), &changed, &all).unwrap_err();
        assert_eq!(err.code(), "ROLE_ABOVE_CALLER");
        assert_eq!(err.http_status_code(), 403);
        assert!(err
            .message()
            .contains("platform:super-admin, platform:viewer"));

        let super_admin = caller(&["platform:*:*:*"]);
        assert!(require_roles(Some(&super_admin), &changed, &all).is_ok());
    }

    #[test]
    fn permission_edits_are_bounded() {
        let role_writer = caller(&["platform:iam:role:update"]);
        let err = require_permissions(Some(&role_writer), ["platform:*:*:*"]).unwrap_err();
        assert_eq!(err.code(), "PERMISSION_ABOVE_CALLER");
        assert!(require_permissions(
            Some(&role_writer),
            ["platform:iam:role:update", "orders:a:b:c"]
        )
        .is_ok());
    }
}
