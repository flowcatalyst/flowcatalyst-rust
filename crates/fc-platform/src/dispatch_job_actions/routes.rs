//! Dispatch job actions (Go's requeue, cancel, complete and sign), on both
//! tiers, at their full paths.

#![allow(
    clippy::absolute_paths,
    reason = "handler paths stay fully qualified: the route-auth scanner reads them"
)]

use std::sync::Arc;

use axum::Router;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::api::DispatchJobActionsState;
use super::operations::{RequeueDispatchJobsUseCase, SettleDispatchJobUseCase};
use super::repository::DispatchJobActionsRepository;
use crate::dispatch_job::delivery_credentials::DeliveryCredentials;
use crate::shared::platform_context::{AggregateRoutes, PlatformContext};

pub fn routes(ctx: &PlatformContext) -> AggregateRoutes {
    AggregateRoutes {
        documented: OpenApiRouter::new()
            .merge(dispatch_job_actions_router(dispatch_job_actions_state(ctx))),
        plain: Router::new(),
    }
}

pub fn dispatch_job_actions_state(ctx: &PlatformContext) -> DispatchJobActionsState {
    let repos = &ctx.repos;
    let repo = Arc::new(DispatchJobActionsRepository::new(&repos.pool));
    // The platform's shared resolver: it carries the secret resolver, so
    // `aws-sm://` webhook credentials resolve here exactly as on delivery.
    let outbound = ctx.outbound_credentials.clone();
    DispatchJobActionsState {
        repo: repo.clone(),
        dispatch_job_repo: repos.dispatch_job_repo.clone(),
        client_repo: repos.client_repo.clone(),
        credentials: Arc::new(DeliveryCredentials::new(
            repos.subscription_repo.clone(),
            repos.connection_repo.clone(),
            repos.application_repo.clone(),
            outbound,
        )),
        requeue_use_case: Arc::new(RequeueDispatchJobsUseCase::new(
            repo.clone(),
            ctx.unit_of_work.clone(),
        )),
        settle_use_case: Arc::new(SettleDispatchJobUseCase::new(
            repo,
            ctx.unit_of_work.clone(),
        )),
    }
}

/// Full-path router; merged at the root.
pub fn dispatch_job_actions_router(state: DispatchJobActionsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(
            crate::dispatch_job_actions::api::api_requeue_dispatch_jobs
        ))
        .routes(routes!(
            crate::dispatch_job_actions::api::api_cancel_dispatch_job
        ))
        .routes(routes!(
            crate::dispatch_job_actions::api::api_complete_dispatch_job
        ))
        .routes(routes!(
            crate::dispatch_job_actions::api::api_sign_dispatch_job
        ))
        .routes(routes!(
            crate::dispatch_job_actions::api::bff_requeue_dispatch_jobs
        ))
        .routes(routes!(
            crate::dispatch_job_actions::api::bff_cancel_dispatch_job
        ))
        .routes(routes!(
            crate::dispatch_job_actions::api::bff_complete_dispatch_job
        ))
        .routes(routes!(
            crate::dispatch_job_actions::api::bff_sign_dispatch_job
        ))
        .with_state(state)
}
