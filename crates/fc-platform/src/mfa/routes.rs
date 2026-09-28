//! Two-factor routes: the admin reset under `/api/principals`,
//! `/api/reset-approvals`, and under `/auth` the token-gated sign-in steps
//! (behind the `/auth` per-IP limit, like `/auth/login`), the session-gated
//! self-service routes and the account routes.

use std::sync::Arc;

use axum::routing::{delete, get, post};
use axum::Router;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::account_api::AccountState;
use super::login_api::TwoFactorLogin;
use super::reset_approval_api::ResetApprovalsState;
use crate::shared::platform_context::{AggregateRoutes, PlatformContext};
use crate::shared::rate_limit_middleware::rate_limit_per_ip;

pub fn routes(ctx: &PlatformContext) -> AggregateRoutes {
    let auth_rate_limit =
        axum::middleware::from_fn_with_state(ctx.auth_ip_limit.clone(), rate_limit_per_ip);
    AggregateRoutes {
        documented: OpenApiRouter::new()
            .nest(
                "/api/principals",
                two_factor_admin_router(ctx.two_factor.clone()),
            )
            .nest(
                "/api/reset-approvals",
                reset_approvals_router(reset_approvals_state(ctx)),
            ),
        // The step-token routes are public and rate-limited like
        // `/auth/login`; the self-service ones need a session.
        plain: Router::new()
            .nest(
                "/auth",
                two_factor_login_router(ctx.two_factor.clone()).layer(auth_rate_limit),
            )
            .nest(
                "/auth",
                two_factor_self_service_router(ctx.two_factor.clone()),
            )
            .nest("/auth", account_router(account_state(ctx))),
    }
}

pub fn reset_approvals_state(ctx: &PlatformContext) -> ResetApprovalsState {
    ResetApprovalsState {
        approvals: Arc::new(super::reset_approval::ResetApprovalRepository::new(
            &ctx.repos.pool,
        )),
        principal_repo: ctx.repos.principal_repo.clone(),
        emailer: ctx.password_reset_emailer.clone(),
    }
}

pub fn account_state(ctx: &PlatformContext) -> Arc<AccountState> {
    Arc::new(AccountState {
        two_factor: ctx.two_factor.clone(),
        password_service: ctx.auth.password.clone(),
        refresh_token_repo: ctx.repos.refresh_token_repo.clone(),
    })
}

/// Nested at `/api/principals`.
pub fn two_factor_admin_router(state: Arc<TwoFactorLogin>) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(crate::mfa::admin_api::reset_two_factor))
        .with_state(state)
}

/// Nested at `/api/reset-approvals`.
pub fn reset_approvals_router(state: ResetApprovalsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(
            crate::mfa::reset_approval_api::list_reset_approvals
        ))
        .routes(routes!(
            crate::mfa::reset_approval_api::approve_reset_approval
        ))
        .routes(routes!(crate::mfa::reset_approval_api::deny_reset_approval))
        .with_state(state)
}

/// The token-gated `/auth/2fa/*` routes, nested under `/auth`.
pub fn two_factor_login_router(state: Arc<TwoFactorLogin>) -> Router {
    Router::new()
        .route("/2fa/verify", post(crate::mfa::login_api::verify))
        .route(
            "/2fa/challenge/email",
            post(crate::mfa::login_api::challenge_email),
        )
        .route(
            "/2fa/enroll/totp/begin",
            post(crate::mfa::login_api::enroll_totp_begin),
        )
        .route(
            "/2fa/enroll/totp/confirm",
            post(crate::mfa::login_api::enroll_totp_confirm),
        )
        .route(
            "/2fa/enroll/email/begin",
            post(crate::mfa::login_api::enroll_email_begin),
        )
        .route(
            "/2fa/enroll/email/confirm",
            post(crate::mfa::login_api::enroll_email_confirm),
        )
        .with_state(state)
}

/// The session-gated `/auth/2fa/*` routes, nested under `/auth`.
pub fn two_factor_self_service_router(state: Arc<TwoFactorLogin>) -> Router {
    Router::new()
        .route("/2fa/status", get(crate::mfa::self_service_api::status))
        .route(
            "/2fa/methods/totp/begin",
            post(crate::mfa::self_service_api::totp_begin),
        )
        .route(
            "/2fa/methods/totp/confirm",
            post(crate::mfa::self_service_api::totp_confirm),
        )
        .route(
            "/2fa/methods/email/begin",
            post(crate::mfa::self_service_api::email_begin),
        )
        .route(
            "/2fa/methods/email/confirm",
            post(crate::mfa::self_service_api::email_confirm),
        )
        .route(
            "/2fa/methods/{method}",
            delete(crate::mfa::self_service_api::remove_method),
        )
        .route(
            "/2fa/recovery-codes/regenerate",
            post(crate::mfa::self_service_api::regenerate_recovery_codes),
        )
        .route(
            "/2fa/trusted-devices",
            get(crate::mfa::self_service_api::list_trusted_devices),
        )
        .route(
            "/2fa/trusted-devices/{id}",
            delete(crate::mfa::self_service_api::revoke_trusted_device),
        )
        .with_state(state)
}

/// The session-gated account routes, nested under `/auth`.
pub fn account_router(state: Arc<AccountState>) -> Router {
    Router::new()
        .route(
            "/change-password",
            post(crate::mfa::account_api::change_password),
        )
        .route(
            "/change-password/send-email-code",
            post(crate::mfa::account_api::send_email_code),
        )
        .route(
            "/login-history",
            get(crate::mfa::account_api::login_history),
        )
        .with_state(state)
}
