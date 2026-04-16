//! Performance & Load Tests
//!
//! Tests that verify throughput and concurrency characteristics of the platform.
//!
//! These tests require Docker to be running and are ignored by default:
//!   cargo test -p fc-platform --test load_tests -- --ignored
//!
//! For verbose timing output:
//!   cargo test -p fc-platform --test load_tests -- --ignored --nocapture

use std::sync::Arc;
use std::time::Instant;

use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

use fc_platform::shared::database::{create_pool, run_migrations};
use fc_platform::{
    ClientRepository, EventRepository, DispatchJobRepository,
    Client, Event, DispatchJob,
};

// ─── Test Helpers ──────────────────────────────────────────────────────────

async fn setup_test_db() -> (sqlx::PgPool, testcontainers::ContainerAsync<Postgres>) {
    let container = Postgres::default()
        .with_db_name("flowcatalyst_test")
        .with_user("test")
        .with_password("test")
        .start()
        .await
        .expect("Failed to start PostgreSQL container");

    let host = container.get_host().await.expect("Failed to get host");
    let port = container.get_host_port_ipv4(5432).await.expect("Failed to get port");

    let database_url = format!(
        "postgresql://test:test@{}:{}/flowcatalyst_test",
        host, port
    );

    let pool = create_pool(&database_url)
        .await
        .expect("Failed to connect to test database");

    run_migrations(&pool)
        .await
        .expect("Failed to run migrations");

    (pool, container)
}

// ─── Sequential Insert Throughput ────────────────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_client_sequential_insert_throughput() {
    let (pool, _container) = setup_test_db().await;
    let repo = ClientRepository::new(&pool);
    let count = 100;

    let start = Instant::now();
    for i in 0..count {
        let client = Client::new(&format!("Load Test {}", i), &format!("load-test-{}", i));
        repo.insert(&client).await.expect("Failed to insert client");
    }
    let elapsed = start.elapsed();

    let rate = count as f64 / elapsed.as_secs_f64();
    println!("Sequential client inserts: {} in {:.2?} ({:.0} ops/sec)", count, elapsed, rate);
    assert!(elapsed.as_secs() < 30, "Sequential inserts should complete within 30s");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_event_sequential_insert_throughput() {
    let (pool, _container) = setup_test_db().await;
    let repo = EventRepository::new(&pool);
    let count = 500;

    let start = Instant::now();
    for i in 0..count {
        let event = Event::new(
            "load:test:event",
            "load-test",
            serde_json::json!({"index": i}),
        );
        repo.insert(&event).await.expect("Failed to insert event");
    }
    let elapsed = start.elapsed();

    let rate = count as f64 / elapsed.as_secs_f64();
    println!("Sequential event inserts: {} in {:.2?} ({:.0} ops/sec)", count, elapsed, rate);
    assert!(elapsed.as_secs() < 60, "Sequential event inserts should complete within 60s");
}

// ─── Batch Insert Throughput ─────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_dispatch_job_batch_insert_throughput() {
    let (pool, _container) = setup_test_db().await;
    let repo = DispatchJobRepository::new(&pool);

    // Batch of 100
    let jobs: Vec<DispatchJob> = (0..100)
        .map(|i| {
            DispatchJob::for_event(
                &format!("evt-{}", i),
                "load:test:event",
                "load-test",
                "https://example.com/webhook",
                &format!("{{\"index\":{}}}", i),
            )
        })
        .collect();

    let start = Instant::now();
    repo.insert_many(&jobs).await.expect("Failed to batch insert");
    let elapsed = start.elapsed();

    println!("Batch insert 100 dispatch jobs: {:.2?} ({:.0} ops/sec)", elapsed, 100.0 / elapsed.as_secs_f64());

    // Verify
    let total = repo.count_all().await.expect("Failed to count");
    assert_eq!(total, 100);

    // 10x batches of 100 = 1000 total
    let start = Instant::now();
    for batch in 0..9 {
        let jobs: Vec<DispatchJob> = (0..100)
            .map(|i| {
                DispatchJob::for_event(
                    &format!("evt-b{}-{}", batch, i),
                    "load:test:event",
                    "load-test",
                    "https://example.com/webhook",
                    "{}",
                )
            })
            .collect();
        repo.insert_many(&jobs).await.expect("Failed to batch insert");
    }
    let elapsed = start.elapsed();

    let total = repo.count_all().await.expect("Failed to count");
    assert_eq!(total, 1000);

    let rate = 900.0 / elapsed.as_secs_f64();
    println!("9 batches of 100 dispatch jobs (900 total): {:.2?} ({:.0} ops/sec)", elapsed, rate);
    assert!(elapsed.as_secs() < 30, "Batch inserts should complete within 30s");
}

