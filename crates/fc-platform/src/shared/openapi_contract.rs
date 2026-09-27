//! The operations Go documents that this platform routes through plain axum
//! routers (generic handlers `routes!` cannot take, or routers that also
//! carry undocumented routes), added to the published document so that
//! `/q/openapi` describes the same programmable surface as Go's huma document
//! (`flowcatalyst-go/api/openapi.lock.json`, vendored as
//! `frontend/openapi/openapi.json`). The SDKs' generated clients are
//! generated from that document; `tests/openapi_go_contract_test.rs` pins the
//! operation ids.
//!
//! Only the handlers Go documents are listed. Routes Go lacks (e.g.
//! `/api/anchor-domains/check/{domain}`, the auth-config sub-resources) stay
//! out of the document, as before.

use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(paths(
    crate::auth::config_api::list_anchor_domains,
    crate::auth::config_api::create_anchor_domain,
    crate::auth::config_api::update_anchor_domain,
    crate::auth::config_api::delete_anchor_domain,
))]
struct AnchorDomainsDoc;

#[derive(OpenApi)]
#[openapi(paths(
    crate::auth::config_api::list_client_auth_configs,
    crate::auth::config_api::create_client_auth_config,
    crate::auth::config_api::update_client_auth_config,
    crate::auth::config_api::delete_client_auth_config,
))]
struct AuthConfigsDoc;

#[derive(OpenApi)]
#[openapi(paths(
    crate::auth::config_api::list_idp_role_mappings,
    crate::auth::config_api::create_idp_role_mapping,
    crate::auth::config_api::delete_idp_role_mapping,
))]
struct IdpRoleMappingsDoc;

#[derive(OpenApi)]
#[openapi(paths(
    crate::application::api::list_applications,
    crate::application::api::create_application,
    crate::application::api::get_application_by_code,
    crate::application::api::list_application_roles,
    crate::application::api::get_application,
    crate::application::api::update_application,
    crate::application::api::delete_application,
    crate::application::api::activate_application,
    crate::application::api::deactivate_application,
    crate::application::api::list_client_configs,
    crate::application::api::enable_for_client,
    crate::application::api::disable_for_client,
    crate::application::api::provision_login_client,
    crate::application::api::provision_service_account,
))]
struct ApplicationsDoc;

#[derive(OpenApi)]
#[openapi(paths(
    crate::connection::api::list_connections,
    crate::connection::api::create_connection,
    crate::connection::api::get_connection,
    crate::connection::api::update_connection,
    crate::connection::api::delete_connection,
    crate::connection::api::activate_connection,
    crate::connection::api::pause_connection,
))]
struct ConnectionsDoc;

#[derive(OpenApi)]
#[openapi(paths(
    crate::dispatch_pool::api::list_dispatch_pools,
    crate::dispatch_pool::api::create_dispatch_pool,
    crate::dispatch_pool::api::get_dispatch_pool,
    crate::dispatch_pool::api::update_dispatch_pool,
    crate::dispatch_pool::api::delete_dispatch_pool,
    crate::dispatch_pool::api::activate_dispatch_pool,
    crate::dispatch_pool::api::archive_dispatch_pool,
    crate::dispatch_pool::api::suspend_dispatch_pool,
))]
struct DispatchPoolsDoc;

#[derive(OpenApi)]
#[openapi(paths(
    crate::email_domain_mapping::api::list_email_domain_mappings,
    crate::email_domain_mapping::api::create_email_domain_mapping,
    crate::email_domain_mapping::api::get_email_domain_mapping,
    crate::email_domain_mapping::api::update_email_domain_mapping,
    crate::email_domain_mapping::api::delete_email_domain_mapping,
))]
struct EmailDomainMappingsDoc;

#[derive(OpenApi)]
#[openapi(paths(
    crate::identity_provider::api::list_identity_providers,
    crate::identity_provider::api::create_identity_provider,
    crate::identity_provider::api::get_identity_provider,
    crate::identity_provider::api::update_identity_provider,
    crate::identity_provider::api::delete_identity_provider,
))]
struct IdentityProvidersDoc;

#[derive(OpenApi)]
#[openapi(paths(crate::login_attempt::api::list_login_attempts))]
struct LoginAttemptsDoc;

#[derive(OpenApi)]
#[openapi(paths(
    crate::cors::api::list_cors_origins,
    crate::cors::api::create_cors_origin,
    crate::cors::api::get_allowed_origins,
    crate::cors::api::get_cors_origin,
    crate::cors::api::delete_cors_origin,
))]
struct CorsDoc;

#[derive(OpenApi)]
#[openapi(paths(
    crate::portal::api::list_portal_apps,
    crate::portal::api::create_portal_app,
    crate::portal::api::update_portal_app,
    crate::portal::api::delete_portal_app,
    crate::portal::api::assign_unassigned_portal_users,
))]
struct PortalAppsDoc;

#[derive(OpenApi)]
#[openapi(paths(
    crate::portal::api::list_portal_users,
    crate::portal::api::ensure_portal_user,
    crate::portal::api::delete_portal_user,
    crate::portal::api::activate_portal_user,
    crate::portal::api::deactivate_portal_user,
    crate::portal::api::grant_portal_user_app,
    crate::portal::api::revoke_portal_user_app,
))]
struct PortalUsersDoc;

#[derive(OpenApi)]
#[openapi(paths(
    crate::service_account::api::list_service_accounts,
    crate::service_account::api::create_service_account,
    crate::service_account::api::get_service_account_by_code,
    crate::service_account::api::get_service_account,
    crate::service_account::api::update_service_account,
    crate::service_account::api::delete_service_account,
    crate::service_account::api::get_roles,
    crate::service_account::api::assign_roles,
    crate::service_account::api::regenerate_auth_token,
    crate::service_account::api::regenerate_token_alias,
    crate::service_account::api::regenerate_signing_secret,
    crate::service_account::api::regenerate_secret_alias,
))]
struct ServiceAccountsDoc;

#[derive(OpenApi)]
#[openapi(paths(crate::shared::batch_api::batch_events))]
struct EventsBatchDoc;

/// The operations above, at their mount points.
pub fn documented_plain_routes() -> utoipa::openapi::OpenApi {
    utoipa::openapi::OpenApiBuilder::new()
        .build()
        .nest("/api/anchor-domains", AnchorDomainsDoc::openapi())
        .nest("/api/auth-configs", AuthConfigsDoc::openapi())
        .nest("/api/idp-role-mappings", IdpRoleMappingsDoc::openapi())
        .nest("/api/applications", ApplicationsDoc::openapi())
        .nest("/api/connections", ConnectionsDoc::openapi())
        .nest("/api/dispatch-pools", DispatchPoolsDoc::openapi())
        .nest(
            "/api/email-domain-mappings",
            EmailDomainMappingsDoc::openapi(),
        )
        .nest("/api/identity-providers", IdentityProvidersDoc::openapi())
        .nest("/api/login-attempts", LoginAttemptsDoc::openapi())
        .nest("/api/platform/cors", CorsDoc::openapi())
        .nest("/api/portal-apps", PortalAppsDoc::openapi())
        .nest("/api/portal-users", PortalUsersDoc::openapi())
        .nest("/api/service-accounts", ServiceAccountsDoc::openapi())
        .nest("/api/events", EventsBatchDoc::openapi())
}
