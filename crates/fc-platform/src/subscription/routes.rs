//! Subscription routes: `/api/subscriptions`.

use std::sync::Arc;

use axum::Router;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::api::SubscriptionsState;
use super::operations::{
    CreateSubscriptionUseCase, DeleteSubscriptionUseCase, PauseSubscriptionUseCase,
    ResumeSubscriptionUseCase, UpdateSubscriptionUseCase,
};
use crate::shared::platform_context::{AggregateRoutes, PlatformContext};

pub fn routes(ctx: &PlatformContext) -> AggregateRoutes {
    AggregateRoutes {
        documented: OpenApiRouter::new().nest(
            "/api/subscriptions",
            subscriptions_router(subscriptions_state(ctx)),
        ),
        plain: Router::new(),
    }
}

pub fn subscriptions_state(ctx: &PlatformContext) -> SubscriptionsState {
    let repos = &ctx.repos;
    let uow = &ctx.unit_of_work;
    SubscriptionsState {
        subscription_repo: repos.subscription_repo.clone(),
        create_use_case: Arc::new(CreateSubscriptionUseCase::new(
            repos.subscription_repo.clone(),
            repos.service_account_repo.clone(),
            repos.connection_repo.clone(),
            uow.clone(),
        )),
        update_use_case: Arc::new(UpdateSubscriptionUseCase::new(
            repos.subscription_repo.clone(),
            repos.service_account_repo.clone(),
            repos.connection_repo.clone(),
            uow.clone(),
        )),
        delete_use_case: Arc::new(DeleteSubscriptionUseCase::new(
            repos.subscription_repo.clone(),
            uow.clone(),
        )),
        pause_use_case: Arc::new(PauseSubscriptionUseCase::new(
            repos.subscription_repo.clone(),
            uow.clone(),
        )),
        resume_use_case: Arc::new(ResumeSubscriptionUseCase::new(
            repos.subscription_repo.clone(),
            uow.clone(),
        )),
    }
}

/// Create subscriptions router
pub fn subscriptions_router(state: SubscriptionsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(
            crate::subscription::api::create_subscription,
            crate::subscription::api::list_subscriptions
        ))
        .routes(routes!(
            crate::subscription::api::get_subscription,
            crate::subscription::api::update_subscription,
            crate::subscription::api::delete_subscription
        ))
        .routes(routes!(crate::subscription::api::pause_subscription))
        .routes(routes!(crate::subscription::api::resume_subscription))
        .with_state(state)
}
