//! WebAuthn Integration Tests
//!
//! Exercise the parts of the passkey flow that don't require a real
//! browser / authenticator ceremony: migration sanity, the email-domain
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

use fc_platform::email_domain_mapping::entity::ScopeType;
use fc_platform::identity_provider::entity::{IdentityProvider, IdentityProviderType};
use fc_platform::shared::database::{create_pool, run_migrations, MigrationProfile};
use fc_platform::webauthn::gate::ensure_internal_auth;
use fc_platform::{EmailDomainMapping, EmailDomainMappingRepository, IdentityProviderRepository};

/// An identity provider of `idp_type`, stored; its id.
async fn identity_provider(
    pool: &sqlx::PgPool,
    code: &str,
    idp_type: IdentityProviderType,
) -> String {
    let idp = IdentityProvider::new(code, code, idp_type);
    IdentityProviderRepository::new(pool)
        .insert(&idp)
        .await
        .expect("insert identity provider");
    idp.id
}

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

#[tokio::test]
#[ignore = "requires Docker"]
async fn gate_allows_internal_domain_with_no_mapping() {
    let (pool, _c) = setup_test_db().await;
    let edm_repo = EmailDomainMappingRepository::new(&pool);

    // No mapping for example.com — gate must pass.
    ensure_internal_auth("alice@example.com", &edm_repo)
        .await
        .expect("internal-auth domain should be allowed");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn gate_rejects_federated_domain() {
    let (pool, _c) = setup_test_db().await;
    let edm_repo = EmailDomainMappingRepository::new(&pool);

    let oidc = identity_provider(&pool, "entra", IdentityProviderType::Oidc).await;
    let mapping = EmailDomainMapping::new("federated.com", &oidc, ScopeType::Anchor);
    edm_repo.insert(&mapping).await.expect("insert mapping");

    let err = ensure_internal_auth("user@federated.com", &edm_repo)
        .await
        .expect_err("federated domain should be rejected");

    // Should map to a 4xx, not a 5xx.
    let resp_kind = format!("{:?}", err);
    assert!(
        resp_kind.contains("Validation"),
        "expected validation/bad_request, got: {}",
        resp_kind
    );
}

/// Go's `fcdev init` maps the anchor domain to an INTERNAL provider; its
/// users keep passkeys (only an OIDC mapping federates a domain).
#[tokio::test]
#[ignore = "requires Docker"]
async fn gate_allows_a_domain_mapped_to_an_internal_provider() {
    let (pool, _c) = setup_test_db().await;
    let edm_repo = EmailDomainMappingRepository::new(&pool);

    let internal = identity_provider(&pool, "internal", IdentityProviderType::Internal).await;
    let mapping = EmailDomainMapping::new("anchor.com", &internal, ScopeType::Anchor);
    edm_repo.insert(&mapping).await.expect("insert mapping");

    ensure_internal_auth("admin@anchor.com", &edm_repo)
        .await
        .expect("a domain mapped to an INTERNAL provider keeps passkeys");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn gate_normalises_domain_case_when_matching_mapping() {
    let (pool, _c) = setup_test_db().await;
    let edm_repo = EmailDomainMappingRepository::new(&pool);

    // Mappings are stored lowercased (see EmailDomainMapping::new); the gate
    // must lowercase the email domain before lookup so mixed-case email
    // inputs aren't mistakenly treated as internal.
    let oidc = identity_provider(&pool, "okta", IdentityProviderType::Oidc).await;
    let mapping = EmailDomainMapping::new("acme.com", &oidc, ScopeType::Anchor);
    edm_repo.insert(&mapping).await.expect("insert mapping");

    assert!(ensure_internal_auth("USER@ACME.COM", &edm_repo)
        .await
        .is_err());
    assert!(ensure_internal_auth("user@Acme.Com", &edm_repo)
        .await
        .is_err());
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn webauthn_credentials_cascade_when_principal_deleted() {
    use fc_platform::{Principal, PrincipalRepository, UserScope};
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
