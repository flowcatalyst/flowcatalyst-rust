//! PostgreSQL Integration Tests
//!
//! Full-stack integration tests using testcontainers to spin up a real
//! PostgreSQL instance, run migrations, and test repository + API operations.
//!
//! These tests require Docker to be running. They are ignored by default
//! and can be run with:
//!   cargo test -p fc-platform --test postgres_integration_tests -- --ignored
//!
//! Or run all (including ignored):
//!   cargo test -p fc-platform --test postgres_integration_tests -- --include-ignored

use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

use sea_orm::DatabaseConnection;

use fc_platform::auth::auth_service::{AuthConfig, AuthService};
use fc_platform::domain::{Principal, UserScope};
use fc_platform::shared::database::{create_connection, run_migrations};
use fc_platform::{
    ClientRepository, PrincipalRepository, RoleRepository,
    EventTypeRepository, ApplicationRepository,
    AuditLogRepository, EventRepository,
};
use fc_platform::{Client, ClientStatus, AuthRole, Application, EventType, AuditLog, Event};

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
    let port = container.get_host_port_ipv4(5432).await.expect("Failed to get port");

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

// ─── Client Repository Tests ──────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_client_crud() {
    let (db, _container) = setup_test_db().await;
    let repo = ClientRepository::new(&db);

    // Create
    let client = Client::new("Acme Corp", "acme-corp");
    repo.insert(&client).await.expect("Failed to insert client");

    // Read
    let found = repo.find_by_id(&client.id).await.expect("Failed to find client");
    assert!(found.is_some());
    let found = found.unwrap();
    assert_eq!(found.name, "Acme Corp");
    assert_eq!(found.identifier, "acme-corp");
    assert_eq!(found.status, ClientStatus::Active);

    // Find by identifier
    let by_ident = repo.find_by_identifier("acme-corp").await.expect("Failed to find by identifier");
    assert!(by_ident.is_some());
    assert_eq!(by_ident.unwrap().id, client.id);

    // List active
    let active = repo.find_active().await.expect("Failed to find active");
    assert!(active.iter().any(|c| c.id == client.id));

    // Update (suspend)
    let mut updated_client = found;
    updated_client.suspend("Testing suspension");
    repo.update(&updated_client).await.expect("Failed to update client");

    let suspended = repo.find_by_id(&client.id).await.unwrap().unwrap();
    assert_eq!(suspended.status, ClientStatus::Suspended);
    assert_eq!(suspended.status_reason, Some("Testing suspension".to_string()));
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_client_not_found() {
    let (db, _container) = setup_test_db().await;
    let repo = ClientRepository::new(&db);

    let result = repo.find_by_id("nonexistent-id").await.expect("Query should succeed");
    assert!(result.is_none());
}

// ─── Principal Repository Tests ───────────────────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_principal_user_crud() {
    let (db, _container) = setup_test_db().await;
    let repo = PrincipalRepository::new(&db);

    // Create user principal
    let mut principal = Principal::new_user("alice@example.com", UserScope::Anchor);
    principal.assign_role("admin");
    repo.insert(&principal).await.expect("Failed to insert principal");

    // Read
    let found = repo.find_by_id(&principal.id).await.expect("Failed to find principal");
    assert!(found.is_some());
    let found = found.unwrap();
    assert_eq!(found.name, "alice@example.com");
    assert_eq!(found.scope, UserScope::Anchor);
    assert!(found.active);
    assert!(found.is_user());

    // Find by email
    let by_email = repo.find_by_email("alice@example.com").await.expect("Failed to find by email");
    assert!(by_email.is_some());
    assert_eq!(by_email.unwrap().id, principal.id);

    // Deactivate
    let mut p = found;
    p.deactivate();
    repo.update(&p).await.expect("Failed to update principal");

    let deactivated = repo.find_by_id(&principal.id).await.unwrap().unwrap();
    assert!(!deactivated.active);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_principal_service_account() {
    let (db, _container) = setup_test_db().await;
    let repo = PrincipalRepository::new(&db);

    let principal = Principal::new_service("svc-abc", "My Service");
    repo.insert(&principal).await.expect("Failed to insert service principal");

    let found = repo.find_by_id(&principal.id).await.unwrap().unwrap();
    assert!(found.is_service());
    assert_eq!(found.name, "My Service");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_principal_with_client_access() {
    let (db, _container) = setup_test_db().await;
    let client_repo = ClientRepository::new(&db);
    let principal_repo = PrincipalRepository::new(&db);

    // Create a client first
    let client = Client::new("Test Client", "test-client");
    client_repo.insert(&client).await.expect("Failed to insert client");

    // Create a principal with client scope
    let principal = Principal::new_user("user@test-client.com", UserScope::Client)
        .with_client_id(&client.id);
    principal_repo.insert(&principal).await.expect("Failed to insert principal");

    let found = principal_repo.find_by_id(&principal.id).await.unwrap().unwrap();
    assert_eq!(found.client_id, Some(client.id.clone()));
    assert_eq!(found.scope, UserScope::Client);
    assert!(found.can_access_client(&client.id));
}

// ─── Role Repository Tests ───────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_role_crud() {
    let (db, _container) = setup_test_db().await;
    let repo = RoleRepository::new(&db);

    // AuthRole::new takes (application_code, role_name, display_name)
    let role = AuthRole::new("platform", "test-admin", "Test Admin");
    repo.insert(&role).await.expect("Failed to insert role");

    let found = repo.find_by_code(&role.name).await.expect("Failed to find role");
    assert!(found.is_some());
    let found = found.unwrap();
    assert_eq!(found.display_name, "Test Admin");

    // Find by codes
    let roles = repo.find_by_codes(&[role.name.clone()]).await.expect("Failed to find roles");
    assert_eq!(roles.len(), 1);
}

