//! Authorization Service
//!
//! Permission-based access control with role resolution.

use crate::application::entity::Application;
use crate::permissions;
use crate::principal::repository::PrincipalApplicationBinding;
use crate::shared::error::{PlatformError, Result};
use crate::AccessTokenClaims;
use crate::RoleRepository;
use crate::{ApplicationRepository, PrincipalRepository};
use crate::{PrincipalType, UserScope};
use dashmap::DashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

/// Authorization context for a request
#[derive(Debug, Clone)]
pub struct AuthContext {
    /// Principal ID
    pub principal_id: String,

    /// Principal type
    pub principal_type: PrincipalType,

    /// User scope
    pub scope: UserScope,

    /// Email (for users)
    pub email: Option<String>,

    /// Display name
    pub name: String,

    /// Client IDs this principal can access
    pub accessible_clients: Vec<String>,

    /// All permissions (resolved from roles)
    pub permissions: HashSet<String>,

    /// Role codes
    pub roles: Vec<String>,
}

impl AuthContext {
    /// Create from JWT claims with resolved permissions
    pub fn from_claims_with_permissions(
        claims: &AccessTokenClaims,
        permissions: HashSet<String>,
    ) -> Self {
        Self {
            principal_id: claims.sub.clone(),
            principal_type: claims.principal_type,
            scope: claims.tier,
            email: claims.email.clone(),
            name: claims.name.clone(),
            accessible_clients: claims.clients.clone(),
            permissions,
            roles: claims.roles.clone(),
        }
    }

    /// Check if this context is for an anchor user
    pub fn is_anchor(&self) -> bool {
        self.scope.is_anchor()
    }

    /// Check if this context can access a specific client
    pub fn can_access_client(&self, client_id: &str) -> bool {
        self.accessible_clients
            .iter()
            .any(|c| c == "*" || c == client_id)
    }

    /// Check if this context has a specific permission (4-level pattern matching)
    pub fn has_permission(&self, permission: &str) -> bool {
        // Direct match
        if self.permissions.contains(permission) {
            return true;
        }

        // 4-level wildcard pattern matching
        for pattern in &self.permissions {
            if crate::role::entity::matches_pattern(permission, pattern) {
                return true;
            }
        }

        false
    }

    /// Check if this context has all specified permissions
    pub fn has_all_permissions(&self, required: &[&str]) -> bool {
        required.iter().all(|p| self.has_permission(p))
    }

    /// Check if this context has any of the specified permissions
    pub fn has_any_permission(&self, required: &[&str]) -> bool {
        required.iter().any(|p| self.has_permission(p))
    }

    /// Check if this context has a specific role
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }
}

/// Go's `errIdentityTokenNotAPICredential` (shared/middleware/middleware.go:24).
pub const IDENTITY_TOKEN_NOT_API_CREDENTIAL: &str = "this access token was issued for interactive login and cannot authorize API requests; obtain an API token via the client_credentials grant";

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
    /// Cache: sorted role codes joined by "," → resolved permissions
    permission_cache: DashMap<String, CachedPermissions>,
}

impl AuthorizationService {
    pub fn new(role_repo: Arc<RoleRepository>) -> Self {
        Self {
            role_repo,
            permission_cache: DashMap::new(),
        }
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
        Ok(AuthContext::from_claims_with_permissions(
            claims,
            permissions,
        ))
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

/// Which applications a principal may act on. Application access is its own
/// axis, orthogonal to the client tier: an ANCHOR service account can still
/// be confined to one application.
///
/// Exactly Go's `CanAccessApplication`
/// (flowcatalyst-go internal/platform/shared/auth/auth.go:259-267): a
/// principal reaches every application when `all_applications` is true,
/// otherwise only the applications it holds an
/// `iam_principal_application_access` grant for. The application a service
/// account was provisioned for is one of those grants; nothing else (the
/// principal's `application_id`, the application's attached service
/// account) widens or narrows the scope. A new service account starts with
/// the flag off and no grants, so it reaches none until granted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplicationScope {
    /// Every application, present and future.
    All,
    /// Only these application ids.
    Only(HashSet<String>),
}

impl ApplicationScope {
    /// Build the scope from a principal's flag and grants. `None` (no such
    /// principal) grants nothing.
    pub fn from_binding(binding: Option<PrincipalApplicationBinding>) -> Self {
        match binding {
            None => Self::Only(HashSet::new()),
            Some(PrincipalApplicationBinding {
                all_applications: true,
                ..
            }) => Self::All,
            Some(PrincipalApplicationBinding {
                granted_application_ids,
                ..
            }) => Self::Only(granted_application_ids.into_iter().collect()),
        }
    }

