//! Auth routes: `/api/oauth-clients`, the auth-config resources
//! (`/api/anchor-domains`, `/api/auth-configs`, `/api/idp-role-mappings`),
//! and the sign-in edge: `/auth` (password login, OIDC login, password
//! setup and reset) behind the shared `/auth` per-IP limit, and `/oauth`
//! behind its own per-IP limit and the distributed token-endpoint budget.

use std::sync::Arc;

use axum::response::{IntoResponse, Json};
use axum::routing::{delete, get, post};
use axum::Router;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::auth_api::AuthState;
use super::config_api::AuthConfigState;
use super::oauth_api::OAuthState;
use super::oauth_clients_api::OAuthClientsState;
use super::oidc_login_api::OidcLoginApiState;
use super::operations::{
    ActivateOAuthClientUseCase, CreateAnchorDomainUseCase, CreateAuthConfigUseCase,
    CreateIdpRoleMappingUseCase, CreateOAuthClientUseCase, DeactivateOAuthClientUseCase,
    DeleteAnchorDomainUseCase, DeleteAuthConfigUseCase, DeleteIdpRoleMappingUseCase,
    DeleteOAuthClientUseCase, RevokeOAuthClientPreviousSecretUseCase,
    RotateOAuthClientSecretUseCase, UpdateAnchorDomainUseCase, UpdateAuthConfigUseCase,
    UpdateOAuthClientUseCase,
};
use super::password_reset_api::PasswordResetApiState;
use super::session_cookie::SessionCookieConfig;
use crate::shared::encryption_service::EncryptionService;
use crate::shared::platform_context::{AggregateRoutes, PlatformContext};
use crate::shared::rate_limit_middleware::{
    rate_limit_per_ip, IpRateLimiterState, RateLimitConfig,
};
use crate::shared::rate_limit_store::{
    distributed_rate_limit_per_email, distributed_rate_limit_per_ip, Bucket,
    DistributedEmailLimitState, DistributedIpLimitState,
};

pub fn routes(ctx: &PlatformContext) -> AggregateRoutes {
    // Per-IP limits: `/auth` and `/oauth` have separate buckets so a
    // high-volume OAuth client doesn't starve the login flow (and vice
    // versa). They compose with the per-account backoff in
    // `auth::login_backoff`.
    let auth_rate_limit =
        axum::middleware::from_fn_with_state(ctx.auth_ip_limit.clone(), rate_limit_per_ip);
    let oauth_rate_limit =
        axum::middleware::from_fn_with_state(ctx.oauth_ip_limit.clone(), rate_limit_per_ip);
    // Distributed (cluster-wide) per-IP limits on top of the in-memory ones:
    // the governor rejects bursts at this instance (sub-ms, no I/O), the
    // store catches one source spreading load across replicas.
    let policies = &ctx.config.rate_limit_policies;
    let oauth_token_budget = axum::middleware::from_fn_with_state(
        DistributedIpLimitState {
            store: ctx.config.rate_limit_store.clone(),
            bucket: Bucket::OAUTH_TOKEN_IP,
            policy: policies.oauth_token_ip,
        },
        distributed_rate_limit_per_ip,
    );
    let password_reset_budget = axum::middleware::from_fn_with_state(
        DistributedIpLimitState {
            store: ctx.config.rate_limit_store.clone(),
            bucket: Bucket::PASSWORD_RESET_IP,
            policy: policies.password_reset_ip,
        },
        distributed_rate_limit_per_ip,
    );
    // S2.7: the reset request is budgeted per address too, and over budget
    // it answers exactly as it always does, so the limit can't be used to
    // tell a known address from an unknown one.
    let password_reset_email_budget = axum::middleware::from_fn_with_state(
        DistributedEmailLimitState {
            store: ctx.config.rate_limit_store.clone(),
            bucket: Bucket::PASSWORD_RESET_EMAIL,
            policy: policies.password_reset_email,
            path_suffix: "/request",
            over_budget: password_reset_requested_response,
        },
        distributed_rate_limit_per_email,
    );
    // The portal plane's hooks answer the portal-subject requests of the
    // shared reset-token and OIDC callback routes (Go wire_routes.go).
    let portal_reset_hook = axum::middleware::from_fn_with_state(
        ctx.portal.passwords.clone(),
        crate::portal::password::intercept,
    );
    let portal_oidc_hook = axum::middleware::from_fn_with_state(
        crate::portal::routes::portal_login_state(ctx),
        crate::portal::oidc::intercept,
    );
    let password_reset = password_reset_state(ctx);

    AggregateRoutes {
        documented: OpenApiRouter::new()
            .nest(
                "/api/oauth-clients",
                oauth_clients_router(oauth_clients_state(ctx)),
            )
            .nest(
                "/auth",
                auth_router(auth_state(ctx)).layer(auth_rate_limit.clone()),
            ),
        plain: Router::new()
            .nest(
                "/api/anchor-domains",
                anchor_domains_router(auth_config_state(ctx)),
            )
            .nest(
                "/api/auth-configs",
                client_auth_configs_router(auth_config_state(ctx)),
            )
            .nest(
                "/api/idp-role-mappings",
                idp_role_mappings_router(auth_config_state(ctx)),
            )
            .nest(
                "/auth",
                oidc_login_router(oidc_login_state(ctx))
                    .layer(portal_oidc_hook)
                    .layer(auth_rate_limit.clone()),
            )
            .nest(
                "/oauth",
                oauth_router(oauth_state(ctx))
                    .layer(axum::middleware::map_response(
                        crate::auth::oauth_api::oauth_errors_no_store,
                    ))
                    .layer(oauth_token_budget)
                    .layer(oauth_rate_limit),
            )
            // `/auth/password-setup/request` spends the reset budgets in its
            // own buckets inside the handler (silent over budget).
            .nest(
                "/auth/password-setup",
                password_setup_router(password_reset.clone()).layer(auth_rate_limit.clone()),
            )
            .nest(
                "/auth/password-reset",
                password_reset_router(password_reset)
                    .layer(portal_reset_hook)
                    .layer(password_reset_email_budget)
                    .layer(password_reset_budget)
                    .layer(auth_rate_limit),
            ),
    }
}

