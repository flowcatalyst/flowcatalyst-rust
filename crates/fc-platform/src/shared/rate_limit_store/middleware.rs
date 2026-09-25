//! Axum middleware + inline helpers that consume `RateLimitStore`.
//!
//! Layered on top of the in-memory `governor` middleware in
//! `rate_limit_middleware`. The two complement each other:
//!
//! * **In-memory governor** rejects bursts at the instance — sub-ms
//!   decision, never touches the DB/Redis, but only sees traffic
//!   landing on one replica.
//! * **Distributed store (this file)** sees the full cluster — slower
//!   (one round-trip per request) but catches a coordinated attacker
//!   spreading requests across replicas.
//!
//! Both must pass for the request to proceed. The in-memory check runs
//! first (cheap), the distributed check second (only on cache miss).

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use tracing::warn;

use super::{Bucket, RateLimitDecision, RateLimitPolicy, RateLimitStore};
use crate::shared::api_common::ApiError;

/// State handed to `distributed_rate_limit_per_ip` via
/// `from_fn_with_state`. One instance per (bucket, policy) — multiple
/// can be wired in parallel onto different route groups.
#[derive(Clone)]
pub struct DistributedIpLimitState {
    pub store: Arc<dyn RateLimitStore>,
    pub bucket: Bucket,
    pub policy: RateLimitPolicy,
}

/// Reject the request with 429 + `Retry-After` when the cluster-wide
/// counter for the source IP exhausts the bucket. Requests with no
/// resolvable IP (no trusted-proxy header) pass through — controlling
/// those is the load balancer's job, not ours.
pub async fn distributed_rate_limit_per_ip(
    State(state): State<DistributedIpLimitState>,
    request: Request,
    next: Next,
) -> Response {
    let Some(ip) = extract_ip(request.headers()) else {
        return next.run(request).await;
    };

    match state
        .store
        .check_and_record(state.bucket, &ip, state.policy)
        .await
    {
        Ok(RateLimitDecision::Allow) => next.run(request).await,
        Ok(RateLimitDecision::Reject { retry_after_secs }) => {
            too_many_requests_response(retry_after_secs, "rate limit exceeded for this IP")
        }
        Err(e) => {
            // Fail open: a degraded backend should not take down auth.
            // The in-memory governor is still in front of us, so bursts
            // are still capped per-instance. We log loudly so ops sees
            // the degradation.
            warn!(
                error = %e,
                bucket = state.bucket.as_str(),
                "distributed rate-limit backend error; failing open",
            );
            next.run(request).await
        }
    }
}

/// Inline helper for handlers that need per-(non-IP) keying — typically
/// per-`client_id` at the OAuth token/authorize endpoints, or per-email
/// at password reset. Returns `Err` shaped as the standard 429 response
/// so the handler can early-return it.
pub async fn enforce_distributed(
    store: &dyn RateLimitStore,
    bucket: Bucket,
    key: &str,
    policy: RateLimitPolicy,
) -> Result<(), Response> {
    match store.check_and_record(bucket, key, policy).await {
        Ok(RateLimitDecision::Allow) => Ok(()),
        Ok(RateLimitDecision::Reject { retry_after_secs }) => Err(too_many_requests_response(
            retry_after_secs,
            "rate limit exceeded",
        )),
        Err(e) => {
            warn!(
                error = %e,
                bucket = bucket.as_str(),
                "distributed rate-limit backend error; failing open",
            );
            Ok(())
        }
    }
}

fn extract_ip(headers: &HeaderMap) -> Option<String> {
    crate::shared::middleware::extract_trusted_client_ip(headers)
}

fn too_many_requests_response(retry_after_secs: u32, message: &str) -> Response {
    let body = ApiError::new("TOO_MANY_REQUESTS", message.to_string());
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(
            axum::http::header::RETRY_AFTER,
            retry_after_secs.max(1).to_string(),
        )],
        Json(body),
    )
        .into_response()
}
