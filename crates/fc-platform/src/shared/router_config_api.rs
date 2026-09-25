//! `GET /api/dispatch/router-config` (Go `internal/platform/dispatch/api.go:31`,
//! `document.go`): the message router's pools and queues.
//!
//! - pools: every `msg_dispatch_pools` row (any status), code composed per
//!   tenant (`{clientIdentifier|platform}-{code}`), `rateLimitPerMinute`
//!   only when set and positive;
//! - queues: one DEFAULT queue per tenant (`platform` first, then pool
//!   tenants, then ACTIVE subscriptions' tenants, first-seen order), plus a
//!   HIGH_PRIORITY queue for a tenant with an ACTIVE subscription on it. A
//!   tenant whose name does not fit SQS's limit is left out with a warning.
//!   `connections` and `visibilityTimeout` are always 0: the router applies
//!   its own defaults.
//!
//! Gate: anchor and `platform:messaging:dispatch-pool:view` (the `router`
//! role holds exactly that).

use axum::{extract::State, Json};
use serde::Serialize;
use std::sync::Arc;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::dispatch_pool::router_config_repository::{
    RouterConfigRepository, RouterPoolRow, RouterSubscriptionRow,
};
use crate::shared::authorization_service::checks;
use crate::shared::dispatch_queue::{
    compose_name, compose_pool_code, tenant_for, NameError, Priority, QueueSettings,
};
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;

#[derive(Clone)]
pub struct RouterConfigState {
    pub repo: Arc<RouterConfigRepository>,
    pub settings: Arc<QueueSettings>,
}

/// Go `common.RouterConfig`.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RouterConfigDocument {
    pub processing_pools: Vec<RouterPoolConfig>,
    pub queues: Vec<RouterQueueConfig>,
}

/// Go `common.PoolConfig`.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RouterPoolConfig {
    pub code: String,
    pub concurrency: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit_per_minute: Option<u32>,
}

/// Go `common.QueueConfig`.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RouterQueueConfig {
    pub queue_name: String,
    pub queue_uri: String,
    pub connections: u32,
    pub visibility_timeout: u32,
}

/// Go `DocumentBuilder.Build`, from the two reads.
pub fn build_document(
    settings: &QueueSettings,
    pools: &[RouterPoolRow],
    subscriptions: &[RouterSubscriptionRow],
) -> RouterConfigDocument {
    let processing_pools = pools
        .iter()
        .map(|p| RouterPoolConfig {
            code: compose_pool_code(&p.code, p.client_identifier.as_deref()),
            concurrency: p.concurrency.max(0) as u32,
            rate_limit_per_minute: p.rate_limit.filter(|r| *r > 0).map(|r| r as u32),
        })
        .collect();

    // Tenants in first-seen order; which of them use HIGH_PRIORITY.
    let mut tenants: Vec<String> = vec![crate::shared::dispatch_queue::TENANT_PLATFORM.to_string()];
    let mut high: Vec<String> = Vec::new();
    let add = |t: &str, tenants: &mut Vec<String>| {
        if !tenants.iter().any(|x| x == t) {
            tenants.push(t.to_string());
        }
    };
    for p in pools {
        add(tenant_for(p.client_identifier.as_deref()), &mut tenants);
    }
    for s in subscriptions {
        let tenant = tenant_for(s.client_identifier.as_deref());
        add(tenant, &mut tenants);
        if Priority::for_publishing(s.queue.as_deref()) == Priority::HighPriority
            && !high.iter().any(|h| h == tenant)
        {
            high.push(tenant.to_string());
        }
    }

    let mut queues = Vec::new();
    for tenant in &tenants {
        let mut names = vec![compose_name(
            &settings.prefix,
            tenant,
            Priority::Default,
            settings.sqs,
        )];
        if high.contains(tenant) {
            names.push(compose_name(
                &settings.prefix,
                tenant,
                Priority::HighPriority,
                settings.sqs,
            ));
        }
        let names: Result<Vec<String>, NameError> = names.into_iter().collect();
        match names {
            Ok(names) => queues.extend(names.into_iter().map(|name| RouterQueueConfig {
                queue_uri: settings.queue_uri_for(&name),
                queue_name: name,
                connections: 0,
                visibility_timeout: 0,
            })),
            Err(e @ NameError::TooLong { .. }) => tracing::warn!(
                category = "CONFIGURATION",
                tenant = %tenant,
                error = %e,
                "dispatch queue name too long for SQS; omitting this tenant's queues from the router config"
            ),
            Err(e) => tracing::warn!(
                category = "CONFIGURATION",
                tenant = %tenant,
                error = %e,
                "dispatch queue name could not be composed; omitting this tenant's queues from the router config"
            ),
        }
    }

    RouterConfigDocument {
        processing_pools,
        queues,
    }
}

