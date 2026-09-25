//! The profile-only gate (Go `middleware.ProfileOnlyWithoutRole`,
//! internal/platform/shared/middleware/profile_only.go).
//!
//! A USER principal holding no platform role and no permission (an SSO
//! just-in-time provision, an administrator whose roles were revoked)
//! authenticated but was never given platform access. Such a user reaches
//! only the self-service surface: `/auth/*`, `/portal/*` and `GET /api/me`.
//! Every other `/api` and `/bff` route answers 403 `NO_PLATFORM_ROLE`
//! before its handler runs, so an ungated read or a zero-role ANCHOR user's
//! tier cannot let it through.
//!
//! Unauthenticated requests pass untouched (each handler answers them), as
//! do service accounts, which authorize through their roles or granted
//! scope rather than through a role-less identity.

use axum::{
    extract::Request,
    http::Method,
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::shared::authorization_service::AuthContext;
use crate::shared::error::PlatformError;
use crate::shared::middleware::resolve_context;
use crate::PrincipalType;

/// Go's refusal message (profile_only.go:28).
pub const NO_PLATFORM_ROLE_MESSAGE: &str =
    "Your account has no platform access. Only your profile is available.";

/// Go `AuthContext.IsRoleless` (shared/auth/auth.go:216): a USER with no
/// role and no permission.
pub fn is_roleless(context: &AuthContext) -> bool {
    context.principal_type == PrincipalType::User
        && context.roles.is_empty()
        && context.permissions.is_empty()
}

/// Whether the gate looks at this request at all: the platform's
/// authenticated API surface (`/api`, `/bff`). Go mounts the gate on its
/// authenticated route group only; the routes Go serves outside it (the
/// public platform info and the SPA's feature flags, the router's dispatch
/// callback, the function contract documents) are outside here too, as are
/// `/auth/*`, `/oauth/*`, `/.well-known/*` and the SPA's static files.
fn gated_path(path: &str) -> bool {
    let api = path == "/api" || path.starts_with("/api/");
    let bff = path == "/bff" || path.starts_with("/bff/");
    if !(api || bff) {
        return false;
    }
    !(path.starts_with("/api/public/")
        || path == "/api/config/platform"
        || path.starts_with("/api/dispatch/")
        || path == "/api/openapi-functions.json"
        || path.starts_with("/api/schemas/"))
}

/// Go `profileAllowed` (profile_only.go:41-48) within the gated surface:
/// only `GET /api/me` itself (`/auth/*` and `/portal/*` are never gated).
fn profile_allowed(method: &Method, path: &str) -> bool {
    method == Method::GET && path == "/api/me"
}

/// The gate, as an axum middleware. Mount it inside the layer that installs
/// the auth services (`AuthLayer`): it authenticates the request once and
/// leaves the context for the handler's extractor.
pub async fn profile_only_without_role(request: Request, next: Next) -> Response {
    let path = request.uri().path();
    if !gated_path(path) || profile_allowed(request.method(), path) {
        return next.run(request).await;
    }
    let (mut parts, body) = request.into_parts();
    let refused = resolve_context(&mut parts)
        .await
        .is_some_and(|context| is_roleless(&context));
    if refused {
        return PlatformError::forbidden_code("NO_PLATFORM_ROLE", NO_PLATFORM_ROLE_MESSAGE)
            .into_response();
    }
    next.run(Request::from_parts(parts, body)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::authorization_service::Credential;
    use crate::UserScope;
    use std::collections::HashSet;

    fn context(kind: PrincipalType, roles: &[&str], permissions: &[&str]) -> AuthContext {
        AuthContext {
            principal_id: "prn_1".to_string(),
            principal_type: kind,
            scope: UserScope::Anchor,
            email: None,
            name: "n".to_string(),
            accessible_clients: vec!["*".to_string()],
            permissions: permissions
                .iter()
                .map(|p| p.to_string())
                .collect::<HashSet<_>>(),
            roles: roles.iter().map(|r| r.to_string()).collect(),
            credential: Credential::SessionCookie,
        }
    }

    #[test]
    fn only_a_user_with_no_role_and_no_permission_is_roleless() {
        assert!(is_roleless(&context(PrincipalType::User, &[], &[])));
        assert!(!is_roleless(&context(
            PrincipalType::User,
            &["platform:viewer"],
            &[]
        )));
        assert!(!is_roleless(&context(
            PrincipalType::User,
            &[],
            &["platform:iam:user:view"]
        )));
        assert!(!is_roleless(&context(PrincipalType::Service, &[], &[])));
    }

    /// Go's TestProfileOnlyWithoutRole paths.
    #[test]
    fn the_gate_covers_the_api_and_bff_surface_but_the_profile() {
        for (method, path) in [
            (Method::GET, "/bff/roles"),
            (Method::GET, "/api/clients"),
            (Method::GET, "/api/me/clients"),
            (Method::POST, "/api/audit-logs/batch"),
            (Method::POST, "/api/me"),
        ] {
            assert!(
                gated_path(path) && !profile_allowed(&method, path),
                "{method} {path}"
            );
        }
        for (method, path) in [
            (Method::GET, "/auth/me"),
            (Method::POST, "/auth/change-password"),
            (Method::GET, "/auth/2fa/status"),
            (Method::GET, "/api/me"),
            (Method::GET, "/api/public/platform"),
            (Method::GET, "/api/config/platform"),
            (Method::POST, "/api/dispatch/process"),
            (Method::GET, "/oauth/userinfo"),
            (Method::GET, "/assets/index.js"),
            (Method::GET, "/users"),
        ] {
            assert!(
                !gated_path(path) || profile_allowed(&method, path),
                "{method} {path}"
            );
        }
    }
}