    pub fn allows(&self, application_id: &str) -> bool {
        match self {
            Self::All => true,
            Self::Only(ids) => ids.contains(application_id),
        }
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

/// Common authorization checks
pub mod checks {
    use super::*;

    /// Require anchor scope
    pub fn require_anchor(context: &AuthContext) -> Result<()> {
        if context.is_anchor() {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Anchor access required"))
        }
    }

    /// Require one permission, answering as Java's `Checks.require`
    /// (shared/auth/Checks.java:53-57): 403 `PERMISSION_REQUIRED`,
    /// `permission required: <code>`. Scope grants no bypass: an anchor
    /// needs the permission too. The function API gates every route with
    /// this.
    pub fn require_permission(context: &AuthContext, permission: &str) -> Result<()> {
        if context.has_permission(permission) {
            Ok(())
        } else {
            Err(PlatformError::Coded {
                status: axum::http::StatusCode::FORBIDDEN,
                code: "PERMISSION_REQUIRED".to_string(),
                message: format!("permission required: {permission}"),
                details: Default::default(),
            })
        }
    }

    /// Require anchor scope, answering as Java's `Checks.requireAnchor`
    /// (Checks.java:74-77): 403 `ANCHOR_REQUIRED`, `anchor scope required`.
    /// [`require_anchor`] is the same check with the platform's older
    /// `FORBIDDEN` body.
    pub fn require_anchor_scope(context: &AuthContext) -> Result<()> {
        if context.is_anchor() {
            Ok(())
        } else {
            Err(PlatformError::Coded {
                status: axum::http::StatusCode::FORBIDDEN,
                code: "ANCHOR_REQUIRED".to_string(),
                message: "anchor scope required".to_string(),
                details: Default::default(),
            })
        }
    }

    /// Go's `anchorWith` (shared/auth/auth.go:704-709): anchor scope, then
    /// one permission, with Go's bodies (403 `ANCHOR_REQUIRED`, then 403
    /// `PERMISSION_REQUIRED`).
    fn anchor_with(context: &AuthContext, permission: &str) -> Result<()> {
        require_anchor_scope(context)?;
        require_permission(context, permission)
    }

    /// Any one of several permissions, answering as Go's `requireAny`
    /// (shared/auth/auth.go:473-481): 403 `PERMISSION_REQUIRED`,
    /// `one of: <a>, <b>`.
    fn require_any_permission(context: &AuthContext, permissions: &[&str]) -> Result<()> {
        if context.has_any_permission(permissions) {
            Ok(())
        } else {
            Err(PlatformError::forbidden_code(
                "PERMISSION_REQUIRED",
                format!("one of: {}", permissions.join(", ")),
            ))
        }
    }

    /// OAuth clients, read (list, get, by client_id): anchor plus
    /// `platform:auth:oauth-client:view` (Go's `CanReadOAuthClients`,
    /// shared/auth/auth.go:732).
    pub fn can_read_oauth_clients(context: &AuthContext) -> Result<()> {
        anchor_with(context, permissions::auth::OAUTH_CLIENT_READ)
    }

    /// OAuth clients, create: anchor plus `platform:auth:oauth-client:create`
    /// (Go's `CanCreateOAuthClients`, auth.go:733).
    pub fn can_create_oauth_clients(context: &AuthContext) -> Result<()> {
        anchor_with(context, permissions::auth::OAUTH_CLIENT_CREATE)
    }

    /// OAuth clients, update, activate, deactivate: anchor plus
    /// `platform:auth:oauth-client:update` (Go's `CanUpdateOAuthClients`,
    /// auth.go:734).
    pub fn can_update_oauth_clients(context: &AuthContext) -> Result<()> {
        anchor_with(context, permissions::auth::OAUTH_CLIENT_UPDATE)
    }

    /// OAuth clients, delete: anchor plus `platform:auth:oauth-client:delete`
    /// (Go's `CanDeleteOAuthClients`, auth.go:735).
    pub fn can_delete_oauth_clients(context: &AuthContext) -> Result<()> {
        anchor_with(context, permissions::auth::OAUTH_CLIENT_DELETE)
    }

    /// OAuth client secrets: rotate, regenerate and revoke-previous all mint
    /// or withdraw a credential, so they share anchor plus
    /// `platform:auth:oauth-client:regenerate-secret` (Go's
    /// `CanRotateOAuthClientSecrets`, auth.go:737-742).
    pub fn can_write_oauth_client_secrets(context: &AuthContext) -> Result<()> {
        anchor_with(context, permissions::auth::OAUTH_CLIENT_REGENERATE_SECRET)
    }

    /// Service accounts, read: `platform:iam:service-account:view`, as Go's
    /// `CanReadServiceAccounts` (shared/auth/auth.go:675-677).
    pub fn can_read_service_accounts(context: &AuthContext) -> Result<()> {
        require_permission(context, permissions::admin::SERVICE_ACCOUNT_READ)
    }

    /// Service accounts, create and update: any of the service-account
    /// create/update/delete permissions, as Go's `CanWriteServiceAccounts`
    /// (auth.go:691-693), and anchor scope on top. Go has no anchor check
    /// here; Rust keeps one because an account's tier follows its client
    /// links, so a non-anchor holder could otherwise create or relink a
    /// client-less account, which is ANCHOR tier.
    pub fn can_write_service_accounts(context: &AuthContext) -> Result<()> {
        require_anchor_scope(context)?;
        require_any_permission(
            context,
            &[
                permissions::admin::SERVICE_ACCOUNT_CREATE,
                permissions::admin::SERVICE_ACCOUNT_UPDATE,
                permissions::admin::SERVICE_ACCOUNT_DELETE,
            ],
        )
    }

    /// Service accounts, what issues authority or a credential to an
    /// existing account: role assignment, auth-token and signing-secret
    /// regeneration. Anchor plus `platform:iam:service-account:update` (Go's
    /// `CanUpdateServiceAccounts`, auth.go:683-685, with decision #19's anchor
    /// requirement; Java 6068fe6b S1.2). Anchor scope alone let an
    /// application's own ANCHOR-tier account grant itself super-admin.
    pub fn can_update_service_accounts(context: &AuthContext) -> Result<()> {
        anchor_with(context, permissions::admin::SERVICE_ACCOUNT_UPDATE)
    }

    /// Service accounts, delete: `platform:iam:service-account:delete`, as
    /// Go's `CanDeleteServiceAccounts` (auth.go:687-689), with the same
    /// anchor requirement as [`can_write_service_accounts`].
    pub fn can_delete_service_accounts(context: &AuthContext) -> Result<()> {
        require_anchor_scope(context)?;
        require_permission(context, permissions::admin::SERVICE_ACCOUNT_DELETE)
    }

    // ── Platform-owner routes: anchor reach plus the permission ─────────
    //
    // Go's `anchorWith(perm)` families (shared/auth/auth.go:704-790). These
    // were anchor-only here, which let any anchor principal (a read-only
    // staff role, a provisioned service account) write them.

    /// Clients, create (Go `CanCreateClients`).
    pub fn can_create_clients(context: &AuthContext) -> Result<()> {
        anchor_with(context, permissions::admin::CLIENT_CREATE)
    }

    /// Clients, update, notes and enabled applications (Go
    /// `CanUpdateClients`; Java ClientApi for the application links, which
    /// Go gates by anchor alone).
    pub fn can_update_clients(context: &AuthContext) -> Result<()> {
        anchor_with(context, permissions::admin::CLIENT_UPDATE)
    }

    /// Clients, delete (Go `CanDeleteClients`).
    pub fn can_delete_clients(context: &AuthContext) -> Result<()> {
        anchor_with(context, permissions::admin::CLIENT_DELETE)
    }

    /// Clients, activate (Go `CanActivateClients`).
    pub fn can_activate_clients(context: &AuthContext) -> Result<()> {
        anchor_with(context, permissions::admin::CLIENT_ACTIVATE)
    }

    /// Clients, suspend (Go `CanSuspendClients`).
    pub fn can_suspend_clients(context: &AuthContext) -> Result<()> {
        anchor_with(context, permissions::admin::CLIENT_SUSPEND)
    }

    /// Clients, deactivate (Go `CanDeactivateClients`).
    pub fn can_deactivate_clients(context: &AuthContext) -> Result<()> {
        anchor_with(context, permissions::admin::CLIENT_DEACTIVATE)
    }

    /// Identity providers, read; also IdP role mappings, read (Go
    /// `CanReadIdentityProviders`).
    pub fn can_read_identity_providers(context: &AuthContext) -> Result<()> {
        anchor_with(context, permissions::admin::IDENTITY_PROVIDER_READ)
    }

    /// Identity providers, create (Go `CanCreateIdentityProviders`).
    pub fn can_create_identity_providers(context: &AuthContext) -> Result<()> {
        anchor_with(context, permissions::admin::IDENTITY_PROVIDER_CREATE)
    }

    /// Identity providers, update; also IdP role mappings, create and
    /// delete (Go `CanUpdateIdentityProviders`).
    pub fn can_update_identity_providers(context: &AuthContext) -> Result<()> {
        anchor_with(context, permissions::admin::IDENTITY_PROVIDER_UPDATE)
    }

    /// Identity providers, delete (Go `CanDeleteIdentityProviders`).
    pub fn can_delete_identity_providers(context: &AuthContext) -> Result<()> {
        anchor_with(context, permissions::admin::IDENTITY_PROVIDER_DELETE)
    }

    /// Email-domain mappings, create (Go `CanCreateEmailDomainMappings`).
    pub fn can_create_email_domain_mappings(context: &AuthContext) -> Result<()> {
        anchor_with(context, permissions::admin::EMAIL_DOMAIN_MAPPING_CREATE)
    }

    /// Email-domain mappings, update (Go `CanUpdateEmailDomainMappings`).
    pub fn can_update_email_domain_mappings(context: &AuthContext) -> Result<()> {
        anchor_with(context, permissions::admin::EMAIL_DOMAIN_MAPPING_UPDATE)
    }

    /// Email-domain mappings, delete (Go `CanDeleteEmailDomainMappings`).
    pub fn can_delete_email_domain_mappings(context: &AuthContext) -> Result<()> {
        anchor_with(context, permissions::admin::EMAIL_DOMAIN_MAPPING_DELETE)
    }

    /// Anchor domains, read (Go `CanReadAnchorDomains`).
    pub fn can_read_anchor_domains(context: &AuthContext) -> Result<()> {
        anchor_with(context, permissions::admin::ANCHOR_DOMAIN_READ)
    }

    /// Anchor domains, create (Go `CanCreateAnchorDomains`).
    pub fn can_create_anchor_domains(context: &AuthContext) -> Result<()> {
        anchor_with(context, permissions::admin::ANCHOR_DOMAIN_CREATE)
    }

    /// Anchor domains, update (Go `CanUpdateAnchorDomains`).
    pub fn can_update_anchor_domains(context: &AuthContext) -> Result<()> {
        anchor_with(context, permissions::admin::ANCHOR_DOMAIN_UPDATE)
    }

    /// Anchor domains, delete (Go `CanDeleteAnchorDomains`).
    pub fn can_delete_anchor_domains(context: &AuthContext) -> Result<()> {
        anchor_with(context, permissions::admin::ANCHOR_DOMAIN_DELETE)
    }

    /// Client auth configs, read (Go `CanReadAuthConfigs`).
    pub fn can_read_auth_configs(context: &AuthContext) -> Result<()> {
        anchor_with(context, permissions::auth::CLIENT_AUTH_CONFIG_READ)
    }

    /// Client auth configs, create (Go `CanCreateAuthConfigs`).
    pub fn can_create_auth_configs(context: &AuthContext) -> Result<()> {
        anchor_with(context, permissions::auth::CLIENT_AUTH_CONFIG_CREATE)
    }

    /// Client auth configs, update (Go `CanUpdateAuthConfigs`).
    pub fn can_update_auth_configs(context: &AuthContext) -> Result<()> {
        anchor_with(context, permissions::auth::CLIENT_AUTH_CONFIG_UPDATE)
    }

    /// Client auth configs, delete (Go `CanDeleteAuthConfigs`).
    pub fn can_delete_auth_configs(context: &AuthContext) -> Result<()> {
        anchor_with(context, permissions::auth::CLIENT_AUTH_CONFIG_DELETE)
    }

    /// CORS origins, create (Go `CanCreateCorsOrigins`).
    pub fn can_create_cors_origins(context: &AuthContext) -> Result<()> {
        anchor_with(context, permissions::admin::CORS_ORIGIN_CREATE)
    }

    /// CORS origins, delete (Go `CanDeleteCorsOrigins`).
    pub fn can_delete_cors_origins(context: &AuthContext) -> Result<()> {
        anchor_with(context, permissions::admin::CORS_ORIGIN_DELETE)
    }

    /// Applications, create, update, activate, deactivate: any of the
    /// application create/update/delete permissions (Go's
    /// `CanWriteApplications`). The handlers keep their anchor check on top.
    pub fn can_write_applications(context: &AuthContext) -> Result<()> {
        require_any_permission(
            context,
            &[
                permissions::admin::APPLICATION_CREATE,
                permissions::admin::APPLICATION_UPDATE,
                permissions::admin::APPLICATION_DELETE,
            ],
        )
    }

    /// Applications, delete (Go's `CanDeleteApplications`).
    pub fn can_delete_applications(context: &AuthContext) -> Result<()> {
        require_permission(context, permissions::admin::APPLICATION_DELETE)
    }

    /// Role administration through `/api/roles` and `/bff/roles`: anchor
    /// reach and the role permission (owner decision #25, stricter than Go,
    /// which asks the permission only).
    pub fn can_administer_roles(context: &AuthContext, permission: &str) -> Result<()> {
        anchor_with(context, permission)
    }

    /// Re-running the built-in role sync: anchor and any role write
    /// permission (Java RolesBff sync-platform).
    pub fn can_sync_platform_roles(context: &AuthContext) -> Result<()> {
        require_anchor_scope(context)?;
        require_any_permission(
            context,
            &[
                permissions::iam::ROLE_CREATE,
                permissions::iam::ROLE_UPDATE,
                permissions::iam::ROLE_DELETE,
            ],
        )
    }

    /// Client-access grant and revoke: anchor reach and
    /// `platform:iam:client-access:grant` / `:revoke` (owner decision #25;
    /// Go asks anchor alone).
    pub fn can_grant_client_access(context: &AuthContext) -> Result<()> {
        anchor_with(context, permissions::iam::CLIENT_ACCESS_GRANT)
    }

    /// See [`can_grant_client_access`].
    pub fn can_revoke_client_access(context: &AuthContext) -> Result<()> {
        anchor_with(context, permissions::iam::CLIENT_ACCESS_REVOKE)
    }

    /// Principals, the user-administration writes (create, update,
    /// activate, deactivate, password reset, application access): any of the
    /// user create/update/delete permissions, as Go's `CanWritePrincipals`
    /// (shared/auth/auth.go, reached through `RequireUserAdmin`). Scope is
    /// reach, never authority: an anchor needs the permission too. The
    /// handler keeps its own tier check on top.
    pub fn can_write_principals(context: &AuthContext) -> Result<()> {
        require_any_permission(
            context,
            &[
                permissions::iam::USER_CREATE,
                permissions::iam::USER_UPDATE,
                permissions::iam::USER_DELETE,
            ],
        )
    }

    /// Principals, delete: `platform:iam:user:delete`, as Go's
    /// `CanDeletePrincipals`.
    pub fn can_delete_principals(context: &AuthContext) -> Result<()> {
        require_permission(context, permissions::iam::USER_DELETE)
    }

    /// Platform-config access grants, read: anchor plus
    /// `platform:admin:config:view` (Go's `CanReadPlatformConfig`,
    /// shared/auth/auth.go:784).
    pub fn can_read_platform_config(context: &AuthContext) -> Result<()> {
        require_anchor(context)?;
        if context.has_permission(permissions::admin::CONFIG_READ) {
            Ok(())
        } else {
            Err(PlatformError::forbidden(
                "Cannot read platform config access",
            ))
        }
    }

    /// Platform-config access grants, write: anchor plus
    /// `platform:admin:config:update` (Go's `CanUpdatePlatformConfig`,
    /// shared/auth/auth.go:785).
    pub fn can_update_platform_config(context: &AuthContext) -> Result<()> {
        require_anchor(context)?;
        if context.has_permission(permissions::admin::CONFIG_UPDATE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden(
                "Cannot update platform config access",
            ))
        }
    }

    /// Developer portal: read an application's OpenAPI document.
    /// Resource scoping (which application the principal can see) is handled
    /// in the handler against `iam_principal_application_access`.
    pub fn can_read_application_openapi(context: &AuthContext) -> Result<()> {
        if context.has_any_permission(&[
            permissions::developer::APPLICATION_OPENAPI_VIEW,
            permissions::developer::APPLICATION_OPENAPI_MANAGE,
        ]) {
            Ok(())
        } else {
            Err(PlatformError::forbidden(
                "Cannot read application OpenAPI specs",
            ))
        }
    }

    /// SDK ingest: sync an application's OpenAPI document.
    /// Service-account-belongs-to-application is enforced in the handler.
    pub fn can_sync_application_openapi(context: &AuthContext) -> Result<()> {
        if context.has_any_permission(&[
            permissions::developer::APPLICATION_OPENAPI_SYNC,
            permissions::developer::APPLICATION_OPENAPI_MANAGE,
        ]) {
            Ok(())
        } else {
            Err(PlatformError::forbidden(
                "Cannot sync application OpenAPI specs",
            ))
        }
    }

    /// Check read access to events
    pub fn can_read_events(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::admin::EVENT_READ) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot read events"))
        }
    }

