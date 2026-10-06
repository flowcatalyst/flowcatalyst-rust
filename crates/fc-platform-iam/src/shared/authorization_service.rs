//! Authorization Service
//!
//! Permission-based access control with role resolution: the
//! repository-backed services. [`AuthContext`], [`Authority`], the
//! [`checks`] and [`ApplicationScope`] are fc-platform-core's (every
//! aggregate uses them) and are re-exported here at their old paths.

pub use fc_platform_core::shared::authorization_service::*;

use crate::application::entity::Application;
use crate::application::repository;
use crate::auth::auth_service;
use crate::auth::auth_service::AccessTokenClaims;
use crate::principal::entity::Principal;
use crate::role::repository::RoleRepository;
use crate::{
    application::repository::ApplicationRepository, principal::repository::PrincipalRepository,
};
use dashmap::DashMap;
use fc_platform_core::directory::ApplicationAccess;
use fc_platform_core::directory::ApplicationRef;
use fc_platform_core::principal_kind::{PrincipalType, UserScope};
use fc_platform_core::shared::error::{PlatformError, Result};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

/// The [`AuthContext`] of a bearer token: its claims, with the permissions
/// its roles resolved to.
pub fn auth_context_from_claims(
    claims: &AccessTokenClaims,
    permissions: HashSet<String>,
) -> AuthContext {
    AuthContext {
        principal_id: claims.sub.clone(),
        principal_type: claims.principal_type,
        // Only an identity-only token lacks a tier, and `build_context`
        // refuses those; were one to get here, CLIENT with no clients
        // reaches nothing.
        scope: claims.tier.unwrap_or(UserScope::Client),
        email: claims.email.clone(),
        name: claims.name.clone(),
        accessible_clients: claims
            .clients
            .iter()
            .map(|c| client_id_of(c).to_string())
            .collect(),
        permissions,
        roles: claims.roles.clone(),
        credential: Credential::BearerToken,
    }
}

/// The context of a signed-in browser session: the principal as it is
/// in the database now (Go `middleware.introspect`'s cookie path,
/// shared/middleware/middleware.go:163-183, over `provider.BuildClaims`).
/// Clients are the home client and the assigned ones as bare ids, `*`
/// for an anchor.
pub fn auth_context_for_session(
    principal: &Principal,
    permissions: HashSet<String>,
) -> AuthContext {
    let accessible_clients = if principal.scope.is_anchor() {
        vec!["*".to_string()]
    } else {
        let mut clients = principal.assigned_clients.clone();
        if let Some(home) = &principal.client_id {
            if !clients.iter().any(|c| c == home.as_str()) {
                clients.push(home.to_string());
            }
        }
        clients
    };
    AuthContext {
        principal_id: principal.id.to_string(),
        principal_type: principal.principal_type,
        scope: principal.scope,
        email: principal.email().map(String::from),
        name: principal.name.clone(),
        accessible_clients,
        permissions,
        roles: auth_service::role_names(principal),
        credential: Credential::SessionCookie,
    }
}

/// The client id in one `clients` claim entry: the claim carries
/// `id:identifier` pairs (or `*`) and everything inward reasons in bare ids
/// (Go `auth.ParseClientsClaim`; Java `ScopeClaim`). Client ids never
/// contain `:`.
fn client_id_of(entry: &str) -> &str {
    entry.split_once(':').map_or(entry, |(id, _)| id)
}

/// Cached permission entry with TTL
struct CachedPermissions {
    permissions: HashSet<String>,
    cached_at: Instant,
}

/// Cache TTL for resolved permissions (60 seconds)
const PERMISSION_CACHE_TTL_SECS: u64 = 60;

/// Authorization service for checking permissions
pub struct AuthorizationService {
    role_repo: Arc<RoleRepository>,
    /// Where a session cookie's principal is reloaded from; without it no
    /// session authenticates.
    principals: Option<Arc<PrincipalRepository>>,
    /// Cache: sorted role codes joined by "," → resolved permissions
    permission_cache: DashMap<String, CachedPermissions>,
}

impl AuthorizationService {
    pub fn new(role_repo: Arc<RoleRepository>) -> Self {
        Self {
            role_repo,
            principals: None,
            permission_cache: DashMap::new(),
        }
    }

