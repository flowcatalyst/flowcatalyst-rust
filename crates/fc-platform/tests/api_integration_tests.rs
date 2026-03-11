//! API Integration Tests
//!
//! Full-stack integration tests that spin up a real PostgreSQL instance via
//! testcontainers, run migrations, build the Axum router, and exercise HTTP
//! endpoints end-to-end (including auth middleware).
//!
//! These tests require Docker to be running. They are ignored by default
//! and can be run with:
//!   cargo test -p fc-platform --test api_integration_tests -- --ignored
//!
//! Or run all (including ignored):
//!   cargo test -p fc-platform --test api_integration_tests -- --include-ignored

use axum::{body::Body, http::Request, Router};
use http_body_util::BodyExt; // for collect
use std::sync::Arc;
use tower::ServiceExt; // for oneshot

use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

use sea_orm::DatabaseConnection;
use serde_json::json;

use fc_platform::auth::auth_service::{AuthConfig, AuthService};
use fc_platform::domain::{Principal, UserScope};
use fc_platform::shared::database::{create_connection, run_migrations};
use fc_platform::{
    ClientRepository, DispatchJobRepository, EventRepository, RoleRepository,
};
use fc_platform::api::{
    clients_router, sdk_dispatch_jobs_batch_router, sdk_events_batch_router, AppState, AuthLayer,
    ClientsState, SdkDispatchJobsState, SdkEventsState,
};
use fc_platform::Client;
use fc_platform::AuthorizationService;

// ─── Test Helpers ──────────────────────────────────────────────────────────

/// Start a PostgreSQL testcontainer and return the database connection.
async fn setup_test_db() -> (DatabaseConnection, testcontainers::ContainerAsync<Postgres>) {
    let container = Postgres::default()
        .with_db_name("flowcatalyst_test")
        .with_user("test")
        .with_password("test")
        .start()
        .await
        .expect("Failed to start PostgreSQL container");

    let host = container.get_host().await.expect("Failed to get host");
    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("Failed to get port");

    let database_url = format!(
        "postgresql://test:test@{}:{}/flowcatalyst_test",
        host, port
    );

    let db = create_connection(&database_url)
        .await
        .expect("Failed to connect to test database");

    run_migrations(&db)
        .await
        .expect("Failed to run migrations");

    (db, container)
}

/// Returns an AuthService configured with a symmetric HS256 test key.
fn test_auth_service() -> AuthService {
    AuthService::new(AuthConfig {
        secret_key: "test-secret-key-for-integration-tests-minimum-32-chars!!".to_string(),
        issuer: "flowcatalyst".to_string(),
        audience: "flowcatalyst".to_string(),
        access_token_expiry_secs: 3600,
        session_token_expiry_secs: 28800,
        refresh_token_expiry_secs: 86400,
        rsa_private_key: None,
        rsa_public_key: None,
    })
}

/// Build a minimal Axum router wired to the test database, returning the
/// router and the AuthService (for token generation).
fn build_test_router(db: &DatabaseConnection) -> (Router, Arc<AuthService>) {
    let auth_service = Arc::new(test_auth_service());
    let role_repo = Arc::new(RoleRepository::new(db));
    let authz_service = Arc::new(AuthorizationService::new(role_repo));

    let app_state = AppState {
        auth_service: auth_service.clone(),
        authz_service,
    };

    let clients_state = ClientsState {
        client_repo: Arc::new(ClientRepository::new(db)),
        application_repo: None,
        application_client_config_repo: None,
        audit_service: None,
    };

    let sdk_events_state = SdkEventsState {
        event_repo: Arc::new(EventRepository::new(db)),
    };

    let sdk_dispatch_jobs_state = SdkDispatchJobsState {
        dispatch_job_repo: Arc::new(DispatchJobRepository::new(db)),
    };

    let router: Router = Router::new()
        .nest(
            "/api/admin/clients",
            Into::<Router>::into(clients_router(clients_state)),
        )
        .nest(
            "/api/sdk/events",
            sdk_events_batch_router(sdk_events_state),
        )
        .nest(
            "/api/sdk/dispatch-jobs",
            sdk_dispatch_jobs_batch_router(sdk_dispatch_jobs_state),
        )
        .layer(AuthLayer::new(app_state));

    (router, auth_service)
}

/// Create an anchor-scoped principal and generate an access token for it.
fn generate_anchor_token(auth_service: &AuthService) -> String {
    let principal = Principal::new_user("admin@flowcatalyst.local", UserScope::Anchor);
    auth_service
        .generate_access_token(&principal)
        .expect("Failed to generate access token")
}