/// The message router's configuration document (Go `getRouterConfig`).
#[utoipa::path(
    get,
    path = "/api/dispatch/router-config",
    tag = "dispatch",
    operation_id = "getRouterConfig",
    responses(
        (status = 200, description = "Pools and queues", body = RouterConfigDocument),
        (status = 403, description = "Not an anchor holding platform:messaging:dispatch-pool:view")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_router_config(
    State(state): State<RouterConfigState>,
    auth: Authenticated,
) -> Result<Json<RouterConfigDocument>, PlatformError> {
    checks::require_anchor_scope(&auth.0)?;
    checks::require_permission(&auth.0, crate::permissions::admin::DISPATCH_POOL_READ)?;
    let (pools, subscriptions) =
        tokio::try_join!(state.repo.pools(), state.repo.active_subscriptions())?;
    Ok(Json(build_document(
        &state.settings,
        &pools,
        &subscriptions,
    )))
}

pub fn router_config_router(state: RouterConfigState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(get_router_config))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool(code: &str, client: Option<&str>, conc: i32, rate: Option<i32>) -> RouterPoolRow {
        RouterPoolRow {
            code: code.into(),
            client_identifier: client.map(Into::into),
            concurrency: conc,
            rate_limit: rate,
        }
    }

    fn sub(client: Option<&str>, queue: Option<&str>) -> RouterSubscriptionRow {
        RouterSubscriptionRow {
            client_identifier: client.map(Into::into),
            queue: queue.map(Into::into),
        }
    }

    #[test]
    fn the_document_lists_pools_and_per_tenant_queues() {
        let settings = QueueSettings::resolve(
            "SQS",
            "https://sqs.eu-west-1.amazonaws.com/123/x",
            "",
            "FC",
            "",
        )
        .unwrap();
        let doc = build_document(
            &settings,
            &[
                pool("DEFAULT-POOL", None, 10, Some(0)),
                pool("fast", Some("acme"), -1, Some(60)),
            ],
            &[
                sub(Some("beta"), Some("high_priority")),
                sub(Some("acme"), None),
            ],
        );
        assert_eq!(
            doc.processing_pools,
            vec![
                RouterPoolConfig {
                    code: "platform-DEFAULT-POOL".into(),
                    concurrency: 10,
                    rate_limit_per_minute: None
                },
                RouterPoolConfig {
                    code: "acme-fast".into(),
                    concurrency: 0,
                    rate_limit_per_minute: Some(60)
                },
            ]
        );
        let names: Vec<&str> = doc.queues.iter().map(|q| q.queue_name.as_str()).collect();
        assert_eq!(
            names,
            [
                "FC-platform-DEFAULT.fifo",
                "FC-acme-DEFAULT.fifo",
                "FC-beta-DEFAULT.fifo",
                "FC-beta-HIGH_PRIORITY.fifo"
            ]
        );
        assert_eq!(
            doc.queues[0].queue_uri,
            "https://sqs.eu-west-1.amazonaws.com/123/FC-platform-DEFAULT.fifo"
        );
        let json = serde_json::to_value(&doc).unwrap();
        assert_eq!(json["queues"][0]["connections"], 0);
        assert_eq!(json["queues"][0]["visibilityTimeout"], 0);
        assert!(json["processingPools"][0]
            .get("rateLimitPerMinute")
            .is_none());
    }

    #[test]
    fn a_tenant_too_long_for_sqs_is_left_out() {
        let settings = QueueSettings::resolve(
            "SQS",
            "https://sqs.eu-west-1.amazonaws.com/123/x",
            "",
            "FC",
            "",
        )
        .unwrap();
        let long = "t".repeat(90);
        let doc = build_document(&settings, &[pool("p", Some(&long), 1, None)], &[]);
        assert_eq!(doc.queues.len(), 1);
        assert_eq!(doc.processing_pools.len(), 1);
    }
}