// ─── Event Type Repository Tests ──────────────────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_event_type_crud() {
    let (db, _container) = setup_test_db().await;
    let repo = EventTypeRepository::new(&db);

    let event_type = EventType::new("orders:fulfillment:shipment:shipped", "Shipment Shipped")
        .expect("Failed to create event type");
    repo.insert(&event_type).await.expect("Failed to insert event type");

    let found = repo.find_by_id(&event_type.id).await.expect("Failed to find event type");
    assert!(found.is_some());
    let found = found.unwrap();
    assert_eq!(found.name, "Shipment Shipped");
    assert_eq!(found.code, "orders:fulfillment:shipment:shipped");

    // Find by code
    let by_code = repo.find_by_code("orders:fulfillment:shipment:shipped").await.expect("Failed to find by code");
    assert!(by_code.is_some());
    assert_eq!(by_code.unwrap().id, event_type.id);
}

// ─── Application Repository Tests ─────────────────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_application_crud() {
    let (db, _container) = setup_test_db().await;
    let repo = ApplicationRepository::new(&db);

    let app = Application::new("my-app", "My Application");
    repo.insert(&app).await.expect("Failed to insert application");

    let found = repo.find_by_id(&app.id).await.expect("Failed to find application");
    assert!(found.is_some());
    let found = found.unwrap();
    assert_eq!(found.code, "my-app");
    assert_eq!(found.name, "My Application");

    // Find by code
    let by_code = repo.find_by_code("my-app").await.expect("Failed to find by code");
    assert!(by_code.is_some());
    assert_eq!(by_code.unwrap().id, app.id);
}

// ─── Audit Log Repository Tests ───────────────────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_audit_log_insert_and_query() {
    let (db, _container) = setup_test_db().await;
    let repo = AuditLogRepository::new(&db);

    // AuditLog::new(entity_type, entity_id, operation, operation_json, principal_id)
    let log = AuditLog::new(
        "Client",
        "client-123",
        "CreateClient",
        None,
        Some("principal-456".to_string()),
    );
    repo.insert(&log).await.expect("Failed to insert audit log");

    let found = repo.find_by_id(&log.id).await.expect("Failed to find audit log");
    assert!(found.is_some());
    let found = found.unwrap();
    assert_eq!(found.entity_type, "Client");
    assert_eq!(found.entity_id, "client-123");
    assert_eq!(found.operation, "CreateClient");
}

// ─── Event Repository Tests ───────────────────────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_event_insert_and_query() {
    let (db, _container) = setup_test_db().await;
    let repo = EventRepository::new(&db);

    // Event::new(event_type, source, data)
    let event = Event::new(
        "platform:client:created",
        "platform:tenant",
        serde_json::json!({"clientId": "test-123"}),
    );
    repo.insert(&event).await.expect("Failed to insert event");

    let found = repo.find_by_id(&event.id).await.expect("Failed to find event");
    assert!(found.is_some());
    let found = found.unwrap();
    assert_eq!(found.event_type, "platform:client:created");
    assert_eq!(found.source, "platform:tenant");
}

