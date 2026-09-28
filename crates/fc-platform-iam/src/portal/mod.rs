//! Portal identity plane (the IAM model) — Go's `internal/platform/portalidentity`,
//! `internal/platform/portalauth` and the portal hooks of the OIDC bridge,
//! the reset-token flows and the token endpoint.
//!
//! Portal end-users are a SEPARATE identity population from `iam_principals`:
//! one identity per (client, email) context, holding its own password (or
//! signing in through the IdP that owns its email domain), granted per portal
//! app. The plane has:
//!
//! - an admin surface, `/api/portal-users` and `/api/portal-apps`
//!   ([`api`]), gated by the client-delegable portal permissions
//!   ([`can_read_portal_users`], [`can_write_portal_users`]);
//! - a login surface, `/portal/*` ([`login_api`]), independent of the
//!   employee one: it never reads or writes `fc_session`, and issues
//!   authorization codes whose subject is the `ptu_…` identity id;
//! - the portal branches of shared endpoints: the reset-token confirm and
//!   validate ([`password`]), `/oauth/token` ([`token`]) and the OIDC
//!   callback ([`oidc`]).
//!
//! Here: the identities and portal apps (`entity`, `repository`), the
//! password policy, the portal flags on OAuth clients and the portal
//! permission checks. The admin and login surfaces and the state they run
//! on are fc-platform-auth's `portal`, which re-exports this module.

pub mod entity;
pub mod policy;
pub mod repository;

pub use entity::{is_portal_subject, trimmed_or_none, PortalApp, PortalIdentity};

use fc_platform_core::permissions;
use fc_platform_core::shared::authorization_service::Authority;
use fc_platform_core::shared::error::{PlatformError, Result};
use repository::PortalAppRepository;

// ── The portal flags on OAuth clients (Go auth/api + auth/operations) ────

/// Go `validatePlaneFlags`: a portal app can only be linked to a portal
/// client. (Rust has no `apiAccess` flag, so Go's portal + apiAccess
/// conflict cannot arise.)
pub fn validate_oauth_client_plane(
    client: &crate::auth::oauth_entity::OAuthClient,
) -> std::result::Result<(), fc_platform_core::usecase::UseCaseError> {
    let is_portal = client
        .portal_client_id
        .as_deref()
        .is_some_and(|p| !p.is_empty());
    // Go validatePlaneFlags: portal identities never carry platform
    // authority, so a portal client cannot be an API-access client.
    if client.api_access && is_portal {
        return Err(fc_platform_core::usecase::UseCaseError::validation(
            "PORTAL_API_ACCESS_CONFLICT",
            "a portal client cannot have apiAccess — portal identities never carry platform authority",
        ));
    }
    if client
        .portal_app_id
        .as_deref()
        .is_some_and(|a| !a.is_empty())
        && !is_portal
    {
        return Err(fc_platform_core::usecase::UseCaseError::validation(
            "PORTAL_APP_REQUIRES_PORTAL_CLIENT",
            "a portal app can only be linked to a portal client (portalClientId)",
        ));
    }
    Ok(())
}

/// Go `authapi.resolvePortalApp`: a linked portal app is authoritative for
/// the portal owner — `portalAppId` (when non-empty) must name an existing
/// app, and `portalClientId` becomes that app's client; a conflicting
/// explicit `portalClientId` is refused.
pub async fn resolve_oauth_client_portal_app(
    apps: &PortalAppRepository,
    portal_app_id: Option<&str>,
    portal_client_id: &mut Option<String>,
) -> Result<()> {
    let Some(app_id) = portal_app_id.map(str::trim).filter(|a| !a.is_empty()) else {
        return Ok(());
    };
    let app = apps.find_by_id(app_id).await?.ok_or_else(|| {
        PlatformError::from(fc_platform_core::usecase::UseCaseError::not_found(
            fc_platform_core::shared::error::not_found_code("PortalApp"),
            format!("PortalApp not found: {app_id}"),
        ))
    })?;
    if portal_client_id
        .as_deref()
        .map(str::trim)
        .is_some_and(|pc| !pc.is_empty() && pc != app.client_id)
    {
        return Err(PlatformError::from(
            fc_platform_core::usecase::UseCaseError::validation(
                "PORTAL_APP_CLIENT_MISMATCH",
                "portalAppId belongs to a different client than portalClientId",
            ),
        ));
    }
    *portal_client_id = Some(app.client_id);
    Ok(())
}

