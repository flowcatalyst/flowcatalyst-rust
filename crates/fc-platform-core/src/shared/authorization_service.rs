//! The authorization context and the rules that read it.
//!
//! [`AuthContext`] (who a request authenticated as), [`Authority`] (what a
//! rule needs of a caller), [`ApplicationScope`] and the [`checks`]. The
//! repository-backed services that build a context (`AuthorizationService`,
//! `ApplicationAccessService`) are fc-platform-iam's.

use crate::permissions;
use crate::principal_kind::{PrincipalType, UserScope};
use crate::shared::error::{PlatformError, Result};
use crate::shared::id::ApplicationId;
use crate::shared::id::ClientId;
use crate::shared::id::PrincipalId;
use std::collections::HashSet;
use std::result;

/// Authorization context for a request
#[derive(Debug, Clone)]
pub struct AuthContext {
    /// Principal ID
    pub principal_id: PrincipalId,

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

    /// What the caller authenticated with. Only the session-cookie path of
    /// the authenticator stamps [`Credential::SessionCookie`]; every other
    /// construction is a bearer, so a check for a signed-in browser fails
    /// closed.
    pub credential: Credential,
}

/// The credential a request authenticated with (Java
/// `AuthContext.Credential`, 6a06a7f0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Credential {
    /// An `Authorization: Bearer` access token: self-contained claims,
    /// possibly narrowed to a scope or delegated to an OAuth client.
    BearerToken,
    /// The platform session cookie: a signed-in user, reloaded from the
    /// database on this request.
    SessionCookie,
}

impl AuthContext {
    /// Whether the caller is a browser session (the platform session
    /// cookie), as opposed to a bearer token.
    pub fn via_session_cookie(&self) -> bool {
        self.credential == Credential::SessionCookie
    }

    /// Require a signed-in user: the session cookie of an active USER
    /// principal (the cookie path only authenticates an active one). A
    /// bearer is refused like no credential at all (401), even when it
    /// belongs to that user: it may be narrowed or delegated to an OAuth
    /// client, so it never stands in for the user's own session (Java
    /// S2.1, S2.4).
    pub fn require_session_user(&self) -> result::Result<(), PlatformError> {
        if self.via_session_cookie() && self.principal_type == PrincipalType::User {
            Ok(())
        } else {
            Err(PlatformError::Unauthorized {
                message: "A signed-in browser session is required".to_string(),
            })
        }
    }

    /// Check if this context is for an anchor user
    pub fn is_anchor(&self) -> bool {
        self.scope.is_anchor()
    }

    /// Check if this context can access a specific client
    pub fn can_access_client(&self, client_id: &ClientId) -> bool {
        self.accessible_clients
            .iter()
            .any(|c| c == "*" || c == client_id.as_str())
    }

