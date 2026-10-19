//! `GET /api/dispatch/router-config` (Go `internal/platform/dispatch/api.go:31`,
//! `document.go`): the message router's pools and queues.
//!
//! - pools: every `msg_dispatch_pools` row (any status), code composed per
//!   tenant (`{clientIdentifier|platform}-{code}`), `rateLimitPerMinute`
//!   only when set and positive;
//! - queues: a DEFAULT and a HIGH_PRIORITY queue per tenant (`platform`
//!   first, then pool tenants, then ACTIVE subscriptions' tenants, then
//!   every client's identifier, first-seen order). A tenant whose name does
//!   not fit SQS's limit is left out with a warning. `connections` and
//!   `visibilityTimeout` are always 0: the router applies its own defaults.
//!
//! **Deliberate deviation from Go** (see [`build_document`]): Go lists only
//! the tenants of pools and subscriptions, and HIGH_PRIORITY only where a
//! subscription asks for it.
//!
//! Gate: anchor and `platform:messaging:dispatch-pool:view` (the `router`
//! role holds exactly that).

use axum::{
    extract::State,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use utoipa::ToSchema;

use crate::dispatch_pool::router_config_repository::{
    RouterConfigRepository, RouterPoolRow, RouterSubscriptionRow,
};
use crate::permissions;
use crate::shared::authorization_service::checks;
use crate::shared::dispatch_queue;
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
#[schema(as = RouterConfig)]
pub struct RouterConfigDocument {
    pub processing_pools: Vec<RouterPoolConfig>,
    pub queues: Vec<RouterQueueConfig>,
}

/// Go `common.PoolConfig`.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = PoolConfig)]
pub struct RouterPoolConfig {
    pub code: String,
    pub concurrency: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit_per_minute: Option<u32>,
}

/// Go `common.QueueConfig`.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = QueueConfig)]
pub struct RouterQueueConfig {
    pub queue_name: String,
    pub queue_uri: String,
    pub connections: u32,
    pub visibility_timeout: u32,
}

/// Go `DocumentBuilder.Build`, from the reads, with one deliberate
/// deviation: the queue list covers **every queue the scheduler can
/// publish to** — every client's tenant, and both priorities for every
/// tenant.
///
/// Why: the scheduler publishes a job to its client's tenant queue
/// (`{prefix}-{client identifier}-{priority}`) and takes the priority from
/// the job's own `queue` column before its subscription's. Go's document
/// lists a tenant only through a pool's or subscription's
/// `client_identifier` (which the API never sets) and HIGH_PRIORITY only
/// for a subscription asking for it, so a client-scoped job, or a job
/// claiming HIGH_PRIORITY itself, lands on a queue no router consumes and
/// sits QUEUED forever (found by the delivery harness). The document's
/// shape is unchanged; only more queues are listed.
pub fn build_document(
    settings: &QueueSettings,
    pools: &[RouterPoolRow],
    subscriptions: &[RouterSubscriptionRow],
    client_identifiers: &[String],
) -> RouterConfigDocument {
    let processing_pools = pools
        .iter()
        .map(|p| RouterPoolConfig {
            code: compose_pool_code(&p.code, p.client_identifier.as_deref()),
            concurrency: p.concurrency.max(0) as u32,
            rate_limit_per_minute: p.rate_limit.filter(|r| *r > 0).map(|r| r as u32),
        })
        .collect();

    // Tenants in first-seen order: Go's, then every client's.
    let mut tenants: Vec<String> = vec![dispatch_queue::TENANT_PLATFORM.to_string()];
    let add = |t: &str, tenants: &mut Vec<String>| {
        if !tenants.iter().any(|x| x == t) {
            tenants.push(t.to_string());
        }
    };
    for p in pools {
        add(tenant_for(p.client_identifier.as_deref()), &mut tenants);
    }
    for s in subscriptions {
        add(tenant_for(s.client_identifier.as_deref()), &mut tenants);
    }
    for c in client_identifiers {
        add(tenant_for(Some(c)), &mut tenants);
    }

    let mut queues = Vec::new();
    for tenant in &tenants {
        let names: Result<Vec<String>, NameError> = [Priority::Default, Priority::HighPriority]
            .into_iter()
            .map(|p| compose_name(&settings.prefix, tenant, p, settings.sqs))
            .collect();
        match names {
            Ok(names) => queues.extend(names.into_iter().map(|name| RouterQueueConfig {
                queue_uri: settings.queue_uri_for(&name),
                queue_name: name,
                connections: 0,
                visibility_timeout: 0,
            })),
            Err(e @ NameError::NameTooLong { .. }) => tracing::warn!(
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

/// A strong validator for the rendered document: a hash of its JSON, which is
/// deterministic (pools are ordered by code, tenants in a fixed order).
fn etag_of(document: &RouterConfigDocument) -> Result<String, serde_json::Error> {
    let json = serde_json::to_vec(document)?;
    let sum = Sha256::digest(&json);
    Ok(format!("\"{}\"", hex::encode(&sum[..16])))
}

/// Whether an `If-None-Match` value names `etag`: a list, weak validators
/// (`W/"..."`) and `*` understood (Go `etagMatches`).
fn etag_matches(if_none_match: &str, etag: &str) -> bool {
    if_none_match.split(',').any(|v| {
        let v = v.trim();
        let v = v.strip_prefix("W/").unwrap_or(v);
        v == "*" || v == etag
    })
}

/// The message router's configuration document (Go `getRouterConfig`). The
/// answer carries an `ETag`; a router that sends it back as `If-None-Match`
/// gets a `304` while nothing changed, so it can poll every few seconds (Go
/// F29). The document is built either way: what the validator saves is the
/// transfer and the router's parse and compare.
#[utoipa::path(
    get,
    path = "/api/dispatch/router-config",
    tag = "dispatch",
    operation_id = "getRouterConfig",
    params(
        ("If-None-Match" = Option<String>, Header, description = "An ETag from an earlier answer; 304 when the document is unchanged")
    ),
    responses(
        (status = 200, description = "Pools and queues", body = RouterConfigDocument,
            headers(("ETag" = String, description = "Strong validator of the document"))),
        (status = 304, description = "Unchanged since the ETag in If-None-Match"),
        (status = 403, description = "Not an anchor holding platform:messaging:dispatch-pool:view")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_router_config(
    State(state): State<RouterConfigState>,
    auth: Authenticated,
    headers: HeaderMap,
) -> Result<Response, PlatformError> {
    checks::require_anchor_scope(&auth.0)?;
    checks::require_permission(&auth.0, permissions::admin::DISPATCH_POOL_READ)?;
    let (pools, subscriptions, clients) = tokio::try_join!(
        state.repo.pools(),
        state.repo.active_subscriptions(),
        state.repo.client_identifiers()
    )?;
    let document = build_document(&state.settings, &pools, &subscriptions, &clients);
    let etag = etag_of(&document).map_err(|e| PlatformError::Internal {
        message: format!("rendering the router config failed: {e}"),
    })?;
    let etag_header = HeaderValue::from_str(&etag).map_err(|e| PlatformError::Internal {
        message: format!("router config ETag is not a header value: {e}"),
    })?;
    let unchanged = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| etag_matches(v, &etag));
    if unchanged {
        return Ok((StatusCode::NOT_MODIFIED, [(header::ETAG, etag_header)]).into_response());
    }
    Ok(([(header::ETAG, etag_header)], Json(document)).into_response())
}

#[cfg(test)]
mod tests {
    #[test]
    fn etag_matching_understands_lists_weak_validators_and_star() {
        let tag = "\"abc123\"";
        assert!(etag_matches("\"abc123\"", tag));
        assert!(etag_matches("W/\"abc123\"", tag));
        assert!(etag_matches("\"zzz\", W/\"abc123\"", tag));
        assert!(etag_matches("*", tag));
        assert!(!etag_matches("\"zzz\"", tag));
        assert!(!etag_matches("", tag));
    }

    #[test]
    fn the_etag_is_stable_and_changes_with_the_document() {
        let doc = |conc| RouterConfigDocument {
            processing_pools: vec![RouterPoolConfig {
                code: "p".into(),
                concurrency: conc,
                rate_limit_per_minute: None,
            }],
            queues: vec![],
        };
        assert_eq!(etag_of(&doc(1)).unwrap(), etag_of(&doc(1)).unwrap());
        assert_ne!(etag_of(&doc(1)).unwrap(), etag_of(&doc(2)).unwrap());
        assert!(etag_of(&doc(1)).unwrap().starts_with('"'));
    }

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
            // A client with no pool or subscription still gets its queues.
            &["acme".to_string(), "gamma".to_string()],
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
                "FC-platform-HIGH_PRIORITY.fifo",
                "FC-acme-DEFAULT.fifo",
                "FC-acme-HIGH_PRIORITY.fifo",
                "FC-beta-DEFAULT.fifo",
                "FC-beta-HIGH_PRIORITY.fifo",
                "FC-gamma-DEFAULT.fifo",
                "FC-gamma-HIGH_PRIORITY.fifo"
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
        let doc = build_document(&settings, &[pool("p", Some(&long), 1, None)], &[], &[]);
        assert_eq!(doc.queues.len(), 2);
        assert_eq!(doc.processing_pools.len(), 1);
    }
}