    /// Reload session-cookie principals from `principals`.
    pub fn with_session_principals(mut self, principals: Arc<PrincipalRepository>) -> Self {
        self.principals = Some(principals);
        self
    }

    /// The context for a session cookie's subject, reloaded from the
    /// database on every request, as Go does: no principal cache, so a
    /// deactivation, deletion, role or client change takes effect on the
    /// very next request. Role → permission resolution keeps its usual
    /// 60-second cache. `None` when the principal is unknown, inactive or
    /// not a USER (only an interactive login mints a session), or when no
    /// principal store is configured.
    pub async fn session_context(&self, principal_id: &str) -> Result<Option<AuthContext>> {
        let Some(principals) = &self.principals else {
            return Ok(None);
        };
        let Some(principal) = principals.find_by_id(principal_id).await? else {
            return Ok(None);
        };
        if !principal.active || principal.principal_type != PrincipalType::User {
            return Ok(None);
        }
        let permissions = self
            .resolve_permissions(&auth_service::role_names(&principal))
            .await?;
        Ok(Some(auth_context_for_session(&principal, permissions)))
    }

    /// Build an authorization context from JWT claims
    /// Resolves all permissions from roles (cached)
    ///
    /// As Go's middleware `introspect` (shared/middleware/middleware.go:185-213):
    /// an identity-only token (`token_use: identity`) authorizes nothing and
    /// is refused; a token whose `scope` carries granted permissions uses
    /// exactly those; one without derives them from its roles.
    pub async fn build_context(&self, claims: &AccessTokenClaims) -> Result<AuthContext> {
        if claims.is_identity_only() {
            return Err(PlatformError::InvalidToken {
                message: IDENTITY_TOKEN_NOT_API_CREDENTIAL.to_string(),
            });
        }
        let granted = claims.granted_permissions();
        let permissions = if granted.is_empty() {
            self.resolve_permissions(&claims.roles).await?
        } else {
            granted.into_iter().collect()
        };
        Ok(auth_context_from_claims(claims, permissions))
    }

    /// Resolve all permissions for a set of role codes, with in-memory caching
    async fn resolve_permissions(&self, role_codes: &[String]) -> Result<HashSet<String>> {
        if role_codes.is_empty() {
            return Ok(HashSet::new());
        }

        // Build cache key from sorted role codes
        let mut sorted_codes = role_codes.to_vec();
        sorted_codes.sort();
        let cache_key = sorted_codes.join(",");

        // Check cache
        if let Some(entry) = self.permission_cache.get(&cache_key) {
            if entry.cached_at.elapsed().as_secs() < PERMISSION_CACHE_TTL_SECS {
                return Ok(entry.permissions.clone());
            }
        }

        // Cache miss or expired — query DB
        let roles = self.role_repo.find_by_codes(role_codes).await?;
        let mut permissions = HashSet::new();

        for role in roles {
            permissions.extend(role.permissions);
        }

        // Store in cache
        self.permission_cache.insert(
            cache_key,
            CachedPermissions {
                permissions: permissions.clone(),
                cached_at: Instant::now(),
            },
        );

        Ok(permissions)
    }

    /// Check if a principal can perform an action on a resource
    pub fn authorize(
        &self,
        context: &AuthContext,
        permission: &str,
        client_id: Option<&str>,
    ) -> Result<()> {
        // Check permission
        if !context.has_permission(permission) {
            return Err(PlatformError::forbidden(format!(
                "Missing permission: {}",
                permission
            )));
        }

        // Check client access if client-specific
        if let Some(cid) = client_id {
            if !context.can_access_client(cid) {
                return Err(PlatformError::forbidden(format!(
                    "No access to client: {}",
                    cid
                )));
            }
        }

        Ok(())
    }

    /// Require anchor scope
    pub fn require_anchor(&self, context: &AuthContext) -> Result<()> {
        if !context.is_anchor() {
            return Err(PlatformError::forbidden("Anchor scope required"));
        }
        Ok(())
    }

    /// Require specific permission
    pub fn require_permission(&self, context: &AuthContext, permission: &str) -> Result<()> {
        if !context.has_permission(permission) {
            return Err(PlatformError::forbidden(format!(
                "Permission required: {}",
                permission
            )));
        }
        Ok(())
    }