    /// Check if this context has a specific permission (4-level pattern matching)
    pub fn has_permission(&self, permission: &str) -> bool {
        // Direct match
        if self.permissions.contains(permission) {
            return true;
        }

        // 4-level wildcard pattern matching
        for pattern in &self.permissions {
            if permissions::matches_pattern(permission, pattern) {
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

/// What an authorization rule needs to know about whoever is acting. The
/// [`checks`] and [`caller_reach`](crate::shared::caller_reach) rules take
/// any `Authority`, so one rule text serves a handler (an [`AuthContext`])
/// and a use case's `authorize` (a [`Caller`](crate::usecase::Caller),
/// which may also be the system).
pub trait Authority {
    /// Anchor tier (the system caller counts as anchor).
    fn is_anchor(&self) -> bool;
    /// Holds client `client_id`, or `*`.
    fn can_access_client(&self, client_id: &ClientId) -> bool;
    /// Holds `permission`, directly or by a wildcard pattern.
    fn has_permission(&self, permission: &str) -> bool;
    /// Holds any one of `permissions`.
    fn has_any_permission(&self, permissions: &[&str]) -> bool {
        permissions.iter().any(|p| self.has_permission(p))
    }
    /// The client ids held, `*` for every client.
    fn accessible_clients(&self) -> &[String];
}

impl Authority for AuthContext {
    fn is_anchor(&self) -> bool {
        AuthContext::is_anchor(self)
    }
    fn can_access_client(&self, client_id: &ClientId) -> bool {
        AuthContext::can_access_client(self, client_id)
    }
    fn has_permission(&self, permission: &str) -> bool {
        AuthContext::has_permission(self, permission)
    }
    fn has_any_permission(&self, permissions: &[&str]) -> bool {
        AuthContext::has_any_permission(self, permissions)
    }
    fn accessible_clients(&self) -> &[String] {
        &self.accessible_clients
    }
}

/// Go's `errIdentityTokenNotAPICredential` (shared/middleware/middleware.go:24).
pub const IDENTITY_TOKEN_NOT_API_CREDENTIAL: &str = "this access token was issued for interactive login and cannot authorize API requests; obtain an API token via the client_credentials grant";

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
/// The application-access facts for one principal, without hydrating the
/// rest of it: its `all_applications` flag and its explicit
/// `iam_principal_application_access` grants.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PrincipalApplicationBinding {
    pub all_applications: bool,
    pub granted_application_ids: Vec<ApplicationId>,
}

/// What an application-scope check needs of an application: its id. The
/// `Application` aggregate (fc-platform-iam) implements it.
pub trait ScopedApplication {
    fn application_id(&self) -> &ApplicationId;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplicationScope {
    /// Every application, present and future.
    All,
    /// Only these application ids.
    Only(HashSet<ApplicationId>),
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

    pub fn allows(&self, application_id: &ApplicationId) -> bool {
        match self {
            Self::All => true,
            Self::Only(ids) => ids.contains(application_id),
        }
    }
}

/// Common authorization checks
pub mod checks {
    use super::*;
    use crate::shared::caller_reach;
    use crate::usecase::Caller;
    use axum::http::StatusCode;

    /// Require anchor scope
    pub fn require_anchor(context: &impl Authority) -> Result<()> {
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
    pub fn require_permission(context: &impl Authority, permission: &str) -> Result<()> {
        if context.has_permission(permission) {
            Ok(())
        } else {
            Err(PlatformError::Coded {
                status: StatusCode::FORBIDDEN,
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
    pub fn require_anchor_scope(context: &impl Authority) -> Result<()> {
        if context.is_anchor() {
            Ok(())
        } else {
            Err(PlatformError::Coded {
                status: StatusCode::FORBIDDEN,
                code: "ANCHOR_REQUIRED".to_string(),
                message: "anchor scope required".to_string(),
                details: Default::default(),
            })
        }
    }

    /// Go's `anchorWith` (shared/auth/auth.go:704-709): anchor scope, then
    /// one permission, with Go's bodies (403 `ANCHOR_REQUIRED`, then 403
    /// `PERMISSION_REQUIRED`).
    fn anchor_with(context: &impl Authority, permission: &str) -> Result<()> {
        require_anchor_scope(context)?;
        require_permission(context, permission)
    }

    /// Any one of several permissions, answering as Go's `requireAny`
    /// (shared/auth/auth.go:473-481): 403 `PERMISSION_REQUIRED`,
    /// `one of: <a>, <b>`.
    fn require_any_permission(context: &impl Authority, permissions: &[&str]) -> Result<()> {
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
    pub fn can_read_oauth_clients(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::auth::OAUTH_CLIENT_READ)
    }

    /// OAuth clients, create: anchor plus `platform:auth:oauth-client:create`
    /// (Go's `CanCreateOAuthClients`, auth.go:733).
    pub fn can_create_oauth_clients(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::auth::OAUTH_CLIENT_CREATE)
    }

    /// OAuth clients, update, activate, deactivate: anchor plus
    /// `platform:auth:oauth-client:update` (Go's `CanUpdateOAuthClients`,
    /// auth.go:734).
    pub fn can_update_oauth_clients(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::auth::OAUTH_CLIENT_UPDATE)
    }

    /// OAuth clients, delete: anchor plus `platform:auth:oauth-client:delete`
    /// (Go's `CanDeleteOAuthClients`, auth.go:735).
    pub fn can_delete_oauth_clients(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::auth::OAUTH_CLIENT_DELETE)
    }

    /// OAuth client secrets: rotate, regenerate and revoke-previous all mint
    /// or withdraw a credential, so they share anchor plus
    /// `platform:auth:oauth-client:regenerate-secret` (Go's
    /// `CanRotateOAuthClientSecrets`, auth.go:737-742).
    pub fn can_write_oauth_client_secrets(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::auth::OAUTH_CLIENT_REGENERATE_SECRET)
    }

    /// Service accounts, read: `platform:iam:service-account:view`, as Go's
    /// `CanReadServiceAccounts` (shared/auth/auth.go:675-677).
    pub fn can_read_service_accounts(context: &impl Authority) -> Result<()> {
        require_permission(context, permissions::admin::SERVICE_ACCOUNT_READ)
    }

    // ── Read gates: Go's `CanRead*` (shared/auth/auth.go) ──────────────
    //
    // Each answers as Go does: 403 `PERMISSION_REQUIRED`, and for the
    // `anchorWith` families 403 `ANCHOR_REQUIRED` first.

    /// Applications, read (list, get, by code, client configs, roles):
    /// `platform:admin:application:view` (Go `CanReadApplications`,
    /// auth.go:575).
    pub fn can_read_applications(context: &impl Authority) -> Result<()> {
        require_permission(context, permissions::admin::APPLICATION_READ)
    }

    /// The coarse guard on the six `/api/applications` read endpoints (list,
    /// get, by code, client configs, one client config, roles): the admin
    /// view permission, or the application-service view an SDK service
    /// account holds (Go `CanReadApplications`). A caller admitted only by
    /// the second is confined by [`can_read_application`] (or, for the
    /// list, by a filter) to the applications it is bound to. The BFF and
    /// the server-rendered UI keep [`can_read_applications`].
    pub fn can_read_applications_or_own(context: &impl Authority) -> Result<()> {
        require_any_permission(
            context,
            &[
                permissions::admin::APPLICATION_READ,
                permissions::application_service::APPLICATION_READ,
            ],
        )
    }

    /// Whether the caller reads applications without the per-application
    /// confinement: it holds the admin view permission, directly or by a
    /// wildcard (Go `CanReadAllApplications`). The list endpoint filters
    /// when this is false.
    pub fn can_read_all_applications(context: &impl Authority) -> bool {
        context.has_permission(permissions::admin::APPLICATION_READ)
    }

    /// The resource-level read rule for one application (Go
    /// `CanReadApplication`): the coarse guard, then the admin view
    /// permission reads any application, while a holder of only the
    /// application-service view reads the applications `scope` covers (the
    /// all-applications flag, or the id among its grants) and no others.
    /// Refused as Go: 403 `APPLICATION_ACCESS_REQUIRED`.
    pub fn can_read_application(
        context: &impl Authority,
        scope: &ApplicationScope,
        application_id: &ApplicationId,
    ) -> Result<()> {
        can_read_applications_or_own(context)?;
        if can_read_all_applications(context) || scope.allows(application_id) {
            return Ok(());
        }
        Err(PlatformError::forbidden_code(
            "APPLICATION_ACCESS_REQUIRED",
            "not authorised for this application",
        ))
    }

    /// Clients, read (list, search, by identifier, get): anchor plus
    /// `platform:admin:client:view` (Go `CanReadClients`, auth.go:713).
    pub fn can_read_clients(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::admin::CLIENT_READ)
    }

    /// Connections, read: `platform:messaging:connection:view` (Go
    /// `CanReadConnections`, auth.go:515). Rows are then confined to the
    /// caller's clients.
    pub fn can_read_connections(context: &impl Authority) -> Result<()> {
        require_permission(context, permissions::admin::CONNECTION_READ)
    }

    /// Connections, create: `platform:messaging:connection:create` (Go
    /// `CanCreateConnections`, auth.go:517).
    pub fn can_create_connections(context: &impl Authority) -> Result<()> {
        require_permission(context, permissions::admin::CONNECTION_CREATE)
    }

    /// Connections, update, pause and activate:
    /// `platform:messaging:connection:update` (Go `CanUpdateConnections`).
    pub fn can_update_connections(context: &impl Authority) -> Result<()> {
        require_permission(context, permissions::admin::CONNECTION_UPDATE)
    }

    /// Connections, delete: `platform:messaging:connection:delete` (Go
    /// `CanDeleteConnections`).
    pub fn can_delete_connections(context: &impl Authority) -> Result<()> {
        require_permission(context, permissions::admin::CONNECTION_DELETE)
    }

    /// Dispatch pools, any write (create, update, archive, suspend,
    /// activate): one of the pool create/update/delete permissions (Go
    /// `CanWriteDispatchPools`, auth.go:561).
    pub fn can_write_dispatch_pools(context: &impl Authority) -> Result<()> {
        require_any_permission(
            context,
            &[
                permissions::admin::DISPATCH_POOL_CREATE,
                permissions::admin::DISPATCH_POOL_UPDATE,
                permissions::admin::DISPATCH_POOL_DELETE,
            ],
        )
    }

    /// Dispatch pools, delete: `platform:messaging:dispatch-pool:delete` (Go
    /// `CanDeleteDispatchPools`, auth.go:557).
    pub fn can_delete_dispatch_pools(context: &impl Authority) -> Result<()> {
        require_permission(context, permissions::admin::DISPATCH_POOL_DELETE)
    }

    /// CORS origins, read: anchor plus `platform:admin:cors-origin:view` (Go
    /// `CanReadCorsOrigins`, auth.go:788).
    pub fn can_read_cors_origins(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::admin::CORS_ORIGIN_READ)
    }

    /// Dispatch pools, read: `platform:messaging:dispatch-pool:view` (Go
    /// `CanReadDispatchPools`, auth.go:547). Rows are then confined to the
    /// caller's clients.
    pub fn can_read_dispatch_pools(context: &impl Authority) -> Result<()> {
        require_permission(context, permissions::admin::DISPATCH_POOL_READ)
    }

    /// Email-domain mappings, read: anchor plus
    /// `platform:iam:email-domain-mapping:view` (Go
    /// `CanReadEmailDomainMappings`, auth.go:763).
    pub fn can_read_email_domain_mappings(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::admin::EMAIL_DOMAIN_MAPPING_READ)
    }

    /// Login attempts, read: anchor plus `platform:admin:login-attempt:view`
    /// (Go `CanReadLoginAttempts`, auth.go:793).
    pub fn can_read_login_attempts(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::admin::LOGIN_ATTEMPT_READ)
    }

    /// Principals, read: `platform:iam:user:view` (Go `CanReadPrincipals`,
    /// auth.go:800). Rows are then confined to the caller's clients.
    pub fn can_read_principals(context: &impl Authority) -> Result<()> {
        require_permission(context, permissions::iam::USER_READ)
    }

    /// The dashboard's platform-wide counts: anchor, then the client or the
    /// application view permission (Go's stats handler: `RequireAnchor` then
    /// `CanViewDashboardStats`, auth.go:393).
    pub fn can_view_dashboard_stats(context: &impl Authority) -> Result<()> {
        require_anchor_scope(context)?;
        require_any_permission(
            context,
            &[
                permissions::admin::CLIENT_READ,
                permissions::admin::APPLICATION_READ,
            ],
        )
    }

    /// Service accounts, create and update: any of the service-account
    /// create/update/delete permissions, as Go's `CanWriteServiceAccounts`
    /// (auth.go:691-693), and anchor scope on top. Go has no anchor check
    /// here; Rust keeps one because an account's tier follows its client
    /// links, so a non-anchor holder could otherwise create or relink a
    /// client-less account, which is ANCHOR tier.
    pub fn can_write_service_accounts(context: &impl Authority) -> Result<()> {
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
    pub fn can_update_service_accounts(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::admin::SERVICE_ACCOUNT_UPDATE)
    }

    /// Service accounts, delete: `platform:iam:service-account:delete`, as
    /// Go's `CanDeleteServiceAccounts` (auth.go:687-689), with the same
    /// anchor requirement as [`can_write_service_accounts`].
    pub fn can_delete_service_accounts(context: &impl Authority) -> Result<()> {
        require_anchor_scope(context)?;
        require_permission(context, permissions::admin::SERVICE_ACCOUNT_DELETE)
    }

    // ── Platform-owner routes: anchor reach plus the permission ─────────
    //
    // Go's `anchorWith(perm)` families (shared/auth/auth.go:704-790). These
    // were anchor-only here, which let any anchor principal (a read-only
    // staff role, a provisioned service account) write them.

    /// Clients, create (Go `CanCreateClients`).
    pub fn can_create_clients(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::admin::CLIENT_CREATE)
    }