/// `POST /auth/password-reset/request`'s one answer — for a known address,
/// an unknown one, and one over its budget alike (the handler's silent
/// success; `password_reset_email_budget_is_silent` pins the two equal).
fn password_reset_requested_response() -> axum::response::Response {
    Json(serde_json::json!({
        "message": "If an account exists, a reset email has been sent."
    }))
    .into_response()
}

pub fn oauth_clients_state(ctx: &PlatformContext) -> OAuthClientsState {
    let repos = &ctx.repos;
    let repo = &repos.oauth_client_repo;
    let uow = &ctx.unit_of_work;
    OAuthClientsState {
        application_repo: repos.application_repo.clone(),
        oauth_client_repo: repo.clone(),
        portal_apps: Arc::new(crate::portal::repository::PortalAppRepository::new(
            &repos.pool,
        )),
        principal_repo: repos.principal_repo.clone(),
        create_oauth_client_use_case: Arc::new(CreateOAuthClientUseCase::new(
            repo.clone(),
            uow.clone(),
        )),
        update_oauth_client_use_case: Arc::new(UpdateOAuthClientUseCase::new(
            repo.clone(),
            uow.clone(),
        )),
        delete_oauth_client_use_case: Arc::new(DeleteOAuthClientUseCase::new(
            repo.clone(),
            uow.clone(),
        )),
        activate_oauth_client_use_case: Arc::new(ActivateOAuthClientUseCase::new(
            repo.clone(),
            uow.clone(),
        )),
        deactivate_oauth_client_use_case: Arc::new(DeactivateOAuthClientUseCase::new(
            repo.clone(),
            uow.clone(),
        )),
        rotate_oauth_client_secret_use_case: Arc::new(RotateOAuthClientSecretUseCase::new(
            repo.clone(),
            uow.clone(),
        )),
        revoke_oauth_client_previous_secret_use_case: Arc::new(
            RevokeOAuthClientPreviousSecretUseCase::new(repo.clone(), uow.clone()),
        ),
    }
}

