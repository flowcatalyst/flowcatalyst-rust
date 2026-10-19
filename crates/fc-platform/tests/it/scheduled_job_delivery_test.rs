//! The scheduled-job dispatcher against a live subscriber (Go
//! `scheduledjob/scheduler/dispatcher_pg_test.go`, commit 6e6b9307): a 429
//! requeues the firing no sooner than `Retry-After` and gives back the attempt,
//! the queue query leaves a deferred firing alone until `not_before`, and a
//! 422 with `FlowCatalyst-Outcome: reject` is terminal with attempts left.
//! Requires a Postgres (see `support::db`).

use crate::support::TestApp;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{routing, Router};
use fc_common::netguard::dev_policy;
use fc_common::tsid::{self, EntityType};
use fc_platform::scheduled_job::scheduler::{ScheduledJobDispatcher, ScheduledJobSchedulerConfig};
use fc_platform::{ScheduledJobInstanceRepository, ScheduledJobRepository};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio::time::sleep;

#[derive(Clone)]
struct Canned {
    status: u16,
    headers: Vec<(&'static str, String)>,
}

#[derive(Default)]
struct Target {
    responses: Mutex<VecDeque<Canned>>,
    hits: AtomicUsize,
}

async fn handle(State(t): State<Arc<Target>>) -> Response {
    t.hits.fetch_add(1, Ordering::SeqCst);
    let canned = t.responses.lock().unwrap().pop_front().unwrap_or(Canned {
        status: 200,
        headers: vec![],
    });
    let mut resp = StatusCode::from_u16(canned.status).unwrap().into_response();
    for (k, v) in canned.headers {
        resp.headers_mut().insert(k, v.parse().unwrap());
    }
    resp
}

async fn start_target() -> (Arc<Target>, String) {
    let t = Arc::new(Target::default());
    let app = Router::new()
        .route("/hook", routing::post(handle))
        .with_state(t.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (t, format!("http://{addr}/hook"))
}

/// A job targeting `url` and one QUEUED firing of it; returns the instance id.
async fn queued_firing(app: &TestApp, code: &str, url: &str) -> String {
    let job_id = tsid::generate(EntityType::ScheduledJob);
    sqlx::query(
        "INSERT INTO msg_scheduled_jobs (id, code, name, status, crons, target_url, delivery_max_attempts)
         VALUES ($1, $2, $2, 'ACTIVE', '{}', $3, 3)",
    )
    .bind(&job_id)
    .bind(code)
    .bind(url)
    .execute(&app.pool)
    .await
    .unwrap();
    let instance_id = tsid::generate(EntityType::ScheduledJobInstance);
    sqlx::query(
        "INSERT INTO msg_scheduled_job_instances
           (id, scheduled_job_id, job_code, trigger_kind, fired_at, status, delivery_attempts, created_at)
         VALUES ($1, $2, $3, 'MANUAL', NOW(), 'QUEUED', 0, NOW())",
    )
    .bind(&instance_id)
    .bind(&job_id)
    .bind(code)
    .execute(&app.pool)
    .await
    .unwrap();
    instance_id
}

struct Row {
    status: String,
    attempts: i32,
    error: Option<String>,
    not_before: Option<chrono::DateTime<chrono::Utc>>,
}

async fn row(app: &TestApp, id: &str) -> Row {
    let (status, attempts, error, not_before): (
        String,
        i32,
        Option<String>,
        Option<chrono::DateTime<chrono::Utc>>,
    ) = sqlx::query_as(
        "SELECT status, delivery_attempts, delivery_error, not_before
               FROM msg_scheduled_job_instances WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    Row {
        status,
        attempts,
        error,
        not_before,
    }
}

/// Run the dispatcher until `done` holds (it ticks every 100 ms), then stop it.
async fn run_until(app: &TestApp, mut done: impl FnMut() -> bool) {
    let (stop, rx) = broadcast::channel(1);
    let config = ScheduledJobSchedulerConfig {
        dispatch_interval: Duration::from_millis(100),
        delivery_policy: dev_policy(),
        ..ScheduledJobSchedulerConfig::default()
    };
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let dispatcher = ScheduledJobDispatcher::new(
        config,
        Arc::new(ScheduledJobRepository::new(&app.pool)),
        Arc::new(ScheduledJobInstanceRepository::new(&app.pool)),
        client,
        rx,
    );
    let task = tokio::spawn(dispatcher.run());
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(
            Instant::now() < deadline,
            "the dispatcher did not get there"
        );
        sleep(Duration::from_millis(50)).await;
    }
    stop.send(()).unwrap();
    task.await.unwrap();
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_429_defers_without_spending_an_attempt_and_waits_for_not_before() {
    let app = TestApp::setup().await;
    let (target, url) = start_target().await;
    target.responses.lock().unwrap().push_back(Canned {
        status: 429,
        headers: vec![("retry-after", "120".into())],
    });
    let id = queued_firing(&app, "sjdsp-429", &url).await;

    let before = chrono::Utc::now();
    let t = target.clone();
    run_until(&app, move || t.hits.load(Ordering::SeqCst) >= 1).await;
    // Let the deferral's write land, then give the next ticks time to (wrongly) retry.
    sleep(Duration::from_millis(300)).await;
    let r = row(&app, &id).await;
    assert_eq!(r.status, "QUEUED");
    assert_eq!(r.attempts, 0, "a deferral spends no attempt");
    let nb = r.not_before.expect("not_before set");
    assert!(
        (nb - (before + chrono::Duration::seconds(120)))
            .num_seconds()
            .abs()
            <= 5,
        "{nb}"
    );

    // Another run of the dispatcher: the deferred firing is not delivered
    // before not_before.
    let t = target.clone();
    let hits_then = t.hits.load(Ordering::SeqCst);
    let deadline = Instant::now() + Duration::from_millis(600);
    run_until(&app, move || Instant::now() > deadline).await;
    assert_eq!(
        target.hits.load(Ordering::SeqCst),
        hits_then,
        "not delivered before not_before"
    );

    // Once not_before passes, it is delivered.
    sqlx::query("UPDATE msg_scheduled_job_instances SET not_before = NOW() - INTERVAL '1 second' WHERE id = $1")
        .bind(&id)
        .execute(&app.pool)
        .await
        .unwrap();
    let t = target.clone();
    run_until(&app, move || t.hits.load(Ordering::SeqCst) > hits_then).await;
    sleep(Duration::from_millis(200)).await;
    assert_eq!(row(&app, &id).await.status, "DELIVERED");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_reject_outcome_is_terminal_with_attempts_left() {
    let app = TestApp::setup().await;
    let (target, url) = start_target().await;
    target.responses.lock().unwrap().push_back(Canned {
        status: 422,
        headers: vec![("flowcatalyst-outcome", "reject".into())],
    });
    let rejected = queued_firing(&app, "sjdsp-reject", &url).await;
    let t = target.clone();
    run_until(&app, move || t.hits.load(Ordering::SeqCst) >= 1).await;
    sleep(Duration::from_millis(300)).await;
    let r = row(&app, &rejected).await;
    assert_eq!(r.status, "DELIVERY_FAILED");
    assert!(
        r.error.as_deref().is_some_and(|e| e.contains("reject")),
        "{:?}",
        r.error
    );

    // A plain 422 is retried (back to QUEUED with the attempt spent).
    target.responses.lock().unwrap().push_back(Canned {
        status: 422,
        headers: vec![],
    });
    let plain = queued_firing(&app, "sjdsp-plain-422", &url).await;
    let before = target.hits.load(Ordering::SeqCst);
    let t = target.clone();
    run_until(&app, move || t.hits.load(Ordering::SeqCst) > before).await;
    sleep(Duration::from_millis(100)).await;
    let r = row(&app, &plain).await;
    assert!(
        r.status == "QUEUED" || r.status == "DELIVERED",
        "a plain 422 is not terminal: {}",
        r.status
    );
}