    /// Clients, update, notes and enabled applications (Go
    /// `CanUpdateClients`; Java ClientApi for the application links, which
    /// Go gates by anchor alone).
    pub fn can_update_clients(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::admin::CLIENT_UPDATE)
    }

    /// Clients, delete (Go `CanDeleteClients`).
    pub fn can_delete_clients(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::admin::CLIENT_DELETE)
    }

    /// Clients, activate (Go `CanActivateClients`).
    pub fn can_activate_clients(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::admin::CLIENT_ACTIVATE)
    }

    /// Clients, suspend (Go `CanSuspendClients`).
    pub fn can_suspend_clients(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::admin::CLIENT_SUSPEND)
    }

    /// Clients, deactivate (Go `CanDeactivateClients`).
    pub fn can_deactivate_clients(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::admin::CLIENT_DEACTIVATE)
    }

    /// Identity providers, read; also IdP role mappings, read (Go
    /// `CanReadIdentityProviders`).
    pub fn can_read_identity_providers(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::admin::IDENTITY_PROVIDER_READ)
    }

    /// Identity providers, create (Go `CanCreateIdentityProviders`).
    pub fn can_create_identity_providers(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::admin::IDENTITY_PROVIDER_CREATE)
    }

    /// Identity providers, update; also IdP role mappings, create and
    /// delete (Go `CanUpdateIdentityProviders`).
    pub fn can_update_identity_providers(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::admin::IDENTITY_PROVIDER_UPDATE)
    }

    /// Identity providers, delete (Go `CanDeleteIdentityProviders`).
    pub fn can_delete_identity_providers(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::admin::IDENTITY_PROVIDER_DELETE)
    }

    /// Email-domain mappings, create (Go `CanCreateEmailDomainMappings`).
    pub fn can_create_email_domain_mappings(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::admin::EMAIL_DOMAIN_MAPPING_CREATE)
    }

    /// Email-domain mappings, update (Go `CanUpdateEmailDomainMappings`).
    pub fn can_update_email_domain_mappings(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::admin::EMAIL_DOMAIN_MAPPING_UPDATE)
    }

    /// Email-domain mappings, delete (Go `CanDeleteEmailDomainMappings`).
    pub fn can_delete_email_domain_mappings(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::admin::EMAIL_DOMAIN_MAPPING_DELETE)
    }

    /// Anchor domains, read (Go `CanReadAnchorDomains`).
    pub fn can_read_anchor_domains(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::admin::ANCHOR_DOMAIN_READ)
    }

    /// Anchor domains, create (Go `CanCreateAnchorDomains`).
    pub fn can_create_anchor_domains(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::admin::ANCHOR_DOMAIN_CREATE)
    }

    /// Anchor domains, update (Go `CanUpdateAnchorDomains`).
    pub fn can_update_anchor_domains(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::admin::ANCHOR_DOMAIN_UPDATE)
    }

    /// Anchor domains, delete (Go `CanDeleteAnchorDomains`).
    pub fn can_delete_anchor_domains(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::admin::ANCHOR_DOMAIN_DELETE)
    }

    /// Client auth configs, read (Go `CanReadAuthConfigs`).
    pub fn can_read_auth_configs(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::auth::CLIENT_AUTH_CONFIG_READ)
    }

    /// Client auth configs, create (Go `CanCreateAuthConfigs`).
    pub fn can_create_auth_configs(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::auth::CLIENT_AUTH_CONFIG_CREATE)
    }

    /// Client auth configs, update (Go `CanUpdateAuthConfigs`).
    pub fn can_update_auth_configs(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::auth::CLIENT_AUTH_CONFIG_UPDATE)
    }

    /// Client auth configs, delete (Go `CanDeleteAuthConfigs`).
    pub fn can_delete_auth_configs(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::auth::CLIENT_AUTH_CONFIG_DELETE)
    }

    /// CORS origins, create (Go `CanCreateCorsOrigins`).
    pub fn can_create_cors_origins(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::admin::CORS_ORIGIN_CREATE)
    }

    /// CORS origins, delete (Go `CanDeleteCorsOrigins`).
    pub fn can_delete_cors_origins(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::admin::CORS_ORIGIN_DELETE)
    }

    /// Applications, create, update, activate, deactivate: any of the
    /// application create/update/delete permissions (Go's
    /// `CanWriteApplications`). The handlers keep their anchor check on top.
    pub fn can_write_applications(context: &impl Authority) -> Result<()> {
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
    pub fn can_delete_applications(context: &impl Authority) -> Result<()> {
        require_permission(context, permissions::admin::APPLICATION_DELETE)
    }

    /// Role administration through `/api/roles` and `/bff/roles`: the role
    /// permission, as Go asks (`CanDeleteRoles`), then anchor reach (owner
    /// decision #25, stricter than Go). The permission is checked first, so
    /// a caller lacking it is refused exactly as Go refuses it; only a
    /// non-anchor holder of the permission meets the extra anchor rule.
    pub fn can_administer_roles(context: &impl Authority, permission: &str) -> Result<()> {
        require_permission(context, permission)?;
        require_anchor_scope(context)
    }

    /// Role administration through `/bff/roles`: anchor scope, as Go's BFF
    /// asks (`RequireAnchor`, shared/bff/roles.go), then the role
    /// permission (owner decision #25).
    pub fn can_administer_bff_roles(context: &impl Authority, permission: &str) -> Result<()> {
        anchor_with(context, permission)
    }

    /// Role create and update through `/api/roles`: any role write
    /// permission, as Go's `CanWriteRoles` (`one of: …`), then anchor reach
    /// (owner decision #25), in that order for the reason given on
    /// [`can_administer_roles`].
    pub fn can_write_roles(context: &impl Authority) -> Result<()> {
        require_any_permission(
            context,
            &[
                permissions::iam::ROLE_CREATE,
                permissions::iam::ROLE_UPDATE,
                permissions::iam::ROLE_DELETE,
            ],
        )?;
        require_anchor_scope(context)
    }

    /// Re-running the built-in role sync: anchor and any role write
    /// permission (Java RolesBff sync-platform).
    pub fn can_sync_platform_roles(context: &impl Authority) -> Result<()> {
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
    pub fn can_grant_client_access(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::iam::CLIENT_ACCESS_GRANT)
    }

    /// See [`can_grant_client_access`].
    pub fn can_revoke_client_access(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::iam::CLIENT_ACCESS_REVOKE)
    }

    /// Principals, the user-administration writes (create, update,
    /// activate, deactivate, password reset, application access): any of the
    /// user create/update/delete permissions, as Go's `CanWritePrincipals`
    /// (shared/auth/auth.go, reached through `RequireUserAdmin`). Scope is
    /// reach, never authority: an anchor needs the permission too. The
    /// handler keeps its own tier check on top.
    pub fn can_write_principals(context: &impl Authority) -> Result<()> {
        require_any_permission(
            context,
            &[
                permissions::iam::USER_CREATE,
                permissions::iam::USER_UPDATE,
                permissions::iam::USER_DELETE,
            ],
        )
    }

    /// Go `RequireUserAdmin` (shared/auth/auth.go:369-385) for a user in
    /// client `target_client_id`: an anchor needs a user-write permission;
    /// anyone else also reaches that client (403 `SCOPE_FORBIDDEN`), and a
    /// client-less (platform) user is an anchor's alone (403
    /// `ANCHOR_REQUIRED`). A client administrator is confined to its own
    /// clients this way.
    pub fn require_user_admin(
        context: &impl Authority,
        target_client_id: Option<&ClientId>,
    ) -> Result<()> {
        if context.is_anchor() {
            return can_write_principals(context);
        }
        let Some(client_id) = target_client_id else {
            return Err(PlatformError::forbidden_code(
                "ANCHOR_REQUIRED",
                "anchor scope required for platform users",
            ));
        };
        if !context.can_access_client(client_id) {
            return Err(PlatformError::forbidden_code(
                "SCOPE_FORBIDDEN",
                "no access to this user's client",
            ));
        }
        can_write_principals(context)
    }

    /// Granting a principal every application (`allApplications`): only a
    /// caller that itself reaches every application may (Go
    /// serviceaccount/api/api.go:157, principal/api/api.go:1204). `scope`
    /// is the caller's resolved application scope; unresolved grants
    /// nothing.
    pub fn require_all_applications_grantor(scope: Option<&ApplicationScope>) -> Result<()> {
        if scope == Some(&ApplicationScope::All) {
            Ok(())
        } else {
            Err(PlatformError::forbidden(
                "Only an all-applications administrator may grant all-applications access",
            ))
        }
    }

    /// Principals, setting a user's roles (add, remove, replace):
    /// `platform:iam:user:assign-roles` itself (owner ruling 14; Java
    /// `Access.requireRoleAssigner`). Holding user create, update or delete
    /// no longer changes roles; the role ceiling then bounds which roles.
    pub fn can_assign_principal_roles(context: &impl Authority) -> Result<()> {
        require_permission(context, permissions::iam::USER_ASSIGN_ROLES)
    }

    /// Principals, delete: `platform:iam:user:delete`, as Go's
    /// `CanDeletePrincipals`.
    pub fn can_delete_principals(context: &impl Authority) -> Result<()> {
        require_permission(context, permissions::iam::USER_DELETE)
    }

    /// Platform-config access grants, read: anchor plus
    /// `platform:admin:config:view` (Go's `CanReadPlatformConfig`,
    /// shared/auth/auth.go:784).
    pub fn can_read_platform_config(context: &impl Authority) -> Result<()> {
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
    /// `platform:admin:config:manage` (Go's `CanUpdatePlatformConfig`,
    /// shared/auth/auth.go:785, checks `…:config:update`; owner decision #44
    /// renames it). A stored role holding Go's `…:config:update` still passes.
    pub fn can_update_platform_config(context: &impl Authority) -> Result<()> {
        require_anchor(context)?;
        if context.has_permission(permissions::admin::CONFIG_MANAGE)
            || context.has_permission(permissions::admin::CONFIG_UPDATE_GO)
        {
            Ok(())
        } else {
            Err(PlatformError::forbidden(
                "Cannot update platform config access",
            ))
        }
    }

    /// The `/bff/developer` reads: anchor scope, then
    /// `platform:developer:application-openapi:view` (Go
    /// `CanReadDeveloperPortal`, shared/auth/auth.go:796).
    pub fn can_read_developer_portal(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::developer::APPLICATION_OPENAPI_VIEW)
    }

    /// `POST /bff/developer/sync-platform-openapi`: anchor scope, then
    /// `platform:developer:application-openapi:sync` (Go
    /// `CanSyncPlatformOpenAPI`, shared/auth/auth.go:797).
    pub fn can_sync_platform_openapi(context: &impl Authority) -> Result<()> {
        anchor_with(context, permissions::developer::APPLICATION_OPENAPI_SYNC)
    }

    /// Developer portal: read an application's OpenAPI document.
    /// Resource scoping (which application the principal can see) is handled
    /// in the handler against `iam_principal_application_access`.
    pub fn can_read_application_openapi(context: &impl Authority) -> Result<()> {
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
    pub fn can_sync_application_openapi(context: &impl Authority) -> Result<()> {
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
    pub fn can_read_events(context: &impl Authority) -> Result<()> {
        if context.has_permission(permissions::admin::EVENT_READ) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot read events"))
        }
    }

    /// Audit logs, read: `platform:admin:audit-log:view`, answering as Go's
    /// `CanWritePermission(ac, viewPerm)` (audit/api/api.go:43).
    pub fn can_read_audit_logs(context: &impl Authority) -> Result<()> {
        require_permission(context, permissions::admin::AUDIT_LOG_READ)
    }

    /// Check raw read access to events (includes payload)
    pub fn can_read_events_raw(context: &impl Authority) -> Result<()> {
        if context.has_permission(permissions::admin::EVENT_VIEW_RAW) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot read raw event data"))
        }
    }

    /// Check read access to event types
    pub fn can_read_event_types(context: &impl Authority) -> Result<()> {
        if context.has_permission(permissions::admin::EVENT_TYPE_READ) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot read event types"))
        }
    }

    /// Check create access to event types
    pub fn can_create_event_types(context: &impl Authority) -> Result<()> {
        if context.has_permission(permissions::admin::EVENT_TYPE_CREATE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot create event types"))
        }
    }

    /// Check update access to event types
    pub fn can_update_event_types(context: &impl Authority) -> Result<()> {
        if context.has_permission(permissions::admin::EVENT_TYPE_UPDATE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot update event types"))
        }
    }

    /// Check delete access to event types
    pub fn can_delete_event_types(context: &impl Authority) -> Result<()> {
        if context.has_permission(permissions::admin::EVENT_TYPE_DELETE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot delete event types"))
        }
    }

    /// Check read access to subscriptions
    pub fn can_read_subscriptions(context: &impl Authority) -> Result<()> {
        if context.has_permission(permissions::admin::SUBSCRIPTION_READ) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot read subscriptions"))
        }
    }

    /// Check create access to subscriptions
    pub fn can_create_subscriptions(context: &impl Authority) -> Result<()> {
        if context.has_permission(permissions::admin::SUBSCRIPTION_CREATE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot create subscriptions"))
        }
    }

    /// Check update access to subscriptions
    pub fn can_update_subscriptions(context: &impl Authority) -> Result<()> {
        if context.has_permission(permissions::admin::SUBSCRIPTION_UPDATE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot update subscriptions"))
        }
    }

    /// Check delete access to subscriptions
    pub fn can_delete_subscriptions(context: &impl Authority) -> Result<()> {
        if context.has_permission(permissions::admin::SUBSCRIPTION_DELETE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot delete subscriptions"))
        }
    }

    /// Check read access to dispatch jobs
    pub fn can_read_dispatch_jobs(context: &impl Authority) -> Result<()> {
        if context.has_permission(permissions::admin::DISPATCH_JOB_READ) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot read dispatch jobs"))
        }
    }

    /// Check raw read access to dispatch jobs (includes payload)
    pub fn can_read_dispatch_jobs_raw(context: &impl Authority) -> Result<()> {
        if context.has_permission(permissions::admin::DISPATCH_JOB_VIEW_RAW) {
            Ok(())
        } else {
            Err(PlatformError::forbidden(
                "Cannot read raw dispatch job data",
            ))
        }
    }

    /// Check admin access (any admin permission)
    pub fn is_admin(context: &impl Authority) -> Result<()> {
        if context.is_anchor() || context.has_permission(permissions::ADMIN_ALL) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Admin access required"))
        }
    }

    /// Check write access to events (create)
    pub fn can_write_events(context: &impl Authority) -> Result<()> {
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
    pub fn can_write_event_types(context: &impl Authority) -> Result<()> {
        // Go `CanWriteEventTypes`: 403 `PERMISSION_REQUIRED`, `one of: …`.
        require_any_permission(
            context,
            &[
                permissions::admin::EVENT_TYPE_CREATE,
                permissions::admin::EVENT_TYPE_UPDATE,
                permissions::admin::EVENT_TYPE_DELETE,
            ],
        )
    }

    // ── Process documentation ────────────────────────────────────────────

    /// Check read access to processes
    pub fn can_read_processes(context: &impl Authority) -> Result<()> {
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
    pub fn can_create_processes(context: &impl Authority) -> Result<()> {
        if context.has_permission(permissions::admin::PROCESS_CREATE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot create processes"))
        }
    }

    /// Check update access to processes
    pub fn can_update_processes(context: &impl Authority) -> Result<()> {
        if context.has_permission(permissions::admin::PROCESS_UPDATE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot update processes"))
        }
    }

    /// Check delete access to processes
    pub fn can_delete_processes(context: &impl Authority) -> Result<()> {
        if context.has_permission(permissions::admin::PROCESS_DELETE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot delete processes"))
        }
    }

    /// Check write access to processes (create, update, archive, or delete)
    pub fn can_write_processes(context: &impl Authority) -> Result<()> {
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
    pub fn can_sync_processes(context: &impl Authority) -> Result<()> {
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
    pub fn can_write_subscriptions(context: &impl Authority) -> Result<()> {
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
    pub fn can_create_dispatch_jobs(context: &impl Authority) -> Result<()> {
        if context.has_permission(permissions::admin::BATCH_DISPATCH_JOBS_WRITE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot create dispatch jobs"))
        }
    }

    /// Check retry access to dispatch jobs
    pub fn can_retry_dispatch_jobs(context: &impl Authority) -> Result<()> {
        if context.has_permission(permissions::admin::BATCH_DISPATCH_JOBS_WRITE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot retry dispatch jobs"))
        }
    }

    /// Check write access to dispatch jobs (batch)
    pub fn can_write_dispatch_jobs(context: &impl Authority) -> Result<()> {
        if context.has_permission(permissions::admin::BATCH_DISPATCH_JOBS_WRITE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot write dispatch jobs"))
        }
    }

    // ── Scheduled jobs ──────────────────────────────────────────────────────

    pub fn can_read_scheduled_jobs(context: &impl Authority) -> Result<()> {
        if context.has_permission(permissions::admin::SCHEDULED_JOB_READ) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot read scheduled jobs"))
        }
    }

    pub fn can_create_scheduled_jobs(context: &impl Authority) -> Result<()> {
        if context.has_permission(permissions::admin::SCHEDULED_JOB_CREATE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot create scheduled jobs"))
        }
    }

    pub fn can_update_scheduled_jobs(context: &impl Authority) -> Result<()> {
        if context.has_permission(permissions::admin::SCHEDULED_JOB_UPDATE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot update scheduled jobs"))
        }
    }

    pub fn can_delete_scheduled_jobs(context: &impl Authority) -> Result<()> {
        if context.has_permission(permissions::admin::SCHEDULED_JOB_DELETE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot delete scheduled jobs"))
        }
    }

    pub fn can_pause_scheduled_jobs(context: &impl Authority) -> Result<()> {
        if context.has_permission(permissions::admin::SCHEDULED_JOB_PAUSE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot pause scheduled jobs"))
        }
    }

    pub fn can_fire_scheduled_jobs(context: &impl Authority) -> Result<()> {
        if context.has_permission(permissions::admin::SCHEDULED_JOB_FIRE) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot fire scheduled jobs"))
        }
    }

    /// Go's `CanWriteScheduledJobs` (shared/auth/auth.go:881): any of
    /// scheduled-job create, update or delete. Go gates update, pause,
    /// resume, archive and the instance log/complete callbacks with it.
    pub fn can_write_scheduled_jobs(context: &impl Authority) -> Result<()> {
        require_any_permission(
            context,
            &[
                permissions::admin::SCHEDULED_JOB_CREATE,
                permissions::admin::SCHEDULED_JOB_UPDATE,
                permissions::admin::SCHEDULED_JOB_DELETE,
            ],
        )
    }

    /// Go's `CheckScopeAccess` (shared/auth/auth.go:433-444): a
    /// client-scoped resource needs that client, a platform one (no client)
    /// anchor or super-admin; otherwise 403 `SCOPE_FORBIDDEN`. One rule,
    /// kept in [`crate::shared::caller_reach::check_scope_access`].
    pub fn check_scope_access(
        context: &impl Authority,
        client_id: Option<&ClientId>,
    ) -> Result<()> {
        caller_reach::require_scope_access(context, client_id)
    }
    /// Sync endpoints: admin path. Application-scoped sync uses the
    /// application_service permission below.
    pub fn can_sync_scheduled_jobs(context: &impl Authority) -> Result<()> {
        if context.has_any_permission(&[
            permissions::admin::SCHEDULED_JOB_SYNC,
            permissions::admin::SCHEDULED_JOB_MANAGE,
        ]) {
            Ok(())
        } else {
            Err(PlatformError::forbidden("Cannot sync scheduled jobs"))
        }
    }

    pub fn can_read_scheduled_job_instances(context: &impl Authority) -> Result<()> {
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
    /// Go gates these with `CanWriteScheduledJobs` (scheduled-job create,
    /// update or delete), so those grant it too.
    pub fn can_write_scheduled_job_instance(context: &impl Authority) -> Result<()> {
        if context.has_any_permission(&[
            permissions::application_service::SCHEDULED_JOB_INSTANCE_WRITE,
            permissions::admin::SCHEDULED_JOB_MANAGE,
            permissions::admin::SCHEDULED_JOB_CREATE,
            permissions::admin::SCHEDULED_JOB_UPDATE,
            permissions::admin::SCHEDULED_JOB_DELETE,
        ]) {
            Ok(())
        } else {
            Err(PlatformError::forbidden(
                "Cannot write to scheduled job instance",
            ))
        }
    }

    /// SDK-driven sync of scheduled-job definitions for an application.
    pub fn can_sync_scheduled_jobs_app(context: &impl Authority) -> Result<()> {
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

    pub fn can_sync_event_types(context: &impl Authority) -> Result<()> {
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

    pub fn can_sync_subscriptions(context: &impl Authority) -> Result<()> {
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

    /// Roles and the permission catalogue, read (`/api/roles/*`, and the
    /// application-scoped SDK list): `platform:iam:role:view` as Go's
    /// `CanReadRoles` (auth.go:588), or role manage, or the
    /// application-service role view. Refused as Go: 403
    /// `PERMISSION_REQUIRED`.
    pub fn can_read_roles(context: &impl Authority) -> Result<()> {
        require_any_permission(
            context,
            &[
                permissions::iam::ROLE_READ,
                permissions::iam::ROLE_MANAGE,
                permissions::application_service::ROLE_READ,
            ],
        )
    }

    /// Create a single role through the application-scoped SDK surface.
    pub fn can_create_roles(context: &impl Authority) -> Result<()> {
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
    pub fn can_delete_roles(context: &impl Authority) -> Result<()> {
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

    pub fn can_sync_roles(context: &impl Authority) -> Result<()> {
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
    pub fn can_sync_dispatch_pools(context: &impl Authority) -> Result<()> {
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
    pub fn can_sync_principals(context: &impl Authority) -> Result<()> {
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
    pub fn require_application_access<A: ScopedApplication>(
        scope: &ApplicationScope,
        app_code: &str,
        application: Option<A>,
    ) -> Result<A> {
        match application {
            Some(app) if scope.allows(app.application_id()) => Ok(app),
            _ => Err(PlatformError::not_found("Application", app_code)),
        }
    }

    /// [`require_application_access`] in a use case's `authorize`: the
    /// caller's resolved application scope (the system caller reaches every
    /// application; an unresolved scope none) against `application`, with
    /// the same 404 as a missing application.
    pub fn require_caller_application_access<A: ScopedApplication>(
        caller: &Caller,
        app_code: &str,
        application: Option<A>,
    ) -> Result<A> {
        match application {
            Some(app) if caller.allows_application(app.application_id()) => Ok(app),
            _ => Err(PlatformError::not_found("Application", app_code)),
        }
    }
}
