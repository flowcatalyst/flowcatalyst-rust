//! Go's F9 follow-up (`45537af`, docs/platform-bug-fixes.md in
//! flowcatalyst-go): older seeds catalogued 15 event types nothing emits
//! here since `cb1b3e4f`. Migration 069 (Go's 071) removes them with their
//! schema versions; a subscription binding naming one of them, or pointing at
//! one of the rows, moves to `msg_subscription_event_types_archive` as it
//! was (not re-pointed); a wildcard binding, a binding to an emitted type and
//! stored events stay; the seed does not add the types back; running it
//! again changes nothing. Requires Docker (or `FC_TEST_PG_BIN`).

use sqlx::PgPool;

use crate::support::TestApp;
use fc_platform::shared::database;

const MIGRATION: &str = include_str!("../../../../migrations/069_prune_unemitted_event_types.sql");

/// Run the migration's SQL in one transaction, as the runner does.
async fn migrate(pool: &PgPool) {
    let mut tx = pool.begin().await.expect("begin");
    sqlx::raw_sql(MIGRATION)
        .execute(&mut *tx)
        .await
        .expect("run migration 069");
    tx.commit().await.expect("commit");
}

/// Insert `code` as an event type with a 1.0 schema; returns its id.
async fn event_type(pool: &PgPool, n: usize, code: &str) -> String {
    let parts: Vec<&str> = code.split(':').collect();
    let id = format!("etc_p069{n:09}");
    sqlx::query(
        "INSERT INTO msg_event_types
           (id, code, name, status, source, client_scoped, application, subdomain, aggregate,
            created_at, updated_at)
         VALUES ($1, $2, $2, 'CURRENT', 'UI', false, $3, $4, $5, NOW(), NOW())",
    )
    .bind(&id)
    .bind(code)
    .bind(parts[0])
    .bind(parts[1])
    .bind(parts[2])
    .execute(pool)
    .await
    .expect("insert event type");
    sqlx::query(
        "INSERT INTO msg_event_type_spec_versions
           (id, event_type_id, version, mime_type, schema_content, schema_type, status)
         VALUES ($1, $2, '1.0', 'application/schema+json', '{}'::jsonb, 'JSON_SCHEMA', 'CURRENT')",
    )
    .bind(format!("sch_p069{n:09}"))
    .bind(&id)
    .execute(pool)
    .await
    .expect("insert schema");
    id
}

/// A binding of `sub` to `code` (and the row `type_id`); returns its id.
async fn bind(pool: &PgPool, sub: &str, type_id: Option<&str>, code: &str) -> i32 {
    sqlx::query_scalar(
        "INSERT INTO msg_subscription_event_types (subscription_id, event_type_id, event_type_code, spec_version)
         VALUES ($1, $2, $3, '1.0') RETURNING id",
    )
    .bind(sub)
    .bind(type_id)
    .bind(code)
    .fetch_one(pool)
    .await
    .expect("binding")
}

async fn codes(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar("SELECT code FROM msg_event_types ORDER BY code")
        .fetch_all(pool)
        .await
        .expect("codes")
}

type Binding = (i32, String, Option<String>, String, Option<String>);

/// A binding and the reason it was archived.
type ArchivedRow = (i32, String, Option<String>, String, Option<String>, String);

async fn bindings(pool: &PgPool) -> Vec<Binding> {
    sqlx::query_as(
        "SELECT id, subscription_id, event_type_id, event_type_code, spec_version
           FROM msg_subscription_event_types ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .expect("bindings")
}

async fn archived(pool: &PgPool) -> Vec<(Binding, String)> {
    let rows: Vec<ArchivedRow> = sqlx::query_as(
        "SELECT id, subscription_id, event_type_id, event_type_code, spec_version, archived_reason
           FROM msg_subscription_event_types_archive ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .expect("archive");
    rows.into_iter()
        .map(|(id, s, t, c, v, r)| ((id, s, t, c, v), r))
        .collect()
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn migration_069_removes_the_unemitted_types_and_archives_their_bindings() {
    let app = TestApp::setup().await;
    let pool = &app.pool;

    // What an older seed wrote: two dead types (with schemas) and an emitted one.
    let role = event_type(pool, 1, "platform:iam:role:created").await;
    let idp = event_type(pool, 2, "platform:admin:idp:updated").await;
    let emitted = event_type(pool, 3, "platform:admin:role:created").await;

    // Bindings: to a dead row (by row and code), to a dead code alone, a
    // wildcard, and to the emitted type.
    let by_row = bind(
        pool,
        "sub_p069000000001",
        Some(&role),
        "platform:iam:role:created",
    )
    .await;
    let by_code = bind(
        pool,
        "sub_p069000000002",
        None,
        "platform:admin:idp:updated",
    )
    .await;
    let wildcard = bind(pool, "sub_p069000000003", None, "platform:iam:role:*").await;
    let live = bind(
        pool,
        "sub_p069000000001",
        Some(&emitted),
        "platform:admin:role:created",
    )
    .await;
    let before = bindings(pool).await;

    migrate(pool).await;

    // The dead types and their schemas are gone; the emitted one stays.
    let after_codes = codes(pool).await;
    assert_eq!(after_codes, vec!["platform:admin:role:created"]);
    let schemas: Vec<String> = sqlx::query_scalar(
        "SELECT event_type_id FROM msg_event_type_spec_versions ORDER BY event_type_id",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    assert_eq!(schemas, vec![emitted.clone()]);
    assert!(!schemas.contains(&idp));

    // The two dead bindings are archived exactly as they were, under their
    // ids; the wildcard and the live binding stay.
    let kept = bindings(pool).await;
    let kept_ids: Vec<i32> = kept.iter().map(|b| b.0).collect();
    assert_eq!(kept_ids, vec![wildcard, live]);
    let archive = archived(pool).await;
    let expected: Vec<(Binding, String)> = before
        .iter()
        .filter(|b| b.0 == by_row || b.0 == by_code)
        .map(|b| (b.clone(), "071_event_type_not_emitted".to_string()))
        .collect();
    assert_eq!(archive, expected);

    // The seed does not add them back.
    database::seed_platform_event_types(pool)
        .await
        .expect("seed");
    let seeded = codes(pool).await;
    for dead in [
        "platform:iam:role:created",
        "platform:iam:roles:synced",
        "platform:iam:anchor-domain:created",
        "platform:iam:auth-config:updated",
        "platform:admin:idp:updated",
        "platform:admin:edm:deleted",
    ] {
        assert!(!seeded.contains(&dead.to_string()), "{dead} came back");
    }
    // ... while the emitted password-reset request (pruned by Go) is kept.
    assert!(seeded.contains(&"platform:iam:user:password-reset-requested".to_string()));

    // Running the migration again changes nothing.
    let (types_before, bindings_before) = (codes(pool).await, bindings(pool).await);
    migrate(pool).await;
    assert_eq!(codes(pool).await, types_before);
    assert_eq!(bindings(pool).await, bindings_before);
    assert_eq!(archived(pool).await, archive);
}
