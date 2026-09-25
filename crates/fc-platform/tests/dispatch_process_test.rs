//! `POST /api/dispatch/process` and `/api/dispatch/settled` against a real
//! PostgreSQL and a live subscriber, as Go's `dispatchjob/processing` and
//! `dispatchjob/settled` behave (pipeline review H2, R-57; owner decision
//! 28). Requires Docker:
//!   cargo test -p fc-platform --test dispatch_process_test -- --ignored

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use chrono::{DateTime, Utc};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use testcontainers::runners::AsyncRunner;
use testcontainers::ContainerAsync;
use testcontainers_modules::postgres::Postgres;
use tower::ServiceExt;

use fc_platform::scheduler::DispatchAuthService;
use fc_platform::shared::database::{create_pool, run_migrations, MigrationProfile};
use fc_platform::shared::dispatch_process_api::{
    delivery_http_client, dispatch_process_router, ClientCodeResolver, DispatchProcessState,
};
use fc_platform::{ClientRepository, DispatchJobRepository};

const APP_KEY: &str = "process-test-app-key";

// ── A subscriber that records what it receives ──────────────────────────

#[derive(Clone)]
struct Canned {
    status: u16,
    body: String,
    headers: Vec<(&'static str, String)>,
    delay: Duration,
}

impl Canned {
    fn status(status: u16) -> Self {
        Self {
            status,
            body: String::new(),
            headers: vec![],
            delay: Duration::ZERO,
        }
    }
}

#[derive(Default)]
struct Subscriber {
    responses: Mutex<VecDeque<Canned>>,
    received: Mutex<Vec<(HeaderMap, Bytes)>>,
    calls: AtomicUsize,
}

async fn subscriber_handler(
    State(s): State<Arc<Subscriber>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    s.calls.fetch_add(1, Ordering::SeqCst);
    s.received.lock().unwrap().push((headers, body));
    let canned = s
        .responses
        .lock()
        .unwrap()
        .pop_front()
        .unwrap_or_else(|| Canned::status(200));
    tokio::time::sleep(canned.delay).await;
    let mut resp = (StatusCode::from_u16(canned.status).unwrap(), canned.body).into_response();
    for (k, v) in canned.headers {
        resp.headers_mut().insert(k, v.parse().unwrap());
    }
    resp
}

async fn start_subscriber() -> (Arc<Subscriber>, String) {
    let s = Arc::new(Subscriber::default());
    let app = Router::new()
        .route("/hook", axum::routing::post(subscriber_handler))
        .with_state(s.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (s, format!("http://{addr}/hook"))
}

// ── Fixture ─────────────────────────────────────────────────────────────

struct Fixture {
    pool: PgPool,
    app: Router,
    auth: DispatchAuthService,
    subscriber: Arc<Subscriber>,
    target: String,
    _container: ContainerAsync<Postgres>,
}

async fn fixture() -> Fixture {
    let container = Postgres::default()
        .with_db_name("fc")
        .with_user("test")
        .with_password("test")
        .start()
        .await
        .expect("start postgres");
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(5432).await.unwrap();
    let pool = create_pool(&format!("postgresql://test:test@{host}:{port}/fc"))
        .await
        .unwrap();
    run_migrations(&pool, MigrationProfile::Production)
        .await
        .unwrap();
    let auth = DispatchAuthService::from_app_key(APP_KEY).unwrap();
    let state = DispatchProcessState {
        dispatch_job_repo: Arc::new(DispatchJobRepository::new(&pool)),
        http_client: delivery_http_client(),
        credentials: None,
        auth: auth.clone(),
        client_codes: Some(Arc::new(ClientCodeResolver::new(Arc::new(
            ClientRepository::new(&pool),
        )))),
    };
    let app = Router::new().nest("/api/dispatch", dispatch_process_router(state));
    let (subscriber, target) = start_subscriber().await;
    Fixture {
        pool,
        app,
        auth,
        subscriber,
        target,
        _container: container,
    }
}

struct Job {
    id: String,
    status: &'static str,
    mode: &'static str,
    group: Option<&'static str>,
    sequence: i32,
    max_retries: i32,
    client_id: Option<&'static str>,
    created_at: DateTime<Utc>,
}

fn job(n: i64) -> Job {
    Job {
        id: format!("j{n:012}"),
        status: "QUEUED",
        mode: "NEXT_ON_ERROR",
        group: None,
        sequence: 99,
        max_retries: 3,
        client_id: None,
        created_at: Utc::now() - chrono::Duration::seconds(100 - n),
    }
}

impl Fixture {
    async fn insert(&self, j: &Job) {
        sqlx::query(
            "INSERT INTO msg_dispatch_jobs (id, code, source, target_url, payload, data_only, status, mode, \
             message_group, sequence, max_retries, client_id, created_at, updated_at) \
             VALUES ($1, 'app:orders:order:shipped', 'app', $2, '{\"orderId\": 7}', false, $3, $4, $5, $6, \
             $7, $8, $9, $9)",
        )
        .bind(&j.id)
        .bind(&self.target)
        .bind(j.status)
        .bind(j.mode)
        .bind(j.group)
        .bind(j.sequence)
        .bind(j.max_retries)
        .bind(j.client_id)
        .bind(j.created_at)
        .execute(&self.pool)
        .await
        .expect("insert job");
    }

    async fn call(&self, path: &str, token: Option<&str>, body: Value) -> (StatusCode, Value) {
        let mut req = Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json");
        if let Some(t) = token {
            req = req.header("authorization", format!("Bearer {t}"));
        }
        let resp = self
            .app
            .clone()
            .oneshot(req.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn process(&self, id: &str) -> (StatusCode, Value) {
        let token = self.auth.sign(id);
        self.call(
            "/api/dispatch/process",
            Some(&token),
            json!({ "messageId": id }),
        )
        .await
    }

    async fn row(&self, id: &str) -> Row {
        sqlx::query_as(
            "SELECT status, attempt_count, scheduled_for, last_error, completed_at \
             FROM msg_dispatch_jobs WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .unwrap()
    }

    async fn attempts(&self, id: &str) -> Vec<Attempt> {
        sqlx::query_as(
            "SELECT attempt_number, status, response_code, error_type, error_message, response_body, \
             request_info FROM msg_dispatch_job_attempts WHERE dispatch_job_id = $1 ORDER BY attempted_at",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await
        .unwrap()
    }

    fn respond(&self, canned: Canned) {
        self.subscriber.responses.lock().unwrap().push_back(canned);
    }

    fn calls(&self) -> usize {
        self.subscriber.calls.load(Ordering::SeqCst)
    }
}

#[derive(Debug, sqlx::FromRow)]
struct Row {
    status: String,
    attempt_count: i32,
    scheduled_for: Option<DateTime<Utc>>,
    last_error: Option<String>,
    completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, sqlx::FromRow)]
struct Attempt {
    attempt_number: Option<i32>,
    status: Option<String>,
    response_code: Option<i32>,
    error_type: Option<String>,
    error_message: Option<String>,
    response_body: Option<String>,
    request_info: Option<Value>,
}

fn secs_from_now(at: Option<DateTime<Utc>>) -> i64 {
    (at.expect("scheduled_for is set") - Utc::now()).num_seconds()
}

// ── Authentication ──────────────────────────────────────────────────────

/// A callback without the job's own token is refused 401 `ack:false` and
/// delivers nothing: only a router holding the scheduler's token may
/// trigger a delivery.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_callback_without_the_jobs_token_is_refused() {
    let f = fixture().await;
    let j = job(1);
    f.insert(&j).await;
    let body = json!({ "messageId": j.id });
    let other = f.auth.sign("some-other-job");
    let forged = DispatchAuthService::from_app_key("another-key")
        .unwrap()
        .sign(&j.id);
    for token in [None, Some(other.as_str()), Some(forged.as_str())] {
        let (status, resp) = f.call("/api/dispatch/process", token, body.clone()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(resp, json!({"ack": false, "message": "unauthorized"}));
    }
    assert_eq!(f.calls(), 0);
    assert_eq!(f.row(&j.id).await.status, "QUEUED");

    let (status, resp) = f
        .call("/api/dispatch/process", None, json!({"nope": 1}))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(resp, json!({"ack": true, "message": "invalid messageId"}));
}

// ── Outcomes ────────────────────────────────────────────────────────────

/// A 2xx completes the job; the delivery is Go's envelope with the client
/// code, and the attempt records what was sent. Like Go, a success does not
/// bump `attempt_count` (it counts retries scheduled).
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_delivered_job_completes_and_records_its_attempt() {
    let f = fixture().await;
    sqlx::query(
        "INSERT INTO tnt_clients (id, name, identifier) VALUES ('clt_acme', 'Acme', 'acme')",
    )
    .execute(&f.pool)
    .await
    .unwrap();
    let mut j = job(1);
    j.client_id = Some("clt_acme");
    j.group = Some("orders-7");
    f.insert(&j).await;
    f.respond(Canned {
        body: "thanks".into(),
        ..Canned::status(202)
    });

    let (status, resp) = f.process(&j.id).await;
    assert_eq!((status, resp), (StatusCode::OK, json!({"ack": true})));
    let row = f.row(&j.id).await;
    assert_eq!(row.status, "COMPLETED");
    assert_eq!(row.attempt_count, 0);
    assert!(row.completed_at.is_some());

    let (headers, body) = f.subscriber.received.lock().unwrap()[0].clone();
    assert_eq!(headers["x-dispatch-job-id"], j.id.as_str());
    assert_eq!(headers["x-event-type"], "app:orders:order:shipped");
    assert_eq!(headers["x-flowcatalyst-client"], "clt_acme:acme");
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        body,
        json!({
            "id": j.id, "type": "app:orders:order:shipped", "source": "app",
            "attemptNumber": 1, "clientId": "clt_acme", "clientCode": "acme",
            "messageGroup": "orders-7", "data": {"orderId": 7}
        })
    );

    let attempts = f.attempts(&j.id).await;
    assert_eq!(attempts.len(), 1);
    let a = &attempts[0];
    assert_eq!(a.attempt_number, Some(1));
    assert_eq!(a.status.as_deref(), Some("SUCCESS"));
    assert_eq!(a.response_code, Some(202));
    assert_eq!(a.response_body.as_deref(), Some("thanks"));
    let info = a.request_info.clone().expect("request_info");
    assert_eq!(info["target"], f.target.as_str());
    assert_eq!(info["unsignedReason"], "no credential resolver configured");
    assert_eq!(
        info["headers"],
        json!([
            "Content-Type",
            "X-Dispatch-Job-Id",
            "X-Event-Type",
            "X-FlowCatalyst-Client"
        ])
    );
}

/// A retryable failure goes back to PENDING with a `scheduled_for` backoff
/// (5s after the first attempt), spends one attempt, and is ACKed: the
/// poller owns the retry, not the queue.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_retryable_failure_is_rescheduled_and_acked() {
    let f = fixture().await;
    let j = job(1);
    f.insert(&j).await;
    f.respond(Canned {
        body: "boom".into(),
        ..Canned::status(500)
    });
    let (status, resp) = f.process(&j.id).await;
    assert_eq!((status, resp), (StatusCode::OK, json!({"ack": true})));
    let row = f.row(&j.id).await;
    assert_eq!(row.status, "PENDING");
    assert_eq!(row.attempt_count, 1);
    let wait = secs_from_now(row.scheduled_for);
    assert!((3..=6).contains(&wait), "backoff {wait}s");
    assert_eq!(
        row.last_error.as_deref(),
        Some("HTTP 500 Internal Server Error (delivered unsigned: no credential resolver configured)")
    );
    let a = &f.attempts(&j.id).await[0];
    assert_eq!(a.status.as_deref(), Some("FAILURE"));
    assert_eq!(a.error_type.as_deref(), Some("HTTP_ERROR"));
    assert_eq!(a.response_code, Some(500));
    assert_eq!(a.response_body.as_deref(), Some("boom"));

    // The second attempt (the poller re-queued it) backs off 15s.
    sqlx::query("UPDATE msg_dispatch_jobs SET status = 'QUEUED' WHERE id = $1")
        .bind(&j.id)
        .execute(&f.pool)
        .await
        .unwrap();
    f.respond(Canned::status(503));
    f.process(&j.id).await;
    let row = f.row(&j.id).await;
    assert_eq!(row.attempt_count, 2);
    let wait = secs_from_now(row.scheduled_for);
    assert!((13..=16).contains(&wait), "backoff {wait}s");
    assert_eq!(f.attempts(&j.id).await[1].attempt_number, Some(2));
}

/// The last attempt of the budget fails the job for good.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_last_attempt_fails_the_job() {
    let f = fixture().await;
    let mut j = job(1);
    j.max_retries = 1;
    f.insert(&j).await;
    f.respond(Canned::status(502));
    f.process(&j.id).await;
    let row = f.row(&j.id).await;
    assert_eq!(row.status, "FAILED");
    assert!(row.completed_at.is_some());
}

/// 401/403 from the subscriber is terminal at once: a retry sends the same
/// credentials.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_refused_credential_fails_at_once() {
    let f = fixture().await;
    let mut j = job(1);
    j.max_retries = 5;
    f.insert(&j).await;
    f.respond(Canned::status(401));
    f.process(&j.id).await;
    assert_eq!(f.row(&j.id).await.status, "FAILED");
}

/// A 429 (with its Retry-After) and a 2xx `{"ack": false}` are cooperative
/// deferrals: rescheduled without spending the budget.
#[tokio::test]
#[ignore = "requires Docker"]
async fn deferrals_reschedule_without_spending_the_budget() {
    let f = fixture().await;
    let (a, b) = (job(1), job(2));
    f.insert(&a).await;
    f.insert(&b).await;
    f.respond(Canned {
        headers: vec![("retry-after", "20".into())],
        ..Canned::status(429)
    });
    f.process(&a.id).await;
    let row = f.row(&a.id).await;
    assert_eq!((row.status.as_str(), row.attempt_count), ("PENDING", 0));
    let wait = secs_from_now(row.scheduled_for);
    assert!((18..=20).contains(&wait), "retry-after {wait}s");
    let at = &f.attempts(&a.id).await[0];
    assert_eq!(at.response_code, Some(429));
    assert_eq!(at.error_type, None);
    assert_eq!(at.error_message.as_deref(), Some("rate limited (429)"));

    f.respond(Canned {
        body: r#"{"ack": false, "delaySeconds": 40}"#.into(),
        ..Canned::status(200)
    });
    f.process(&b.id).await;
    let row = f.row(&b.id).await;
    assert_eq!((row.status.as_str(), row.attempt_count), ("PENDING", 0));
    let wait = secs_from_now(row.scheduled_for);
    assert!((38..=40).contains(&wait), "delaySeconds {wait}s");
}

// ── What is not delivered ───────────────────────────────────────────────

/// A finished job, or a missing one, is ACKed without a delivery.
#[tokio::test]
#[ignore = "requires Docker"]
async fn finished_and_missing_jobs_are_acked_without_delivery() {
    let f = fixture().await;
    let mut j = job(1);
    j.status = "COMPLETED";
    f.insert(&j).await;
    assert_eq!(
        f.process(&j.id).await,
        (StatusCode::OK, json!({"ack": true}))
    );
    assert_eq!(
        f.process("j999999999999").await,
        (
            StatusCode::OK,
            json!({"ack": true, "message": "job not found"})
        )
    );
    assert_eq!(f.calls(), 0);
}

/// Two copies of one message delivered at once reach the subscriber once:
/// the conditional claim lets exactly one through.
#[tokio::test]
#[ignore = "requires Docker"]
async fn concurrent_copies_deliver_once() {
    let f = fixture().await;
    let j = job(1);
    f.insert(&j).await;
    f.respond(Canned {
        delay: Duration::from_millis(500),
        ..Canned::status(200)
    });
    let (r1, r2) = tokio::join!(f.process(&j.id), f.process(&j.id));
    assert_eq!(f.calls(), 1);
    let messages: Vec<Value> = vec![r1.1["message"].clone(), r2.1["message"].clone()];
    assert!(messages.contains(&json!("already claimed")), "{messages:?}");
    assert_eq!((r1.0, r2.0), (StatusCode::OK, StatusCode::OK));
    assert_eq!(f.row(&j.id).await.status, "COMPLETED");
}

/// A BLOCK_ON_ERROR job whose group has an earlier failure goes back to
/// PENDING undelivered, without spending budget; a NEXT_ON_ERROR sibling in
/// the same place is delivered.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_held_block_on_error_job_is_returned_undelivered() {
    let f = fixture().await;
    let mut failed = job(1);
    failed.group = Some("g");
    failed.status = "FAILED";
    failed.sequence = 1;
    let mut boe = job(2);
    boe.group = Some("g");
    boe.mode = "BLOCK_ON_ERROR";
    boe.sequence = 2;
    let mut noe = job(3);
    noe.group = Some("g");
    noe.sequence = 2;
    for j in [&failed, &boe, &noe] {
        f.insert(j).await;
    }
    assert_eq!(
        f.process(&boe.id).await,
        (
            StatusCode::OK,
            json!({"ack": true, "message": "group blocked"})
        )
    );
    let row = f.row(&boe.id).await;
    assert_eq!((row.status.as_str(), row.attempt_count), ("PENDING", 0));
    assert_eq!(f.calls(), 0);

