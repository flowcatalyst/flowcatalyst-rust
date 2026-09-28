//! Audit log routes: `/api/audit-logs` (reads), the SDK ingest
//! `POST /api/audit-logs/batch` (`shared::sdk_audit_batch_api`), and the
//! temporary `/bff/audit-logs` sweep (docs/spec/audit-redaction.md, Java
//! repo).

use std::sync::Arc;

use axum::routing::post;
use axum::Router;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::api::AuditLogsState;
use super::operations::RedactExistingAuditLogsUseCase;
use crate::shared::bff_audit_logs_api::BffAuditLogsState;
use crate::shared::platform_context::{AggregateRoutes, PlatformContext};
use crate::shared::sdk_audit_batch_api::SdkAuditBatchState;

pub fn routes(ctx: &PlatformContext) -> AggregateRoutes {
    let repos = &ctx.repos;
    AggregateRoutes {
        documented: OpenApiRouter::new().nest(
            "/api/audit-logs",
            audit_logs_router(AuditLogsState {
                audit_log_repo: repos.audit_log_repo.clone(),
                principal_repo: repos.principal_repo.clone(),
            }),
        ),
        plain: Router::new()
            .nest(
                "/bff/audit-logs",
                bff_audit_logs_router(BffAuditLogsState {
                    redact_existing_use_case: Arc::new(RedactExistingAuditLogsUseCase::new(
                        repos.audit_log_repo.clone(),
                        ctx.unit_of_work.clone(),
                    )),
                })
                .into(),
            )
            .nest(
                "/api/audit-logs",
                sdk_audit_batch_router(SdkAuditBatchState {
                    audit_log_repo: repos.audit_log_repo.clone(),
                    application_repo: repos.application_repo.clone(),
                    client_repo: repos.client_repo.clone(),
                }),
            ),
    }
}

/// Create audit logs router
pub fn audit_logs_router(state: AuditLogsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(crate::audit::api::list_audit_logs))
        .routes(routes!(crate::audit::api::get_entity_types))
        .routes(routes!(crate::audit::api::get_operations))
        .routes(routes!(crate::audit::api::get_application_ids))
        .routes(routes!(crate::audit::api::get_client_ids))
        .routes(routes!(crate::audit::api::get_recent_audit_logs))
        .routes(routes!(crate::audit::api::get_audit_log))
        .routes(routes!(crate::audit::api::get_entity_audit_logs))
        .routes(routes!(crate::audit::api::get_principal_audit_logs))
        .with_state(state)
}

/// Create the BFF audit-logs router (mounted at `/bff/audit-logs`).
pub fn bff_audit_logs_router(state: BffAuditLogsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(
            crate::shared::bff_audit_logs_api::redact_existing_audit_logs
        ))
        .with_state(state)
}

pub fn sdk_audit_batch_router(state: SdkAuditBatchState) -> Router {
    Router::new()
        .route(
            "/batch",
            post(crate::shared::sdk_audit_batch_api::batch_audit_logs),
        )
        .with_state(state)
}
