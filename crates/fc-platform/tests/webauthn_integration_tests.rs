//! WebAuthn Integration Tests
//!
//! Exercise the parts of the passkey flow that don't require a real
//! browser / authenticator ceremony: migration sanity, the per-principal
//! gate against a real database, and cascade-delete behaviour.
//!
//! Full register/authenticate ceremony testing requires synthesising a
//! `Passkey` (which `webauthn-rs` exposes only behind a `danger-*` feature)
//! or driving a browser; that lives in the end-to-end suite, not here.
//!
//! Requires Docker. Run with:
//!   cargo test -p fc-platform --test webauthn_integration_tests -- --ignored

use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

use fc_platform::shared::database::{create_pool, run_migrations, MigrationProfile};
use fc_platform::webauthn::gate::ensure_internal_principal;
use fc_platform::{Principal, PrincipalRepository, UserScope};

async fn setup_test_db() -> (sqlx::PgPool, testcontainers::ContainerAsync<Postgres>) {
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
    let url = format!("postgresql://test:test@{}:{}/flowcatalyst_test", host, port);
    let pool = create_pool(&url).await.expect("Failed to connect");
    run_migrations(&pool, MigrationProfile::Production)
        .await
        .expect("Failed to run migrations");
    (pool, container)
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn migration_creates_webauthn_credentials_table_with_expected_columns() {
    let (pool, _c) = setup_test_db().await;

    let columns: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT column_name::text, data_type::text, is_nullable::text
           FROM information_schema.columns
          WHERE table_name = 'webauthn_credentials'
          ORDER BY ordinal_position",
    )
    .fetch_all(&pool)
    .await
    .expect("query columns");

    let names: Vec<&str> = columns.iter().map(|(n, _, _)| n.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "id",
            "principal_id",
            "credential_id",
            "passkey_data",
            "name",
            "created_at",
            "last_used_at",
        ]
    );

    // Spot-check a few critical types/nullabilities.
    let by_name: std::collections::HashMap<_, _> = columns
        .iter()
        .map(|(n, t, nullable)| (n.as_str(), (t.as_str(), nullable.as_str())))
        .collect();
    assert_eq!(by_name["id"].1, "NO");
    assert_eq!(by_name["principal_id"].1, "NO");
    assert_eq!(by_name["credential_id"].0, "bytea");
    assert_eq!(by_name["passkey_data"].0, "jsonb");
    assert_eq!(by_name["last_used_at"].1, "YES");
}

/// Insert a USER principal whose `user_identity` matches the requested
/// auth shape. Returns the inserted principal so the caller can use its
/// email or id.
async fn insert_principal_with_identity(
    repo: &PrincipalRepository,
    email: &str,
    password_hash: Option<&str>,
    external_id: Option<&str>,
) -> Principal {
    let mut principal = Principal::new_user(email, UserScope::Anchor);
    let identity = principal
        .user_identity
        .as_mut()
        .expect("USER principal must have user_identity");
    identity.password_hash = password_hash.map(String::from);
    identity.external_id = external_id.map(String::from);
    repo.insert(&principal).await.expect("insert principal");
    principal
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn gate_allows_principal_with_password_and_no_external_id() {
    let (pool, _c) = setup_test_db().await;
    let principal_repo = PrincipalRepository::new(&pool);

    // Local-auth user: password hash present, no external_id. Gate passes.
    insert_principal_with_identity(
        &principal_repo,
        "alice@example.com",
        Some("argon2id$dummy"),
        None,
    )
    .await;

    ensure_internal_principal("alice@example.com", &principal_repo)
        .await
        .expect("internal-auth principal should be allowed");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn gate_rejects_principal_with_external_id() {
    let (pool, _c) = setup_test_db().await;
    let principal_repo = PrincipalRepository::new(&pool);

    // Federated user: linked to an external IdP. Even if a stale password
    // hash exists, the presence of an external_id locks out passkeys.
    insert_principal_with_identity(
        &principal_repo,
        "bob@example.com",
        Some("argon2id$dummy"),
        Some("idp-subject-123"),
    )
    .await;

    let err = ensure_internal_principal("bob@example.com", &principal_repo)
        .await
        .expect_err("federated principal should be rejected");

    let resp_kind = format!("{:?}", err);
    assert!(
        resp_kind.contains("Validation"),
        "expected validation/bad_request, got: {}",
        resp_kind
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn gate_rejects_principal_without_password_hash() {
    let (pool, _c) = setup_test_db().await;
    let principal_repo = PrincipalRepository::new(&pool);

    // Newly-created user that hasn't set a password yet — internal-auth
    // path requires a password hash before passkey registration is offered.
    insert_principal_with_identity(&principal_repo, "carol@example.com", None, None).await;

    assert!(
        ensure_internal_principal("carol@example.com", &principal_repo)
            .await
            .is_err(),
        "password-less principal should be rejected"
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn gate_rejects_unknown_email() {
    let (pool, _c) = setup_test_db().await;
    let principal_repo = PrincipalRepository::new(&pool);

    // No principal for this email. The gate returns the same error shape
    // as the federated case so the caller's enumeration-defence wrapper
    // (which collapses any error into "no credentials") can't distinguish.
    assert!(
        ensure_internal_principal("nobody@example.com", &principal_repo)
            .await
            .is_err(),
        "unknown email should be rejected"
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn webauthn_credentials_cascade_when_principal_deleted() {
    let (pool, _c) = setup_test_db().await;

    // Create a principal directly via repo.
    let principal_repo = PrincipalRepository::new(&pool);
    let principal = Principal::new_user("alice@example.com", UserScope::Anchor);
    principal_repo
        .insert(&principal)
        .await
        .expect("insert principal");

    // Insert a stub webauthn_credentials row pointing at this principal.
    // (We bypass the entity here because constructing a real Passkey requires
    // a full ceremony — the FK + CASCADE behaviour is what we're verifying.)
    sqlx::query(
        "INSERT INTO webauthn_credentials
            (id, principal_id, credential_id, passkey_data, name, created_at)
         VALUES ($1, $2, $3, $4::jsonb, $5, NOW())",
    )
    .bind("pkc_TESTCREDENTIA")
    .bind(&principal.id)
    .bind(&[1u8, 2, 3, 4][..])
    .bind(r#"{"placeholder": true}"#)
    .bind("Test Key")
    .execute(&pool)
    .await
    .expect("insert credential row");

    // Sanity check before delete.
    let count_before: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM webauthn_credentials WHERE principal_id = $1")
            .bind(&principal.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count_before.0, 1);

    // Delete the principal — cascade should remove the credential.
    sqlx::query("DELETE FROM iam_principals WHERE id = $1")
        .bind(&principal.id)
        .execute(&pool)
        .await
        .expect("delete principal");

    let count_after: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM webauthn_credentials WHERE principal_id = $1")
            .bind(&principal.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        count_after.0, 0,
        "credential should have cascaded with principal"
    );
}