// ─── Test Cases ────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_create_client_via_api() {
    let (db, _container) = setup_test_db().await;
    let (app, auth_service) = build_test_router(&db);
    let token = generate_anchor_token(&auth_service);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/admin/clients")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::from(
                    serde_json::to_string(&json!({
                        "identifier": "api-test",
                        "name": "API Test Client"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body)
        .unwrap_or_else(|_| panic!("Failed to parse response body: {:?}", body));

    assert_eq!(status.as_u16(), 200, "Expected 200 OK, got {} — body: {}", status, json);
    assert!(json.get("id").is_some(), "Response should contain an 'id' field");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_list_clients_via_api() {
    let (db, _container) = setup_test_db().await;
    let client_repo = ClientRepository::new(&db);

    // Seed a client directly via the repository
    let client = Client::new("List Test Client", "list-test");
    client_repo
        .insert(&client)
        .await
        .expect("Failed to insert test client");

    let (app, auth_service) = build_test_router(&db);
    let token = generate_anchor_token(&auth_service);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/admin/clients")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body)
        .unwrap_or_else(|_| panic!("Failed to parse response body: {:?}", body));

    assert_eq!(status.as_u16(), 200, "Expected 200 OK, got {} — body: {}", status, json);

    let clients = json["clients"].as_array().expect("Response should have a 'clients' array");
    assert!(
        clients.iter().any(|c| c["identifier"] == "list-test"),
        "Response should contain the seeded client"
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_get_client_by_id_via_api() {
    let (db, _container) = setup_test_db().await;
    let client_repo = ClientRepository::new(&db);

    // Seed a client directly via the repository
    let client = Client::new("Get By ID Client", "get-by-id");
    client_repo
        .insert(&client)
        .await
        .expect("Failed to insert test client");

    let (app, auth_service) = build_test_router(&db);
    let token = generate_anchor_token(&auth_service);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(&format!("/api/admin/clients/{}", client.id))
                .header("authorization", format!("Bearer {}", token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body)
        .unwrap_or_else(|_| panic!("Failed to parse response body: {:?}", body));

    assert_eq!(status.as_u16(), 200, "Expected 200 OK, got {} — body: {}", status, json);
    assert_eq!(json["name"], "Get By ID Client", "Response name should match");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_unauthorized_request() {
    let (db, _container) = setup_test_db().await;
    let (app, _auth_service) = build_test_router(&db);

    // Send a request WITHOUT an Authorization header
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/admin/clients")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    assert_eq!(status.as_u16(), 401, "Expected 401 Unauthorized, got {}", status);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_batch_events_via_api() {
    let (db, _container) = setup_test_db().await;
    let (app, auth_service) = build_test_router(&db);
    let token = generate_anchor_token(&auth_service);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sdk/events/batch")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::from(
                    serde_json::to_string(&json!({
                        "items": [
                            { "type": "orders:created", "source": "test", "data": {"orderId": "1"} },
                            { "type": "orders:updated", "source": "test", "data": {"orderId": "2"} },
                            { "type": "orders:shipped", "source": "test", "data": {"orderId": "3"} }
                        ]
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body)
        .unwrap_or_else(|_| panic!("Failed to parse response body: {:?}", body));

    assert_eq!(status.as_u16(), 200, "Expected 200 OK, got {} — body: {}", status, json);

    let results = json["results"].as_array().expect("Response should have a 'results' array");
    assert_eq!(results.len(), 3, "Should have 3 results");
    for result in results {
        assert_eq!(result["status"], "SUCCESS", "Each result should have SUCCESS status");
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_batch_events_exceeds_limit() {
    let (db, _container) = setup_test_db().await;
    let (app, auth_service) = build_test_router(&db);
    let token = generate_anchor_token(&auth_service);

    // Build a batch with 101 items (exceeds the 100-item limit)
    let items: Vec<serde_json::Value> = (0..101)
        .map(|i| {
            json!({
                "type": format!("test:event:{}", i),
                "source": "test"
            })
        })
        .collect();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sdk/events/batch")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::from(
                    serde_json::to_string(&json!({ "items": items })).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    assert_eq!(status.as_u16(), 400, "Expected 400 Bad Request for >100 items, got {}", status);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_batch_dispatch_jobs_via_api() {
    let (db, _container) = setup_test_db().await;
    let (app, auth_service) = build_test_router(&db);
    let token = generate_anchor_token(&auth_service);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sdk/dispatch-jobs/batch")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::from(
                    serde_json::to_string(&json!({
                        "jobs": [
                            {
                                "code": "orders:fulfillment:shipment:shipped",
                                "targetUrl": "https://example.com/webhook1",
                                "payload": "{\"orderId\":\"1\"}",
                                "serviceAccountId": "svc-test-001"
                            },
                            {
                                "code": "orders:fulfillment:shipment:delivered",
                                "targetUrl": "https://example.com/webhook2",
                                "payload": "{\"orderId\":\"2\"}",
                                "serviceAccountId": "svc-test-002"
                            }
                        ]
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body)
        .unwrap_or_else(|_| panic!("Failed to parse response body: {:?}", body));

    assert_eq!(status.as_u16(), 200, "Expected 200 OK, got {} — body: {}", status, json);
    assert_eq!(json["count"], 2, "Response should report count=2");

    let jobs = json["jobs"].as_array().expect("Response should have a 'jobs' array");
    assert_eq!(jobs.len(), 2, "Should have 2 job responses");
}
