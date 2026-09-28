//! Passkey routes, nested under `/auth` behind the shared `/auth` per-IP
//! limit.

#![allow(
    clippy::absolute_paths,
    reason = "handler paths stay fully qualified: the route-auth scanner reads them"
)]

use std::sync::Arc;

use axum::Router;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::api::WebauthnApiState;
use super::repository::WebauthnCredentialRepository;
use super::{WebauthnCeremonyRepository, WebauthnService};
use crate::shared::platform_context::{AggregateRoutes, PlatformContext};
use crate::shared::rate_limit_middleware::rate_limit_per_ip;

pub fn routes(ctx: &PlatformContext) -> AggregateRoutes {
    let auth_rate_limit =
        axum::middleware::from_fn_with_state(ctx.auth_ip_limit.clone(), rate_limit_per_ip);
    AggregateRoutes {
        documented: OpenApiRouter::new().nest(
            "/auth",
            webauthn_router(webauthn_state(ctx)).layer(auth_rate_limit),
        ),
        plain: Router::new(),
    }
}

pub fn webauthn_state(ctx: &PlatformContext) -> WebauthnApiState {
    let repos = &ctx.repos;
    WebauthnApiState {
        credential_repo: Arc::new(WebauthnCredentialRepository::new(&repos.pool)),
        ceremony_repo: Arc::new(WebauthnCeremonyRepository::new(&repos.pool)),
        principal_repo: repos.principal_repo.clone(),
        email_domain_mapping_repo: repos.edm_repo.clone(),
        login_attempt_repo: repos.login_attempt_repo.clone(),
        webauthn_service: Arc::new(
            WebauthnService::from_env()
                .expect("FC_WEBAUTHN_RP_ID/FC_WEBAUTHN_ORIGINS misconfigured"),
        ),
        auth_service: ctx.auth.auth.clone(),
        backoff_policy: ctx.backoff_policy.clone(),
        unit_of_work: ctx.unit_of_work.clone(),
        session_cookie: ctx.session_cookie.clone(),
    }
}

pub fn webauthn_router(state: WebauthnApiState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(crate::webauthn::api::register_begin))
        .routes(routes!(crate::webauthn::api::register_complete))
        .routes(routes!(crate::webauthn::api::authenticate_begin))
        .routes(routes!(crate::webauthn::api::authenticate_complete))
        .routes(routes!(crate::webauthn::api::list_credentials))
        .routes(routes!(crate::webauthn::api::delete_credential))
        .with_state(state)
}