    /// Require client access
    pub fn require_client_access(&self, context: &AuthContext, client_id: &str) -> Result<()> {
        if !context.can_access_client(client_id) {
            return Err(PlatformError::forbidden(format!(
                "Client access required: {}",
                client_id
            )));
        }
        Ok(())
    }
}

/// Cached application scope with TTL
struct CachedApplicationScope {
    scope: ApplicationScope,
    cached_at: Instant,
}

/// Resolves `/{appCode}` path targets against the caller's application
/// scope. The scope costs one indexed query per principal, cached for the
/// same TTL as resolved permissions.
pub struct ApplicationAccessService {
    principal_repo: Arc<PrincipalRepository>,
    application_repo: Arc<ApplicationRepository>,
    /// Cache: principal id → application scope
    scope_cache: DashMap<String, CachedApplicationScope>,
}

impl ApplicationAccessService {
    pub fn new(
        principal_repo: Arc<PrincipalRepository>,
        application_repo: Arc<ApplicationRepository>,
    ) -> Self {
        Self {
            principal_repo,
            application_repo,
            scope_cache: DashMap::new(),
        }
    }

    /// The caller's application scope (cached).
    pub async fn scope_for(&self, principal_id: &str) -> Result<ApplicationScope> {
        if let Some(entry) = self.scope_cache.get(principal_id) {
            if entry.cached_at.elapsed().as_secs() < PERMISSION_CACHE_TTL_SECS {
                return Ok(entry.scope.clone());
            }
        }

        let binding = self
            .principal_repo
            .find_application_binding(principal_id)
            .await?;
        let scope = ApplicationScope::from_binding(binding);
        self.scope_cache.insert(
            principal_id.to_string(),
            CachedApplicationScope {
                scope: scope.clone(),
                cached_at: Instant::now(),
            },
        );
        Ok(scope)
    }

    /// Drop a principal's cached scope, after its application access changed.
    pub fn forget(&self, principal_id: &str) {
        self.scope_cache.remove(principal_id);
    }

    /// Resolve the application named by `app_code` and require the caller
    /// may act on it. Call after the handler's permission check. A missing
    /// application and one outside the caller's scope give the same 404.
    pub async fn require_application_access(
        &self,
        context: &AuthContext,
        app_code: &str,
    ) -> Result<Application> {
        let (application, scope) = tokio::try_join!(
            self.application_repo.find_by_code(app_code),
            self.scope_for(&context.principal_id),
        )?;
        checks::require_application_access(&scope, app_code, application)
    }
}

#[async_trait::async_trait]
impl ApplicationAccess for ApplicationAccessService {
    async fn require_application_access(
        &self,
        context: &AuthContext,
        app_code: &str,
    ) -> Result<ApplicationRef> {
        ApplicationAccessService::require_application_access(self, context, app_code)
            .await
            .map(repository::application_ref)
    }