    f.process(&noe.id).await;
    assert_eq!(f.calls(), 1);
    assert_eq!(f.row(&noe.id).await.status, "COMPLETED");
}

/// An internal error answers 503 `ack:false`, which both routers retry,
/// rather than Go's 500, which both ACK-drop (review R-57).
#[tokio::test]
#[ignore = "requires Docker"]
async fn an_internal_error_is_retryable() {
    let f = fixture().await;
    let j = job(1);
    f.insert(&j).await;
    f.pool.close().await;
    assert_eq!(
        f.process(&j.id).await,
        (
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"ack": false, "message": "load failed"})
        )
    );
    assert_eq!(f.calls(), 0);
}

// ── /settled and the reaper ─────────────────────────────────────────────

/// The router's settled hook resets exactly the QUEUED/PROCESSING jobs whose
/// tokens verify; a batch with no valid token is 401.
#[tokio::test]
#[ignore = "requires Docker"]
async fn settled_resets_only_verified_queued_jobs() {
    let f = fixture().await;
    let (a, b) = (job(1), job(2));
    let mut done = job(3);
    done.status = "COMPLETED";
    for j in [&a, &b, &done] {
        f.insert(j).await;
    }
    let body = json!({
        "reason": "head failed",
        "jobs": [
            {"id": a.id, "token": f.auth.sign(&a.id)},
            {"id": b.id, "token": "forged"},
            {"id": done.id, "token": f.auth.sign(&done.id)},
        ]
    });
    let (status, resp) = f.call("/api/dispatch/settled", None, body).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(resp, json!({"settled": 1, "ids": [a.id]}));
    let row = f.row(&a.id).await;
    assert_eq!(row.status, "PENDING");
    assert_eq!(row.last_error.as_deref(), Some("head failed"));
    assert_eq!(f.row(&b.id).await.status, "QUEUED");
    assert_eq!(f.row(&done.id).await.status, "COMPLETED");

    let (status, resp) = f
        .call(
            "/api/dispatch/settled",
            None,
            json!({"jobs": [{"id": b.id, "token": "forged"}]}),
        )
        .await;
    assert_eq!(
        (status, resp),
        (StatusCode::UNAUTHORIZED, json!({"settled": 0}))
    );
    assert_eq!(
        f.call("/api/dispatch/settled", None, json!({"jobs": []}))
            .await,
        (StatusCode::OK, json!({"settled": 0}))
    );
}