// ─── Concurrent Insert Throughput ────────────────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_concurrent_event_inserts() {
    let (pool, _container) = setup_test_db().await;
    let repo = Arc::new(EventRepository::new(&pool));
    let concurrency = 10;
    let per_task = 50;
    let total = concurrency * per_task;

    let start = Instant::now();
    let mut handles = Vec::new();

    for task_id in 0..concurrency {
        let repo = repo.clone();
        let handle = tokio::spawn(async move {
            for i in 0..per_task {
                let event = Event::new(
                    "load:concurrent:event",
                    &format!("task-{}", task_id),
                    serde_json::json!({"task": task_id, "index": i}),
                );
                repo.insert(&event).await.expect("Failed to insert event");
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.expect("Task panicked");
    }
    let elapsed = start.elapsed();

    let rate = total as f64 / elapsed.as_secs_f64();
    println!("Concurrent event inserts ({} tasks x {} each = {} total): {:.2?} ({:.0} ops/sec)",
        concurrency, per_task, total, elapsed, rate);
    assert!(elapsed.as_secs() < 60, "Concurrent inserts should complete within 60s");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_concurrent_client_inserts() {
    let (pool, _container) = setup_test_db().await;
    let repo = Arc::new(ClientRepository::new(&pool));
    let concurrency = 5;
    let per_task = 20;
    let total = concurrency * per_task;

    let start = Instant::now();
    let mut handles = Vec::new();

    for task_id in 0..concurrency {
        let repo = repo.clone();
        let handle = tokio::spawn(async move {
            for i in 0..per_task {
                let client = Client::new(
                    &format!("Concurrent {} - {}", task_id, i),
                    &format!("conc-{}-{}", task_id, i),
                );
                repo.insert(&client).await.expect("Failed to insert client");
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.expect("Task panicked");
    }
    let elapsed = start.elapsed();

    let rate = total as f64 / elapsed.as_secs_f64();
    println!("Concurrent client inserts ({} tasks x {} each = {} total): {:.2?} ({:.0} ops/sec)",
        concurrency, per_task, total, elapsed, rate);
    assert!(elapsed.as_secs() < 30, "Concurrent client inserts should complete within 30s");
}

// ─── Query Performance Under Load ────────────────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_dispatch_job_query_performance() {
    let (pool, _container) = setup_test_db().await;
    let repo = DispatchJobRepository::new(&pool);

    // Seed 500 jobs (5 batches of 100)
    for batch in 0..5 {
        let jobs: Vec<DispatchJob> = (0..100)
            .map(|i| {
                let mut job = DispatchJob::for_event(
                    &format!("evt-q{}-{}", batch, i),
                    "load:query:event",
                    "load-test",
                    "https://example.com/webhook",
                    "{}",
                );
                if i % 3 == 0 {
                    job.mark_queued();
                }
                job
            })
            .collect();
        repo.insert_many(&jobs).await.expect("Failed to batch insert");
    }

    let total = repo.count_all().await.expect("Failed to count");
    assert_eq!(total, 500);

    // Benchmark: find_pending_for_dispatch
    let start = Instant::now();
    let pending = repo.find_pending_for_dispatch(100).await.expect("Failed to query pending");
    let elapsed = start.elapsed();
    println!("find_pending_for_dispatch(100) from 500 jobs: {:.2?} ({} results)", elapsed, pending.len());
    assert!(elapsed.as_millis() < 500, "Pending query should complete within 500ms");

    // Benchmark: count_by_status
    let start = Instant::now();
    let pending_count = repo.count_by_status(fc_platform::DispatchStatus::Pending).await.expect("Failed to count");
    let elapsed = start.elapsed();
    println!("count_by_status(Pending) from 500 jobs: {:.2?} (count={})", elapsed, pending_count);
    assert!(elapsed.as_millis() < 200, "Count query should complete within 200ms");

    // Benchmark: find_by_status with limit
    let start = Instant::now();
    let queued = repo.find_by_status(fc_platform::DispatchStatus::Queued, 50).await.expect("Failed to query");
    let elapsed = start.elapsed();
    println!("find_by_status(Queued, 50) from 500 jobs: {:.2?} ({} results)", elapsed, queued.len());
    assert!(elapsed.as_millis() < 500, "Status query should complete within 500ms");
}

// ─── API Throughput Under Load ───────────────────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_api_batch_events_throughput() {
    use axum::{body::Body, http::Request, Router};
    use tower::ServiceExt;
    use http_body_util::BodyExt;
    use fc_platform::auth::auth_service::{AuthConfig, AuthService};
    use fc_platform::domain::{Principal, UserScope};
    use fc_platform::{RoleRepository, AuthorizationService};
    use fc_platform::api::{AppState, AuthLayer, SdkEventsState, sdk_events_batch_router};

    let (pool, _container) = setup_test_db().await;

    let auth_service = Arc::new(AuthService::new(AuthConfig {
        secret_key: "test-secret-key-for-integration-tests-minimum-32-chars!!".to_string(),
        issuer: "flowcatalyst".to_string(),
        audience: "flowcatalyst".to_string(),
        access_token_expiry_secs: 3600,
        session_token_expiry_secs: 28800,
        refresh_token_expiry_secs: 86400,
        rsa_private_key: None,
        rsa_public_key: None,
        rsa_public_key_previous: None,
    }));

    let role_repo = Arc::new(RoleRepository::new(&pool));
    let authz_service = Arc::new(AuthorizationService::new(role_repo));
    let app_state = AppState {
        auth_service: auth_service.clone(),
        authz_service,
    };

    let event_repo = Arc::new(EventRepository::new(&pool));
    let sdk_events_state = SdkEventsState { event_repo, dispatch: None };
    let app: Router = Router::new()
        .nest("/api/events", sdk_events_batch_router(sdk_events_state))
        .layer(AuthLayer::new(app_state));

    let principal = Principal::new_user("load@test.local", UserScope::Anchor);
    let token = auth_service.generate_access_token(&principal).expect("Failed to generate token");

    // 10 batch requests of 100 events each = 1000 events
    let start = Instant::now();
    for batch in 0..10 {
        let items: Vec<serde_json::Value> = (0..100)
            .map(|i| serde_json::json!({
                "type": "load:api:event",
                "source": "load-test",
                "data": {"batch": batch, "index": i}
            }))
            .collect();

        let response = app.clone().oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/events/batch")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::from(serde_json::to_string(&serde_json::json!({"items": items})).unwrap()))
                .unwrap()
        ).await.unwrap();

        assert_eq!(response.status().as_u16(), 200, "Batch {} failed", batch);
    }
    let elapsed = start.elapsed();

    let rate = 1000.0 / elapsed.as_secs_f64();
    println!("API batch events (10 batches x 100 = 1000 events): {:.2?} ({:.0} events/sec)", elapsed, rate);
    assert!(elapsed.as_secs() < 60, "API batch events should complete within 60s");
}