    async fn scope_for(&self, principal_id: &str) -> Result<ApplicationScope> {
        ApplicationAccessService::scope_for(self, principal_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fc_platform_core::permissions;
    use fc_platform_core::shared::id::ApplicationId;

    fn create_test_context(permissions: Vec<&str>, scope: &str, clients: Vec<&str>) -> AuthContext {
        AuthContext {
            principal_id: "test123".to_string(),
            principal_type: PrincipalType::User,
            scope: scope.parse().unwrap(),
            email: Some("test@example.com".to_string()),
            name: "Test User".to_string(),
            accessible_clients: clients.into_iter().map(String::from).collect(),
            permissions: permissions.into_iter().map(String::from).collect(),
            roles: vec!["test:admin".to_string()],
            credential: Credential::BearerToken,
        }
    }

    /// A bearer's `clients` claim carries `id:identifier` pairs (Go
    /// `buildClients`); client checks and every reader of
    /// `accessible_clients` take bare ids.
    #[test]
    fn clients_claim_pairs_become_bare_ids() {
        let claims: AccessTokenClaims = serde_json::from_value(serde_json::json!({
            "sub": "prn_1", "iss": "i", "aud": "a", "exp": 2, "iat": 1,
            "type": "SERVICE", "tier": "PARTNER",
            "clients": ["clt_A:acme", "clt_B"]
        }))
        .unwrap();
        let ctx = auth_context_from_claims(&claims, HashSet::new());
        assert_eq!(ctx.accessible_clients, vec!["clt_A", "clt_B"]);
        assert!(ctx.can_access_client("clt_A"));
        assert!(ctx.can_access_client("clt_B"));
        assert!(!ctx.can_access_client("acme"));
        assert!(!ctx.can_access_client("clt_C"));
        assert_eq!(client_id_of("*"), "*");
    }

    #[test]
    fn test_direct_permission() {
        let ctx = create_test_context(vec!["platform:admin:event:read"], "CLIENT", vec!["client1"]);
        assert!(ctx.has_permission("platform:admin:event:read"));
        assert!(!ctx.has_permission("platform:admin:event:create"));
    }

    #[test]
    fn test_wildcard_permission_4_level() {
        let ctx = create_test_context(vec!["platform:admin:*:*"], "CLIENT", vec!["client1"]);
        assert!(ctx.has_permission("platform:admin:event:read"));
        assert!(ctx.has_permission("platform:admin:client:create"));
        assert!(!ctx.has_permission("platform:iam:user:read"));
    }

    #[test]
    fn test_superuser_permission() {
        let ctx = create_test_context(vec!["platform:*:*:*"], "ANCHOR", vec!["*"]);
        assert!(ctx.has_permission("platform:admin:event:read"));
        assert!(ctx.has_permission("platform:iam:user:delete"));
        assert!(ctx.has_permission("platform:auth:oauth-client:read"));
    }

    #[test]
    fn test_client_access() {
        let ctx = create_test_context(vec![], "CLIENT", vec!["client1", "client2"]);
        assert!(ctx.can_access_client("client1"));
        assert!(ctx.can_access_client("client2"));
        assert!(!ctx.can_access_client("client3"));
    }

    #[test]
    fn test_anchor_all_clients() {
        let ctx = create_test_context(vec![], "ANCHOR", vec!["*"]);
        assert!(ctx.can_access_client("any_client"));
        assert!(ctx.can_access_client("another_client"));
    }

    // ── Wildcard permission edge cases ────────────────────────────────

    #[test]
    fn test_wildcard_single_level() {
        // Wildcard at only one level
        let ctx = create_test_context(vec!["platform:admin:event:*"], "CLIENT", vec![]);
        assert!(ctx.has_permission("platform:admin:event:read"));
        assert!(ctx.has_permission("platform:admin:event:create"));
        assert!(ctx.has_permission("platform:admin:event:delete"));
        // Different aggregate should not match
        assert!(!ctx.has_permission("platform:admin:client:read"));
    }

    #[test]
    fn test_wildcard_context_level() {
        let ctx = create_test_context(vec!["platform:*:event:read"], "CLIENT", vec![]);
        assert!(ctx.has_permission("platform:admin:event:read"));
        assert!(ctx.has_permission("platform:iam:event:read"));
        assert!(!ctx.has_permission("platform:admin:event:create"));
    }

    #[test]
    fn test_non_four_level_permission_no_match() {
        // Permissions with != 4 parts should never match wildcard patterns
        let ctx = create_test_context(vec!["platform:*:*:*"], "ANCHOR", vec!["*"]);
        assert!(!ctx.has_permission("platform:admin"));
        assert!(!ctx.has_permission("platform:admin:event"));
        assert!(!ctx.has_permission("a:b:c:d:e"));
        assert!(!ctx.has_permission(""));
    }

    #[test]
    fn test_no_wildcard_in_permission_itself() {
        // The permission being checked should not use wildcards — only patterns do
        let ctx = create_test_context(vec!["platform:admin:event:read"], "CLIENT", vec![]);
        // Checking a wildcard as a "permission" — should only match the literal string
        assert!(!ctx.has_permission("platform:admin:*:read"));
    }

    // ── Empty roles / permissions ─────────────────────────────────────

    #[test]
    fn test_empty_permissions_denies_all() {
        let ctx = create_test_context(vec![], "CLIENT", vec!["client1"]);
        assert!(!ctx.has_permission("platform:admin:event:read"));
        assert!(!ctx.has_permission("anything"));
    }

    #[test]
    fn test_empty_roles_list() {
        let ctx = AuthContext {
            principal_id: "p1".to_string(),
            principal_type: PrincipalType::User,
            scope: UserScope::Client,
            email: None,
            name: "No Roles".to_string(),
            accessible_clients: vec![],
            permissions: HashSet::new(),
            roles: vec![],
            credential: Credential::BearerToken,
        };
        assert!(!ctx.has_role("admin"));
        assert!(!ctx.has_permission("anything"));
    }

    // ── has_all_permissions / has_any_permission ──────────────────────

    #[test]
    fn test_has_all_permissions_all_present() {
        let ctx = create_test_context(
            vec!["platform:admin:event:read", "platform:admin:client:read"],
            "CLIENT",
            vec![],
        );
        assert!(
            ctx.has_all_permissions(&["platform:admin:event:read", "platform:admin:client:read",])
        );
    }

    #[test]
    fn test_has_all_permissions_one_missing() {
        let ctx = create_test_context(vec!["platform:admin:event:read"], "CLIENT", vec![]);
        assert!(
            !ctx.has_all_permissions(&["platform:admin:event:read", "platform:admin:client:read",])
        );
    }

    #[test]
    fn test_has_all_permissions_empty_required() {
        let ctx = create_test_context(vec![], "CLIENT", vec![]);
        // Empty required set — trivially true
        assert!(ctx.has_all_permissions(&[]));
    }

    #[test]
    fn test_has_any_permission_one_present() {
        let ctx = create_test_context(vec!["platform:admin:event:read"], "CLIENT", vec![]);
        assert!(
            ctx.has_any_permission(&["platform:admin:event:read", "platform:admin:client:read",])
        );
    }

    #[test]
    fn test_has_any_permission_none_present() {
        let ctx = create_test_context(vec![], "CLIENT", vec![]);
        assert!(!ctx.has_any_permission(&["platform:admin:event:read",]));
    }

    #[test]
    fn test_has_any_permission_empty_required() {
        let ctx = create_test_context(vec![], "CLIENT", vec![]);
        // Empty required set — none match
        assert!(!ctx.has_any_permission(&[]));
    }

    // ── has_role ──────────────────────────────────────────────────────

    #[test]
    fn test_has_role_present() {
        let ctx = create_test_context(vec![], "CLIENT", vec![]);
        assert!(ctx.has_role("test:admin"));
    }

    #[test]
    fn test_has_role_absent() {
        let ctx = create_test_context(vec![], "CLIENT", vec![]);
        assert!(!ctx.has_role("nonexistent:role"));
    }

    // ── is_anchor ─────────────────────────────────────────────────────

    #[test]
    fn test_is_anchor_true() {
        let ctx = create_test_context(vec![], "ANCHOR", vec!["*"]);
        assert!(ctx.is_anchor());
    }

    #[test]
    fn test_is_anchor_false_for_client_scope() {
        let ctx = create_test_context(vec![], "CLIENT", vec![]);
        assert!(!ctx.is_anchor());
    }

    #[test]
    fn test_is_anchor_false_for_partner_scope() {
        let ctx = create_test_context(vec![], "PARTNER", vec![]);
        assert!(!ctx.is_anchor());
    }

    // ── Client access edge cases ──────────────────────────────────────

    #[test]
    fn test_no_clients_denies_all() {
        let ctx = create_test_context(vec![], "CLIENT", vec![]);
        assert!(!ctx.can_access_client("anything"));
    }

    #[test]
    fn test_client_access_exact_match_only() {
        let ctx = create_test_context(vec![], "CLIENT", vec!["client1"]);
        assert!(ctx.can_access_client("client1"));
        assert!(!ctx.can_access_client("client10")); // no prefix matching
        assert!(!ctx.can_access_client("client")); // no partial matching
    }

    // ── from_claims_with_permissions ──────────────────────────────────

    #[test]
    fn test_from_claims_preserves_all_fields() {
        let claims = AccessTokenClaims {
            sub: "principal_1".to_string(),
            iss: "https://auth.example.com".to_string(),
            aud: "api".to_string(),
            exp: 1700000000,
            iat: 1699996400,
            nbf: 1699996400,
            jti: "jwt-id-1".to_string(),
            principal_type: PrincipalType::Service,
            tier: Some(UserScope::Anchor),
            scope: None,
            email: Some("svc@test.com".to_string()),
            name: "Service Account".to_string(),
            clients: vec!["*".to_string()],
            roles: vec!["platform:super-admin".to_string()],
            applications: vec!["app1".to_string()],
            all_applications: false,
            azp: None,
            token_use: Some("api".to_string()),
        };
        let mut perms = HashSet::new();
        perms.insert("platform:*:*:*".to_string());

        let ctx = auth_context_from_claims(&claims, perms);
        assert_eq!(ctx.principal_id, "principal_1");
        assert_eq!(ctx.principal_type, PrincipalType::Service);
        assert_eq!(ctx.scope, UserScope::Anchor);
        assert_eq!(ctx.email, Some("svc@test.com".to_string()));
        assert_eq!(ctx.name, "Service Account");
        assert!(ctx.can_access_client("any_client"));
        assert!(ctx.is_anchor());
        assert!(ctx.has_permission("platform:admin:event:read"));
    }

    // ── Authorization checks module ───────────────────────────────────

    #[test]
    fn test_check_require_anchor_passes() {
        let ctx = create_test_context(vec![], "ANCHOR", vec!["*"]);
        assert!(checks::require_anchor(&ctx).is_ok());
    }

    #[test]
    fn test_check_require_anchor_fails() {
        let ctx = create_test_context(vec![], "CLIENT", vec![]);
        assert!(checks::require_anchor(&ctx).is_err());
    }

    /// Owner decision #44: config writes need `…:config:manage`; a custom
    /// role stored while Go ran still holds `…:config:update` and passes.
    #[test]
    fn platform_config_writes_take_manage_or_gos_update() {
        for code in [
            "platform:admin:config:manage",
            "platform:admin:config:update",
        ] {
            let ctx = create_test_context(vec![code], "ANCHOR", vec!["*"]);
            assert!(checks::can_update_platform_config(&ctx).is_ok(), "{code}");
            let ctx = create_test_context(vec![code], "CLIENT", vec![]);
            assert!(checks::can_update_platform_config(&ctx).is_err(), "{code}");
        }
        let ctx = create_test_context(vec!["platform:admin:config:view"], "ANCHOR", vec!["*"]);
        assert!(checks::can_update_platform_config(&ctx).is_err());
    }

    #[test]
    fn test_check_is_admin_with_superuser() {
        let ctx = create_test_context(vec![permissions::ADMIN_ALL], "ANCHOR", vec!["*"]);
        assert!(checks::is_admin(&ctx).is_ok());
    }

    #[test]
    fn test_check_is_admin_anchor_scope_only() {
        // Anchor scope alone is sufficient for is_admin
        let ctx = create_test_context(vec![], "ANCHOR", vec!["*"]);
        assert!(checks::is_admin(&ctx).is_ok());
    }

    #[test]
    fn test_check_is_admin_fails_for_normal_user() {
        let ctx = create_test_context(vec!["platform:admin:event:read"], "CLIENT", vec!["c1"]);
        assert!(checks::is_admin(&ctx).is_err());
    }

    #[test]
    fn test_can_read_events_with_permission() {
        let ctx = create_test_context(vec![permissions::admin::EVENT_READ], "CLIENT", vec!["c1"]);
        assert!(checks::can_read_events(&ctx).is_ok());
    }

    #[test]
    fn test_can_read_events_without_permission() {
        let ctx = create_test_context(vec![], "CLIENT", vec!["c1"]);
        assert!(checks::can_read_events(&ctx).is_err());
    }

    #[test]
    fn test_can_write_events_with_batch_permission() {
        let ctx = create_test_context(
            vec![permissions::admin::BATCH_EVENTS_WRITE],
            "CLIENT",
            vec!["c1"],
        );
        assert!(checks::can_write_events(&ctx).is_ok());
    }

    #[test]
    fn test_can_write_events_with_app_permission() {
        let ctx = create_test_context(
            vec![permissions::application_service::EVENT_CREATE],
            "CLIENT",
            vec!["c1"],
        );
        assert!(checks::can_write_events(&ctx).is_ok());
    }

    #[test]
    fn test_wildcard_permission_satisfies_check() {
        // platform:*:*:* should satisfy any specific permission check
        let ctx = create_test_context(vec!["platform:*:*:*"], "ANCHOR", vec!["*"]);
        assert!(checks::can_read_events(&ctx).is_ok());
        assert!(checks::can_read_event_types(&ctx).is_ok());
        assert!(checks::can_read_subscriptions(&ctx).is_ok());
        assert!(checks::can_read_dispatch_jobs(&ctx).is_ok());
    }

    // ── Application scope ─────────────────────────────────────────────

    fn binding(all_applications: bool, granted: &[&str]) -> Option<PrincipalApplicationBinding> {
        Some(PrincipalApplicationBinding {
            all_applications,
            granted_application_ids: ApplicationId::from_wire_all(granted.iter().copied()),
        })
    }

    #[test]
    fn test_application_scope_from_binding() {
        // all_applications: every application; grants don't narrow it. This
        // holds for an application's own service account too, as in Go.
        let all = ApplicationScope::from_binding(binding(true, &[]));
        assert_eq!(all, ApplicationScope::All);
        assert!(all.allows(&ApplicationId::parse("app_any").unwrap()));
        assert!(ApplicationScope::from_binding(binding(true, &["app_1"]))
            .allows(&ApplicationId::parse("app_2").unwrap()));

        // Without it (a new or provisioned service account): nothing until
        // granted, then only the grants.
        let none = ApplicationScope::from_binding(binding(false, &[]));
        assert_eq!(none, ApplicationScope::Only(HashSet::new()));
        assert!(!none.allows(&ApplicationId::parse("app_any").unwrap()));
        let granted = ApplicationScope::from_binding(binding(false, &["app_1", "app_2"]));
        assert!(granted.allows(&ApplicationId::parse("app_1").unwrap()));
        assert!(granted.allows(&ApplicationId::parse("app_2").unwrap()));
        assert!(!granted.allows(&ApplicationId::parse("app_3").unwrap()));

        // No such principal: nothing.
        assert!(
            !ApplicationScope::from_binding(None).allows(&ApplicationId::parse("app_own").unwrap())
        );
    }

    fn app(id: &str, code: &str, service_account_id: Option<&str>) -> Application {
        let mut app = Application::new(code, code);
        app.id = ApplicationId::parse(id).unwrap();
        app.service_account_id = service_account_id.map(String::from);
        app
    }

    #[test]
    fn test_require_application_access() {
        let own = ApplicationScope::from_binding(binding(false, &["app_a"]));

        let ok = checks::require_application_access(&own, "a", Some(app("app_a", "a", None)));
        assert_eq!(ok.expect("granted application").id.as_str(), "app_a");

        let out_of_scope =
            checks::require_application_access(&own, "b", Some(app("app_b", "b", None)))
                .unwrap_err();
        let missing =
            checks::require_application_access(&own, "b", None::<Application>).unwrap_err();
        assert!(matches!(out_of_scope, PlatformError::NotFound { .. }));
        assert_eq!(format!("{:?}", out_of_scope), format!("{:?}", missing));

        // An all-applications caller still gets 404 for a missing application.
        let missing_all =
            checks::require_application_access(&ApplicationScope::All, "b", None::<Application>)
                .unwrap_err();
        assert_eq!(format!("{:?}", missing_all), format!("{:?}", missing));
    }

    #[test]
    fn test_attached_service_account_gets_no_implicit_pass() {
        // Go has no "the application's own service account passes" rule: the
        // account reaches its application through its access grant.
        let nothing = ApplicationScope::from_binding(binding(false, &[]));
        let attached = app("app_a", "a", Some("sa_principal"));
        assert!(checks::require_application_access(&nothing, "a", Some(attached)).is_err());
    }

    /// Java `Checks.require` / `Checks.requireAnchor`: their own 403 codes.
    #[test]
    fn test_require_permission_and_anchor_scope_codes() {
        let anchor =
            create_test_context(vec!["platform:function:function:view"], "ANCHOR", vec!["*"]);
        assert!(checks::require_permission(&anchor, "platform:function:function:view").is_ok());
        match checks::require_permission(&anchor, "platform:function:function:manage") {
            Err(PlatformError::Coded {
                status,
                code,
                message,
                ..
            }) => {
                assert_eq!(status.as_u16(), 403);
                assert_eq!(code, "PERMISSION_REQUIRED");
                assert_eq!(
                    message,
                    "permission required: platform:function:function:manage"
                );
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(checks::require_anchor_scope(&anchor).is_ok());
        let client = create_test_context(vec![], "CLIENT", vec!["client1"]);
        match checks::require_anchor_scope(&client) {
            Err(PlatformError::Coded { status, code, .. }) => {
                assert_eq!(status.as_u16(), 403);
                assert_eq!(code, "ANCHOR_REQUIRED");
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