/// The state `/auth/login`, `/auth/check-domain` and friends run on (fc-web
/// signs in through it too).
pub fn auth_state(ctx: &PlatformContext) -> AuthState {
    let repos = &ctx.repos;
    AuthState {
        auth_service: ctx.auth.auth.clone(),
        principal_repo: repos.principal_repo.clone(),
        role_repo: repos.role_repo.clone(),
        password_service: ctx.auth.password.clone(),
        refresh_token_repo: repos.refresh_token_repo.clone(),
        email_domain_mapping_repo: repos.edm_repo.clone(),
        identity_provider_repo: repos.idp_repo.clone(),
        login_attempt_repo: repos.login_attempt_repo.clone(),
        backoff_policy: ctx.backoff_policy.clone(),
        session_cookie: SessionCookieConfig::password_login(ctx.config.session_cookie_secure),
        two_factor: Some(ctx.two_factor.clone()),
    }
}

pub fn auth_config_state(ctx: &PlatformContext) -> AuthConfigState {
    let repos = &ctx.repos;
    let uow = &ctx.unit_of_work;
    AuthConfigState {
        anchor_domain_repo: repos.anchor_domain_repo.clone(),
        client_auth_config_repo: repos.client_auth_config_repo.clone(),
        idp_role_mapping_repo: repos.idp_role_mapping_repo.clone(),
        role_repo: repos.role_repo.clone(),
        principal_repo: repos.principal_repo.clone(),
        unit_of_work: uow.clone(),
        encryption_service: EncryptionService::from_env().map(Arc::new),
        create_anchor_domain_use_case: Arc::new(CreateAnchorDomainUseCase::new(
            repos.anchor_domain_repo.clone(),
            uow.clone(),
        )),
        update_anchor_domain_use_case: Arc::new(UpdateAnchorDomainUseCase::new(
            repos.anchor_domain_repo.clone(),
            uow.clone(),
        )),
        delete_anchor_domain_use_case: Arc::new(DeleteAnchorDomainUseCase::new(
            repos.anchor_domain_repo.clone(),
            uow.clone(),
        )),
        create_auth_config_use_case: Arc::new(CreateAuthConfigUseCase::new(
            repos.client_auth_config_repo.clone(),
            uow.clone(),
        )),
        update_auth_config_use_case: Arc::new(UpdateAuthConfigUseCase::new(
            repos.client_auth_config_repo.clone(),
            uow.clone(),
        )),
        delete_auth_config_use_case: Arc::new(DeleteAuthConfigUseCase::new(
            repos.client_auth_config_repo.clone(),
            uow.clone(),
        )),
        create_idp_role_mapping_use_case: Arc::new(CreateIdpRoleMappingUseCase::new(
            repos.idp_role_mapping_repo.clone(),
            uow.clone(),
        )),
        delete_idp_role_mapping_use_case: Arc::new(DeleteIdpRoleMappingUseCase::new(
            repos.idp_role_mapping_repo.clone(),
            uow.clone(),
        )),
    }
}

/// The OIDC login flow's state (also the portal's OIDC login's, which
/// shares its key cache through the context).
pub fn oidc_login_state(ctx: &PlatformContext) -> OidcLoginApiState {
    let repos = &ctx.repos;
    OidcLoginApiState {
        anchor_domain_repo: repos.anchor_domain_repo.clone(),
        identity_provider_repo: repos.idp_repo.clone(),
        email_domain_mapping_repo: repos.edm_repo.clone(),
        oidc_login_state_repo: repos.oidc_login_state_repo.clone(),
        oidc_sync_service: ctx.auth.oidc_sync.clone(),
        auth_service: ctx.auth.auth.clone(),
        jwks_cache: ctx.jwks_cache.clone(),
        unit_of_work: ctx.unit_of_work.clone(),
        oauth_client_repo: repos.oauth_client_repo.clone(),
        external_base_url: ctx.config.oidc_login_external_base_url.clone(),
        session_cookie: ctx.session_cookie.clone(),
        secret_resolver: ctx.secret_resolver.clone(),
        password_setup_hint: Some(crate::auth::oidc_login_api::PasswordSetupHint {
            principal_repo: repos.principal_repo.clone(),
            login_attempt_repo: repos.login_attempt_repo.clone(),
            backoff_policy: Arc::new(crate::auth::login_backoff::BackoffPolicy::from_env()),
        }),
    }
}

