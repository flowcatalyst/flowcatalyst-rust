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

/// State for [`distributed_rate_limit_per_email`]: one per route whose
/// JSON body names an e-mail address to budget (the password-reset
/// request, S2.7).
#[derive(Clone)]
pub struct DistributedEmailLimitState {
    pub store: Arc<dyn RateLimitStore>,
    pub bucket: Bucket,
    pub policy: RateLimitPolicy,
    /// Only a `POST` whose path ends with this is budgeted; the layer's
    /// other routes pass through.
    pub path_suffix: &'static str,
    /// The route's own answer when it has nothing to say. Sent unchanged
    /// when the address is over budget, so the limit reveals nothing about
    /// whether the address exists.
    pub over_budget: fn() -> Response,
}

/// The largest body the per-e-mail limiter reads to find the address.
pub const EMAIL_LIMIT_MAX_BODY_BYTES: usize = 64 * 1024;

/// The budget key of a JSON body's `email`: trimmed and lower-cased, so
/// `A@B.C ` and `a@b.c` share a budget. `None` for a blank or missing
/// address (the handler answers those itself, and they spend nothing) or a
/// body that isn't JSON.
pub fn email_budget_key(body: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    let email = value.get("email")?.as_str()?.trim().to_lowercase();
    (!email.is_empty()).then_some(email)
}

/// Budget a request per the e-mail address in its JSON body, cluster-wide.
/// Over budget, the handler never runs (nothing is issued or mailed) and
/// the caller gets the route's ordinary answer (`over_budget`). The
/// per-IP layer runs outside this one, so an IP already over its budget
/// doesn't also spend the address's. Fails open on a backend error, like
/// the per-IP layer.
pub async fn distributed_rate_limit_per_email(
    State(state): State<DistributedEmailLimitState>,
    request: Request,
    next: Next,
) -> Response {
    if request.method() != axum::http::Method::POST
        || !request.uri().path().ends_with(state.path_suffix)
    {
        return next.run(request).await;
    }
    let (parts, body) = request.into_parts();
    let bytes = match axum::body::to_bytes(body, EMAIL_LIMIT_MAX_BODY_BYTES).await {
        Ok(bytes) => bytes,
        Err(_) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
    };
    let key = email_budget_key(&bytes);
    let request = Request::from_parts(parts, axum::body::Body::from(bytes));
    let Some(key) = key else {
        return next.run(request).await;
    };

    match state
        .store
        .check_and_record(state.bucket, &key, state.policy)
        .await
    {
        Ok(RateLimitDecision::Allow) => next.run(request).await,
        Ok(RateLimitDecision::Reject { .. }) => {
            warn!(
                bucket = state.bucket.as_str(),
                domain = key.rsplit_once('@').map(|(_, d)| d).unwrap_or(""),
                "e-mail rate limit reached; request answered without acting",
            );
            (state.over_budget)()
        }
        Err(e) => {
            warn!(
                error = %e,
                bucket = state.bucket.as_str(),
                "distributed rate-limit backend error; failing open",
            );
            next.run(request).await
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::rate_limit_store::RateLimitError;
    use async_trait::async_trait;
    use axum::{body::Body, routing::post, Router};
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use std::time::Duration;
    use tower::ServiceExt;

    /// Counts per (bucket, key); rejects past the policy's limit.
    #[derive(Default)]
    struct CountingStore(Mutex<HashMap<String, u32>>);

    #[async_trait]
    impl RateLimitStore for CountingStore {
        async fn check_and_record(
            &self,
            bucket: Bucket,
            key: &str,
            policy: RateLimitPolicy,
        ) -> Result<RateLimitDecision, RateLimitError> {
            let mut map = self.0.lock().unwrap();
            let n = map.entry(format!("{}:{key}", bucket.as_str())).or_default();
            *n += 1;
            Ok(if *n > policy.limit {
                RateLimitDecision::Reject {
                    retry_after_secs: 60,
                }
            } else {
                RateLimitDecision::Allow
            })
        }
    }

    fn silent() -> Response {
        Json(serde_json::json!({"message": "silent"})).into_response()
    }

    fn app(store: Arc<CountingStore>, handled: Arc<AtomicUsize>) -> Router {
        let echo = move |body: String| {
            let handled = handled.clone();
            async move {
                handled.fetch_add(1, Ordering::SeqCst);
                assert!(body.contains("email"), "the handler still reads the body");
                silent()
            }
        };
        Router::new()
            .route("/request", post(echo.clone()))
            .route("/confirm", post(echo))
            .layer(axum::middleware::from_fn_with_state(
                DistributedEmailLimitState {
                    store,
                    bucket: Bucket::PASSWORD_RESET_EMAIL,
                    policy: RateLimitPolicy::new(Duration::from_secs(3600), 2),
                    path_suffix: "/request",
                    over_budget: silent,
                },
                distributed_rate_limit_per_email,
            ))
    }

    async fn post_json(app: &Router, path: &str, body: &str) -> (StatusCode, String) {
        let res = app
            .clone()
            .oneshot(
                Request::post(path)
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    }

    /// S2.7: past the address's budget the handler never runs and the
    /// answer is the handler's own, whatever the case or padding of the
    /// address.
    #[tokio::test]
    async fn an_address_over_budget_gets_the_same_answer_and_nothing_runs() {
        let store = Arc::new(CountingStore::default());
        let handled = Arc::new(AtomicUsize::new(0));
        let app = app(store, handled.clone());

        let first = post_json(&app, "/request", r#"{"email":"a@b.c"}"#).await;
        post_json(&app, "/request", r#"{"email":" A@B.C "}"#).await;
        let over = post_json(&app, "/request", r#"{"email":"a@b.c"}"#).await;
        assert_eq!(
            first,
            (StatusCode::OK, r#"{"message":"silent"}"#.to_string())
        );
        assert_eq!(over, first);
        assert_eq!(handled.load(Ordering::SeqCst), 2, "the third never ran");

        // Another address has its own budget; other routes aren't budgeted.
        post_json(&app, "/request", r#"{"email":"x@b.c"}"#).await;
        post_json(&app, "/confirm", r#"{"email":"a@b.c"}"#).await;
        assert_eq!(handled.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn the_budget_key_is_the_trimmed_lower_case_address() {
        assert_eq!(
            email_budget_key(br#"{"email":"  Bob@Example.COM "}"#).as_deref(),
            Some("bob@example.com")
        );
        assert_eq!(email_budget_key(br#"{"email":"  "}"#), None);
        assert_eq!(email_budget_key(br#"{"other":"x"}"#), None);
        assert_eq!(email_budget_key(b"not json"), None);
    }
}