    /// Check read access to audit logs
    pub fn can_read_audit_logs(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::admin::AUDIT_LOG_READ) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot read audit logs"))
        }
    }

    /// Check raw read access to events (includes payload)
    pub fn can_read_events_raw(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::admin::EVENT_VIEW_RAW) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot read raw event data"))
        }
    }

    /// Check read access to event types
    pub fn can_read_event_types(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::admin::EVENT_TYPE_READ) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot read event types"))
        }
    }

    /// Check create access to event types
    pub fn can_create_event_types(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::admin::EVENT_TYPE_CREATE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot create event types"))
        }
    }

    /// Check update access to event types
    pub fn can_update_event_types(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::admin::EVENT_TYPE_UPDATE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot update event types"))
        }
    }

    /// Check delete access to event types
    pub fn can_delete_event_types(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::admin::EVENT_TYPE_DELETE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot delete event types"))
        }
    }

    /// Check read access to subscriptions
    pub fn can_read_subscriptions(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::admin::SUBSCRIPTION_READ) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot read subscriptions"))
        }
    }

    /// Check create access to subscriptions
    pub fn can_create_subscriptions(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::admin::SUBSCRIPTION_CREATE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot create subscriptions"))
        }
    }

    /// Check update access to subscriptions
    pub fn can_update_subscriptions(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::admin::SUBSCRIPTION_UPDATE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot update subscriptions"))
        }
    }

    /// Check delete access to subscriptions
    pub fn can_delete_subscriptions(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::admin::SUBSCRIPTION_DELETE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot delete subscriptions"))
        }
    }

    /// Check read access to dispatch jobs
    pub fn can_read_dispatch_jobs(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::admin::DISPATCH_JOB_READ) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot read dispatch jobs"))
        }
    }

    /// Check raw read access to dispatch jobs (includes payload)
    pub fn can_read_dispatch_jobs_raw(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::admin::DISPATCH_JOB_VIEW_RAW) {
            Ok(())
        } else {
            Err(PlatformError::forbidden(
                "Cannot read raw dispatch job data",
            ))
        }
    }

    /// Check admin access (any admin permission)
    pub fn is_admin(context: &AuthContext) -> Result<()> {
        if context.is_anchor() || context.has_permission(permissions::ADMIN_ALL) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Admin access required"))
        }
    }

    /// Check write access to events (create)
    pub fn can_write_events(context: &AuthContext) -> Result<()> {
        if context.has_any_permission(&[
            permissions::admin::BATCH_EVENTS_WRITE,
            permissions::application_service::EVENT_CREATE,
        ]) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot write events"))
        }
    }

    /// Check write access to event types (create, update, or delete)
    pub fn can_write_event_types(context: &AuthContext) -> Result<()> {
        if context.has_any_permission(&[
            permissions::admin::EVENT_TYPE_CREATE,
            permissions::admin::EVENT_TYPE_UPDATE,
            permissions::admin::EVENT_TYPE_DELETE,
        ]) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot write event types"))
        }
    }

    // ── Process documentation ────────────────────────────────────────────

    /// Check read access to processes
    pub fn can_read_processes(context: &AuthContext) -> Result<()> {
        if context.has_any_permission(&[
            permissions::admin::PROCESS_READ,
            permissions::application_service::PROCESS_READ,
        ]) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot read processes"))
        }
    }

    /// Check create access to processes
    pub fn can_create_processes(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::admin::PROCESS_CREATE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot create processes"))
        }
    }

    /// Check update access to processes
    pub fn can_update_processes(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::admin::PROCESS_UPDATE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot update processes"))
        }
    }

    /// Check delete access to processes
    pub fn can_delete_processes(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::admin::PROCESS_DELETE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot delete processes"))
        }
    }

    /// Check write access to processes (create, update, archive, or delete)
    pub fn can_write_processes(context: &AuthContext) -> Result<()> {
        if context.has_any_permission(&[
            permissions::admin::PROCESS_CREATE,
            permissions::admin::PROCESS_UPDATE,
            permissions::admin::PROCESS_DELETE,
            permissions::admin::PROCESS_ARCHIVE,
        ]) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot write processes"))
        }
    }

    /// Check sync access to processes (SDK push from an application)
    pub fn can_sync_processes(context: &AuthContext) -> Result<()> {
        if context.has_any_permission(&[
            permissions::admin::PROCESS_SYNC,
            permissions::application_service::PROCESS_SYNC,
        ]) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot sync processes"))
        }
    }

    /// Check write access to subscriptions (create, update, or delete)
    pub fn can_write_subscriptions(context: &AuthContext) -> Result<()> {
        if context.has_any_permission(&[
            permissions::admin::SUBSCRIPTION_CREATE,
            permissions::admin::SUBSCRIPTION_UPDATE,
            permissions::admin::SUBSCRIPTION_DELETE,
        ]) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot write subscriptions"))
        }
    }

    /// Check create access to dispatch jobs
    pub fn can_create_dispatch_jobs(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::admin::BATCH_DISPATCH_JOBS_WRITE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot create dispatch jobs"))
        }
    }

    /// Check retry access to dispatch jobs
    pub fn can_retry_dispatch_jobs(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::admin::BATCH_DISPATCH_JOBS_WRITE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot retry dispatch jobs"))
        }
    }

    /// Check write access to dispatch jobs (batch)
    pub fn can_write_dispatch_jobs(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::admin::BATCH_DISPATCH_JOBS_WRITE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot write dispatch jobs"))
        }
    }

    // ── Scheduled jobs ──────────────────────────────────────────────────────

    pub fn can_read_scheduled_jobs(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::admin::SCHEDULED_JOB_READ) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot read scheduled jobs"))
        }
    }

    pub fn can_create_scheduled_jobs(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::admin::SCHEDULED_JOB_CREATE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot create scheduled jobs"))
        }
    }

    pub fn can_update_scheduled_jobs(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::admin::SCHEDULED_JOB_UPDATE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot update scheduled jobs"))
        }
    }

    pub fn can_delete_scheduled_jobs(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::admin::SCHEDULED_JOB_DELETE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot delete scheduled jobs"))
        }
    }

    pub fn can_pause_scheduled_jobs(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::admin::SCHEDULED_JOB_PAUSE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot pause scheduled jobs"))
        }
    }

    pub fn can_fire_scheduled_jobs(context: &AuthContext) -> Result<()> {
        if context.has_permission(permissions::admin::SCHEDULED_JOB_FIRE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot fire scheduled jobs"))
        }
    }

    /// Umbrella check: any write permission on scheduled jobs.
    pub fn can_write_scheduled_jobs(context: &AuthContext) -> Result<()> {
        if context.has_any_permission(&[
            permissions::admin::SCHEDULED_JOB_CREATE,
            permissions::admin::SCHEDULED_JOB_UPDATE,
            permissions::admin::SCHEDULED_JOB_DELETE,
            permissions::admin::SCHEDULED_JOB_PAUSE,
            permissions::admin::SCHEDULED_JOB_FIRE,
            permissions::admin::SCHEDULED_JOB_MANAGE,
            permissions::admin::SCHEDULED_JOB_SYNC,
        ]) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot write scheduled jobs"))
        }
    }

    /// Sync endpoints: admin path. Application-scoped sync uses the
    /// application_service permission below.
    pub fn can_sync_scheduled_jobs(context: &AuthContext) -> Result<()> {
        if context.has_any_permission(&[
            permissions::admin::SCHEDULED_JOB_SYNC,
            permissions::admin::SCHEDULED_JOB_MANAGE,
        ]) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot sync scheduled jobs"))
        }
    }

    pub fn can_read_scheduled_job_instances(context: &AuthContext) -> Result<()> {
        if context.has_any_permission(&[
            permissions::admin::SCHEDULED_JOB_INSTANCE_READ,
            permissions::admin::SCHEDULED_JOB_READ,
        ]) {
            Ok(())
        } else {
            Err(PlatformError::forbidden(
                "Cannot read scheduled job instances",
            ))
        }
    }

    /// SDK callback path — log/complete an instance the platform fired.
    /// Granted to application service accounts via
    /// `application_service::SCHEDULED_JOB_INSTANCE_WRITE`. Anchor /
    /// `ADMIN_ALL` also work.
    pub fn can_write_scheduled_job_instance(context: &AuthContext) -> Result<()> {
        if context.has_any_permission(&[
            permissions::application_service::SCHEDULED_JOB_INSTANCE_WRITE,
            permissions::admin::SCHEDULED_JOB_MANAGE,
        ]) {
            Ok(())
        } else {
            Err(PlatformError::forbidden(
                "Cannot write to scheduled job instance",
            ))
        }
    }

    /// SDK-driven sync of scheduled-job definitions for an application.
    pub fn can_sync_scheduled_jobs_app(context: &AuthContext) -> Result<()> {
        if context.has_any_permission(&[
            permissions::application_service::SCHEDULED_JOB_SYNC,
            permissions::admin::SCHEDULED_JOB_SYNC,
            permissions::admin::SCHEDULED_JOB_MANAGE,
        ]) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot sync scheduled jobs"))
        }
    }

    // ── App-scoped SDK sync checks ─────────────────────────────────────────
    //
    // These guard the `/api/applications/{app_code}/{resource}/sync` handlers.
    // They admit both admin-tier callers (messaging-admin, ADMIN_ALL) and
    // application-service service accounts where an app-service permission
    // exists for the resource. Per-application scope is verified inside the
    // use case (the app_code in the URL is the partition key).

    pub fn can_sync_event_types(context: &AuthContext) -> Result<()> {
        if context.has_any_permission(&[
            permissions::admin::EVENT_TYPE_SYNC,
            permissions::admin::EVENT_TYPE_MANAGE,
            permissions::application_service::EVENT_TYPE_CREATE,
            permissions::application_service::EVENT_TYPE_UPDATE,
            permissions::application_service::EVENT_TYPE_DELETE,
        ]) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot sync event types"))
        }
    }

    pub fn can_sync_subscriptions(context: &AuthContext) -> Result<()> {
        if context.has_any_permission(&[
            permissions::admin::SUBSCRIPTION_SYNC,
            permissions::admin::SUBSCRIPTION_MANAGE,
            permissions::application_service::SUBSCRIPTION_CREATE,
            permissions::application_service::SUBSCRIPTION_UPDATE,
            permissions::application_service::SUBSCRIPTION_DELETE,
        ]) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot sync subscriptions"))
        }
    }

    /// Read roles through the application-scoped SDK surface.
    pub fn can_read_roles(context: &AuthContext) -> Result<()> {
        if context.has_any_permission(&[
            permissions::iam::ROLE_MANAGE,
            permissions::iam::ROLE_READ,
            permissions::application_service::ROLE_READ,
        ]) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot read roles"))
        }
    }

    /// Create a single role through the application-scoped SDK surface.
    pub fn can_create_roles(context: &AuthContext) -> Result<()> {
        if context.has_any_permission(&[
            permissions::iam::ROLE_MANAGE,
            permissions::iam::ROLE_CREATE,
            permissions::application_service::ROLE_CREATE,
        ]) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot create roles"))
        }
    }

    /// Delete a single role through the application-scoped SDK surface.
    pub fn can_delete_roles(context: &AuthContext) -> Result<()> {
        if context.has_any_permission(&[
            permissions::iam::ROLE_MANAGE,
            permissions::iam::ROLE_DELETE,
            permissions::application_service::ROLE_DELETE,
        ]) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot delete roles"))
        }
    }

    pub fn can_sync_roles(context: &AuthContext) -> Result<()> {
        if context.has_any_permission(&[
            permissions::iam::ROLE_MANAGE,
            permissions::iam::ROLE_CREATE,
            permissions::iam::ROLE_UPDATE,
            permissions::iam::ROLE_DELETE,
            permissions::application_service::ROLE_CREATE,
            permissions::application_service::ROLE_UPDATE,
            permissions::application_service::ROLE_DELETE,
        ]) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot sync roles"))
        }
    }

    /// Dispatch-pool sync is admin-tier only — no application-service
    /// permission exists for dispatch pools today.
    pub fn can_sync_dispatch_pools(context: &AuthContext) -> Result<()> {
        if context.has_any_permission(&[
            permissions::admin::DISPATCH_POOL_SYNC,
            permissions::admin::DISPATCH_POOL_MANAGE,
        ]) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot sync dispatch pools"))
        }
    }

    /// Principal sync is admin-tier only — no application-service
    /// permission exists for users today.
    pub fn can_sync_principals(context: &AuthContext) -> Result<()> {
        if context.has_any_permission(&[
            permissions::iam::USER_MANAGE,
            permissions::iam::USER_CREATE,
            permissions::iam::USER_UPDATE,
            permissions::iam::USER_DELETE,
            permissions::iam::USER_ASSIGN_ROLES,
        ]) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot sync principals"))
        }
    }

    /// Resource scope for `/{appCode}` routes. `application` is what
    /// `app_code` resolved to (`None` if nothing). The caller may act on it
    /// when its scope covers it. Otherwise the answer is the same 404 as a
    /// missing application (owner ruling; Go answers 403), so the route
    /// can't be used to probe which codes exist.
    pub fn require_application_access(
        scope: &ApplicationScope,
        app_code: &str,
        application: Option<Application>,
    ) -> Result<Application> {
        match application {
            Some(app) if scope.allows(&app.id) => Ok(app),
            _ => Err(PlatformError::not_found("Application", app_code)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        }
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
            tier: UserScope::Anchor,
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

        let ctx = AuthContext::from_claims_with_permissions(&claims, perms);
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
            granted_application_ids: granted.iter().map(|s| s.to_string()).collect(),
        })
    }

    #[test]
    fn test_application_scope_from_binding() {
        // all_applications: every application; grants don't narrow it. This
        // holds for an application's own service account too, as in Go.
        let all = ApplicationScope::from_binding(binding(true, &[]));
        assert_eq!(all, ApplicationScope::All);
        assert!(all.allows("app_any"));
        assert!(ApplicationScope::from_binding(binding(true, &["app_1"])).allows("app_2"));

        // Without it (a new or provisioned service account): nothing until
        // granted, then only the grants.
        let none = ApplicationScope::from_binding(binding(false, &[]));
        assert_eq!(none, ApplicationScope::Only(HashSet::new()));
        assert!(!none.allows("app_any"));
        let granted = ApplicationScope::from_binding(binding(false, &["app_1", "app_2"]));
        assert!(granted.allows("app_1"));
        assert!(granted.allows("app_2"));
        assert!(!granted.allows("app_3"));

        // No such principal: nothing.
        assert!(!ApplicationScope::from_binding(None).allows("app_own"));
    }

    fn app(id: &str, code: &str, service_account_id: Option<&str>) -> Application {
        let mut app = Application::new(code, code);
        app.id = id.to_string();
        app.service_account_id = service_account_id.map(String::from);
        app
    }

    #[test]
    fn test_require_application_access() {
        let own = ApplicationScope::from_binding(binding(false, &["app_a"]));

        let ok = checks::require_application_access(&own, "a", Some(app("app_a", "a", None)));
        assert_eq!(ok.expect("granted application").id, "app_a");

        let out_of_scope =
            checks::require_application_access(&own, "b", Some(app("app_b", "b", None)))
                .unwrap_err();
        let missing = checks::require_application_access(&own, "b", None).unwrap_err();
        assert!(matches!(out_of_scope, PlatformError::NotFound { .. }));
        assert_eq!(format!("{:?}", out_of_scope), format!("{:?}", missing));

        // An all-applications caller still gets 404 for a missing application.
        let missing_all =
            checks::require_application_access(&ApplicationScope::All, "b", None).unwrap_err();
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