pub fn oauth_state(ctx: &PlatformContext) -> OAuthState {
    let repos = &ctx.repos;
    OAuthState {
        service_account_repo: repos.service_account_repo.clone(),
        oauth_client_repo: repos.oauth_client_repo.clone(),
        principal_repo: repos.principal_repo.clone(),
        role_repo: repos.role_repo.clone(),
        auth_service: ctx.auth.auth.clone(),
        auth_code_repo: repos.auth_code_repo.clone(),
        refresh_token_repo: repos.refresh_token_repo.clone(),
        pending_auth_repo: repos.pending_auth_repo.clone(),
        password_service: ctx.auth.password.clone(),
        login_attempt_repo: repos.login_attempt_repo.clone(),
        client_token_rate_limit: IpRateLimiterState::new(
            &RateLimitConfig::oauth_token_per_client_from_env(),
        ),
        rate_limit_store: ctx.config.rate_limit_store.clone(),
        rate_limit_policies: ctx.config.rate_limit_policies.clone(),
        encryption_service: ctx.encryption.clone(),
        portal: Some(ctx.portal.clone()),
    }
}

pub fn password_reset_state(ctx: &PlatformContext) -> PasswordResetApiState {
    let repos = &ctx.repos;
    PasswordResetApiState {
        principal_repo: repos.principal_repo.clone(),
        password_service: ctx.auth.password.clone(),
        unit_of_work: ctx.unit_of_work.clone(),
        emailer: ctx.password_reset_emailer.clone(),
        password_reset_repo: repos.password_reset_repo.clone(),
        reset_password_use_case: Arc::new(crate::principal::operations::ResetPasswordUseCase::new(
            repos.principal_repo.clone(),
            ctx.auth.password.clone(),
            ctx.unit_of_work.clone(),
        )),
        two_factor: Some(ctx.two_factor.clone()),
        refresh_token_repo: repos.refresh_token_repo.clone(),
        rate_limit_store: ctx.config.rate_limit_store.clone(),
        rate_limit_policies: ctx.config.rate_limit_policies.clone(),
    }
}

/// Create OAuth clients router
pub fn oauth_clients_router(state: OAuthClientsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(
            crate::auth::oauth_clients_api::create_oauth_client,
            crate::auth::oauth_clients_api::list_oauth_clients
        ))
        .routes(routes!(
            crate::auth::oauth_clients_api::get_oauth_client,
            crate::auth::oauth_clients_api::update_oauth_client,
            crate::auth::oauth_clients_api::delete_oauth_client
        ))
        .routes(routes!(
            crate::auth::oauth_clients_api::get_oauth_client_by_client_id
        ))
        .routes(routes!(
            crate::auth::oauth_clients_api::activate_oauth_client
        ))
        .routes(routes!(
            crate::auth::oauth_clients_api::deactivate_oauth_client
        ))
        .routes(routes!(
            crate::auth::oauth_clients_api::regenerate_oauth_client_secret
        ))
        .routes(routes!(
            crate::auth::oauth_clients_api::rotate_oauth_client_secret
        ))
        .routes(routes!(
            crate::auth::oauth_clients_api::revoke_oauth_client_previous_secret
        ))
        .with_state(state)
}

/// Create the auth router
pub fn auth_router(state: AuthState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(crate::auth::auth_api::login))
        .routes(routes!(crate::auth::auth_api::logout))
        .routes(routes!(crate::auth::auth_api::check_domain))
        .routes(routes!(crate::auth::auth_api::get_current_user))
        .routes(routes!(crate::auth::auth_api::refresh_token))
        .with_state(state)
}

/// Create anchor domains router
pub fn anchor_domains_router(state: AuthConfigState) -> Router {
    Router::new()
        .route(
            "/",
            post(crate::auth::config_api::create_anchor_domain)
                .get(crate::auth::config_api::list_anchor_domains),
        )
        .route(
            "/check/{domain}",
            get(crate::auth::config_api::check_anchor_domain),
        )
        .route(
            "/{id}",
            get(crate::auth::config_api::get_anchor_domain)
                .put(crate::auth::config_api::update_anchor_domain)
                .delete(crate::auth::config_api::delete_anchor_domain),
        )
        .with_state(state)
}