// ── Permission checks (Go shared/auth CanReadPortalUsers /
// CanManagePortalUsers) ─────────────────────────────────────────────────────
//
// The plane is CLIENT-delegable: anchors pass everywhere; a client-scoped
// caller (a client administrator in the platform UI, or the portal
// application's confined service account) needs access to the target client
// AND the portal-user permission, so a client manages its own portal
// population and nobody else's.

fn scope_forbidden() -> PlatformError {
    PlatformError::forbidden_code("SCOPE_FORBIDDEN", "no access to this client")
}

/// Listing a client's portal identities (and its portal apps).
pub fn can_read_portal_users(ctx: &impl Authority, client_id: &str) -> Result<()> {
    if ctx.is_anchor() {
        return Ok(());
    }
    if !ctx.can_access_client(client_id) {
        return Err(scope_forbidden());
    }
    let any = [
        permissions::iam::PORTAL_USER_READ,
        permissions::iam::PORTAL_USER_MANAGE,
    ];
    if ctx.has_any_permission(&any) {
        Ok(())
    } else {
        Err(PlatformError::forbidden_code(
            "PERMISSION_REQUIRED",
            format!("one of: {}", any.join(", ")),
        ))
    }
}

/// Ensure/invite, suspension, deletion and app administration of a client's
/// portal identities (Go `CanManagePortalUsers`).
pub fn can_write_portal_users(ctx: &impl Authority, client_id: &str) -> Result<()> {
    if ctx.is_anchor() {
        return Ok(());
    }
    if !ctx.can_access_client(client_id) {
        return Err(scope_forbidden());
    }
    if ctx.has_permission(permissions::iam::PORTAL_USER_MANAGE) {
        Ok(())
    } else {
        Err(PlatformError::forbidden_code(
            "PERMISSION_REQUIRED",
            format!(
                "permission required: {}",
                permissions::iam::PORTAL_USER_MANAGE
            ),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fc_platform_core::principal_kind::UserScope;
    use fc_platform_core::shared::authorization_service::Credential;
    use std::collections::HashSet;

    fn ctx(
        scope: UserScope,
        clients: &[&str],
        perms: &[&str],
    ) -> fc_platform_core::shared::authorization_service::AuthContext {
        fc_platform_core::shared::authorization_service::AuthContext {
            principal_id: "prn_1".into(),
            principal_type: fc_platform_core::principal_kind::PrincipalType::User,
            scope,
            email: None,
            name: "x".into(),
            accessible_clients: clients.iter().map(|c| c.to_string()).collect(),
            permissions: perms.iter().map(|p| p.to_string()).collect::<HashSet<_>>(),
            roles: vec![],
            credential: Credential::BearerToken,
        }
    }

    #[test]
    fn anchors_pass_without_a_permission() {
        let a = ctx(UserScope::Anchor, &["*"], &[]);
        assert!(can_read_portal_users(&a, "clt_1").is_ok());
        assert!(can_write_portal_users(&a, "clt_1").is_ok());
    }

    #[test]
    fn client_callers_need_reach_and_the_permission() {
        let manage = permissions::iam::PORTAL_USER_MANAGE;
        let view = permissions::iam::PORTAL_USER_READ;
        let other = ctx(UserScope::Client, &["clt_2"], &[manage]);
        assert!(can_write_portal_users(&other, "clt_1").is_err());
        let viewer = ctx(UserScope::Client, &["clt_1"], &[view]);
        assert!(can_read_portal_users(&viewer, "clt_1").is_ok());
        assert!(can_write_portal_users(&viewer, "clt_1").is_err());
        let manager = ctx(UserScope::Client, &["clt_1"], &[manage]);
        assert!(can_read_portal_users(&manager, "clt_1").is_ok());
        assert!(can_write_portal_users(&manager, "clt_1").is_ok());
    }
}