/// The reaper resets BLOCK_ON_ERROR siblings stranded behind a FAILED head:
/// QUEUED at once, PROCESSING only once it is no longer plausibly live.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_reaper_resets_stranded_siblings() {
    let f = fixture().await;
    let mut head = job(1);
    head.group = Some("g");
    head.status = "FAILED";
    head.sequence = 1;
    let mut queued = job(2);
    queued.group = Some("g");
    queued.mode = "BLOCK_ON_ERROR";
    queued.sequence = 2;
    let mut live = job(3);
    live.group = Some("g");
    live.mode = "BLOCK_ON_ERROR";
    live.status = "PROCESSING";
    live.sequence = 2;
    let mut stale = job(4);
    stale.group = Some("g");
    stale.mode = "BLOCK_ON_ERROR";
    stale.status = "PROCESSING";
    stale.sequence = 2;
    let mut noe = job(5);
    noe.group = Some("g");
    noe.sequence = 2;
    for j in [&head, &queued, &live, &stale, &noe] {
        f.insert(j).await;
    }
    sqlx::query(
        "UPDATE msg_dispatch_jobs SET updated_at = NOW() - INTERVAL '50 minutes' WHERE id = $1",
    )
    .bind(&stale.id)
    .execute(&f.pool)
    .await
    .unwrap();
    sqlx::query("UPDATE msg_dispatch_jobs SET updated_at = NOW() WHERE id = $1")
        .bind(&live.id)
        .execute(&f.pool)
        .await
        .unwrap();
    let repo = DispatchJobRepository::new(&f.pool);
    let mut ids = fc_platform::dispatch_job::reaper::sweep_once(
        &repo,
        fc_platform::dispatch_job::reaper::DEFAULT_PROCESSING_LIVE_AFTER,
    )
    .await
    .unwrap();
    ids.sort();
    assert_eq!(ids, vec![queued.id.clone(), stale.id.clone()]);
    assert_eq!(f.row(&live.id).await.status, "PROCESSING");
    assert_eq!(f.row(&noe.id).await.status, "QUEUED");
}