/// Create client auth configs router
pub fn client_auth_configs_router(state: AuthConfigState) -> Router {
    Router::new()
        .route(
            "/",
            post(crate::auth::config_api::create_client_auth_config)
                .get(crate::auth::config_api::list_client_auth_configs),
        )
        .route(
            "/internal",
            post(crate::auth::config_api::create_internal_auth_config),
        )
        .route(
            "/oidc",
            post(crate::auth::config_api::create_oidc_auth_config),
        )
        .route(
            "/by-domain/{domain}",
            get(crate::auth::config_api::get_by_domain),
        )
        .route(
            "/{id}",
            get(crate::auth::config_api::get_client_auth_config)
                .put(crate::auth::config_api::update_client_auth_config)
                .delete(crate::auth::config_api::delete_client_auth_config),
        )
        .route(
            "/{id}/config-type",
            axum::routing::put(crate::auth::config_api::update_config_type),
        )
        .route(
            "/{id}/oidc",
            axum::routing::put(crate::auth::config_api::update_oidc_config),
        )
        .route(
            "/{id}/client-binding",
            axum::routing::put(crate::auth::config_api::update_client_binding),
        )
        .route(
            "/{id}/additional-clients",
            axum::routing::put(crate::auth::config_api::update_additional_clients),
        )
        .route(
            "/{id}/granted-clients",
            axum::routing::put(crate::auth::config_api::update_granted_clients),
        )
        .with_state(state)
}

/// Create IDP role mappings router
pub fn idp_role_mappings_router(state: AuthConfigState) -> Router {
    Router::new()
        .route(
            "/",
            post(crate::auth::config_api::create_idp_role_mapping)
                .get(crate::auth::config_api::list_idp_role_mappings),
        )
        .route(
            "/{id}",
            delete(crate::auth::config_api::delete_idp_role_mapping),
        )
        .with_state(state)
}

/// Create the OIDC login router
pub fn oidc_login_router(state: OidcLoginApiState) -> Router {
    Router::new()
        .route(
            "/check-domain",
            post(crate::auth::oidc_login_api::check_domain),
        )
        .route("/oidc/login", get(crate::auth::oidc_login_api::oidc_login))
        .route(
            "/oidc/callback",
            get(crate::auth::oidc_login_api::oidc_callback),
        )
        .route(
            "/oidc/interaction/{uid}",
            get(crate::auth::oidc_login_api::get_interaction),
        )
        .route(
            "/oidc/interaction/{uid}/login",
            post(crate::auth::oidc_login_api::post_interaction_login),
        )
        .route(
            "/oidc/session/end",
            get(crate::auth::oidc_login_api::session_end),
        )
        .with_state(state)
}

/// Create OAuth router
pub fn oauth_router(state: OAuthState) -> Router {
    Router::new()
        .route("/authorize", get(crate::auth::oauth_api::authorize))
        .route("/token", post(crate::auth::oauth_api::token))
        .route(
            "/userinfo",
            get(crate::auth::oauth_api::userinfo).post(crate::auth::oauth_api::userinfo),
        )
        .route("/introspect", post(crate::auth::oauth_api::introspect))
        .route("/revoke", post(crate::auth::oauth_api::revoke))
        .with_state(state)
}

/// `/auth/password-setup/*`.
pub fn password_setup_router(state: PasswordResetApiState) -> Router {
    Router::new()
        .route(
            "/request",
            post(crate::auth::password_reset_api::request_password_setup),
        )
        .with_state(state)
}

pub fn password_reset_router(state: PasswordResetApiState) -> Router {
    Router::new()
        .route(
            "/request",
            post(crate::auth::password_reset_api::request_reset),
        )
        .route(
            "/validate",
            get(crate::auth::password_reset_api::validate_token),
        )
        .route(
            "/confirm",
            post(crate::auth::password_reset_api::confirm_reset),
        )
        .with_state(state)
}