// ─── Token Round-Trip with DB-Backed Principal ────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_token_generation_from_db_principal() {
    let (db, _container) = setup_test_db().await;
    let principal_repo = PrincipalRepository::new(&db);
    let auth_service = test_auth_service();

    // Create and persist a principal
    let mut principal = Principal::new_user("admin@flowcatalyst.local", UserScope::Anchor);
    principal.assign_role("platform-admin");
    principal_repo.insert(&principal).await.expect("Failed to insert principal");

    // Load from DB
    let loaded = principal_repo.find_by_id(&principal.id).await.unwrap().unwrap();

    // Generate token from DB-loaded principal
    let token = auth_service.generate_access_token(&loaded).expect("Failed to generate token");

    // Validate token
    let claims = auth_service.validate_token(&token).expect("Failed to validate token");
    assert_eq!(claims.sub, principal.id);
    assert_eq!(claims.email, Some("admin@flowcatalyst.local".to_string()));
    assert_eq!(claims.scope, "ANCHOR");
    assert!(claims.clients.contains(&"*".to_string()));
}

// ─── Multiple Clients with Principal Access ───────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_multiple_clients_with_partner_principal() {
    let (db, _container) = setup_test_db().await;
    let client_repo = ClientRepository::new(&db);
    let principal_repo = PrincipalRepository::new(&db);

    // Create two clients
    let client1 = Client::new("Client Alpha", "alpha");
    let client2 = Client::new("Client Beta", "beta");
    client_repo.insert(&client1).await.unwrap();
    client_repo.insert(&client2).await.unwrap();

    // Create partner with access to both
    let mut principal = Principal::new_user("partner@example.com", UserScope::Partner);
    principal.grant_client_access(&client1.id);
    principal.grant_client_access(&client2.id);
    principal_repo.insert(&principal).await.unwrap();

    // Verify access
    let loaded = principal_repo.find_by_id(&principal.id).await.unwrap().unwrap();

    // Generate token
    let auth_service = test_auth_service();
    let token = auth_service.generate_access_token(&loaded).unwrap();
    let claims = auth_service.validate_token(&token).unwrap();

    assert_eq!(claims.scope, "PARTNER");
}

// ─── Migration Idempotency Test ───────────────────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_migrations_are_idempotent() {
    let (db, _container) = setup_test_db().await;

    // Run migrations again — should succeed (IF NOT EXISTS)
    run_migrations(&db).await.expect("Second migration run should succeed");

    // Run a third time for good measure
    run_migrations(&db).await.expect("Third migration run should succeed");
}

// ─── Cross-Repository Transaction Test ────────────────────────────────────

#[tokio::test]
#[ignore = "requires Docker"]
async fn test_unit_of_work_commit() {
    let (db, _container) = setup_test_db().await;
    let client_repo = ClientRepository::new(&db);

    use fc_platform::{PgUnitOfWork, UnitOfWork};
    use fc_platform::client::operations::events::ClientCreated;
    use fc_platform::usecase::ExecutionContext;

    // Create a client
    let client = Client::new("UoW Test Client", "uow-test");
    client_repo.insert(&client).await.expect("Failed to insert client");

    // Commit an event via UnitOfWork
    let uow = PgUnitOfWork::new(db.clone());
    let ctx = ExecutionContext::create("test-principal-id");
    let event = ClientCreated::new(&ctx, &client.id, &client.name, &client.identifier, None);

    #[derive(serde::Serialize)]
    struct CreateClientCommand { name: String }
    let command = CreateClientCommand { name: "UoW Test Client".to_string() };

    let result = uow.commit(&client, event, &command).await;
    assert!(result.into_result().is_ok(), "UnitOfWork commit should succeed");

    // Verify event was persisted (use find_by_type since find_all doesn't exist)
    let event_repo = EventRepository::new(&db);
    let events = event_repo.find_by_type("platform:iam:client:created", 10).await
        .expect("Failed to query events");
    assert!(!events.is_empty(), "At least one event should exist");

    // Verify audit log was persisted
    let audit_repo = AuditLogRepository::new(&db);
    let logs = audit_repo.find_by_entity("Client", &client.id, 10).await
        .expect("Failed to query audit logs");
    assert!(!logs.is_empty(), "At least one audit log should exist");
}
