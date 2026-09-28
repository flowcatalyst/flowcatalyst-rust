//! Owner decision #48 (2026-09-28): Rust's function registry is `fnr_*`;
//! Java keeps `fn_*` and Go's function runner (its migration 059) has its
//! own, incompatible `fn_*`. The runner retires Rust's 034, 037 and 056 (the
//! migrations that created and altered Rust's `fn_*`) and 062 creates
//! `fnr_*` in their end state.
//!
//! - Go's 059 (`tests/data/go/059_functions.sql`, copied from
//!   flowcatalyst-go) and Rust's migrations run in either order on one
//!   database, and neither fails; Go's `fn_*` keep Go's shape.
//! - A database that applied the old 034/037/056 (their rows recorded the
//!   way the runner records them) migrates with no drift error, keeps those
//!   rows and its `fn_*` tables untouched, and gains `fnr_*`, whose schema
//!   is exactly the retired migrations' end state renamed. No row is copied.
//!
//! Requires Docker.

use sqlx::PgPool;
use testcontainers::runners::AsyncRunner;
use testcontainers::ContainerAsync;
use testcontainers_modules::postgres::Postgres;

use fc_platform::shared::database::{create_pool, run_migrations, MigrationProfile};

const GO_059: &str = include_str!("data/go/059_functions.sql");
const RETIRED: [(&str, &str); 3] = [
    (
        "034_functions",
        include_str!("../../../migrations/034_functions.sql"),
    ),
    (
        "037_function_component_runtime",
        include_str!("../../../migrations/037_function_component_runtime.sql"),
    ),
    (
        "056_function_js_runtime",
        include_str!("../../../migrations/056_function_js_runtime.sql"),
    ),
];

async fn fresh_postgres() -> (PgPool, ContainerAsync<Postgres>) {
    let container = Postgres::default()
        .with_db_name("flowcatalyst_test")
        .with_user("test")
        .with_password("test")
        .start()
        .await
        .expect("start PostgreSQL");
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(5432).await.unwrap();
    let pool = create_pool(&format!(
        "postgresql://test:test@{host}:{port}/flowcatalyst_test"
    ))
    .await
    .expect("connect");
    (pool, container)
}

async fn migrate(pool: &PgPool) {
    run_migrations(pool, MigrationProfile::Production)
        .await
        .expect("Rust migrations");
}

/// Go's 059, Up section only (goose runs Down on rollback, never here).
async fn apply_go_059(pool: &PgPool) {
    let up = GO_059
        .split("-- +goose Down")
        .next()
        .expect("an Up section");
    sqlx::raw_sql(up).execute(pool).await.expect("Go's 059");
}

async fn exec(pool: &PgPool, sql: &str) {
    sqlx::raw_sql(sql).execute(pool).await.expect(sql);
}

async fn strings(pool: &PgPool, sql: &str) -> Vec<String> {
    sqlx::query_as::<_, (String,)>(sql)
        .fetch_all(pool)
        .await
        .expect(sql)
        .into_iter()
        .map(|(s,)| s)
        .collect()
}

async fn columns(pool: &PgPool, table: &str) -> Vec<String> {
    sqlx::query_as::<_, (String,)>(
        "SELECT column_name::text FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name = $1 ORDER BY ordinal_position",
    )
    .bind(table)
    .fetch_all(pool)
    .await
    .unwrap()
    .into_iter()
    .map(|(c,)| c)
    .collect()
}

async fn source_check(pool: &PgPool) -> String {
    let (def,): (String,) = sqlx::query_as(
        "SELECT pg_get_constraintdef(oid) FROM pg_constraint \
         WHERE conname = 'chk_msg_subscriptions_source'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    def
}

/// Go's `fn_*` are exactly the six tables of its 059, in Go's shape.
async fn assert_go_fn_tables(pool: &PgPool) {
    assert_eq!(
        strings(
            pool,
            "SELECT table_name::text FROM information_schema.tables \
             WHERE table_schema = 'public' AND table_name LIKE 'fn\\_%' ORDER BY table_name"
        )
        .await,
        [
            "fn_aliases",
            "fn_functions",
            "fn_pool_revisions",
            "fn_runners",
            "fn_settings",
            "fn_versions"
        ]
    );
    let go_functions = columns(pool, "fn_functions").await;
    assert!(
        go_functions.contains(&"address".to_string()),
        "{go_functions:?}"
    );
    assert!(!go_functions.contains(&"service_name".to_string()));
    let go_versions = columns(pool, "fn_versions").await;
    assert!(
        go_versions.contains(&"status".to_string()),
        "{go_versions:?}"
    );
    assert!(!go_versions.contains(&"state".to_string()));
}

/// Rust's registry is `fnr_*`, in Rust's shape, and writable.
async fn assert_rust_fnr_tables(pool: &PgPool) {
    assert_eq!(
        strings(
            pool,
            "SELECT table_name::text FROM information_schema.tables \
             WHERE table_schema = 'public' AND table_name LIKE 'fnr\\_%' ORDER BY table_name"
        )
        .await,
        [
            "fnr_aliases",
            "fnr_client_policies",
            "fnr_config",
            "fnr_domains",
            "fnr_functions",
            "fnr_hosts",
            "fnr_routes",
            "fnr_secrets",
            "fnr_trigger_objects",
            "fnr_versions"
        ]
    );
    assert!(columns(pool, "fnr_functions")
        .await
        .contains(&"service_name".to_string()));
    assert!(columns(pool, "fnr_versions")
        .await
        .contains(&"state".to_string()));
    let digest = format!("sha256:{}", "a".repeat(64));
    exec(
        pool,
        &format!(
            "INSERT INTO fnr_functions (id, application_id, application_code, service_name, name, runtime) \
             VALUES ('fnr_probe', 'app_1', 'app-code', 'svc', 'fn-name', 'JS'); \
             INSERT INTO fnr_versions (id, function_id, version, artifact_ref, digest, manifest, state, published_by) \
             VALUES ('fnr_probe_v1', 'fnr_probe', 1, 'file://x', '{digest}', '{{}}'::jsonb, 'PUBLISHED', 'prn_1'); \
             DELETE FROM fnr_functions WHERE id = 'fnr_probe';"
        ),
    )
    .await;
}

/// Turn a database Rust migrated into what a Go-migrated database looks like
/// to Rust on first contact: no Rust tracker, no `fnr_*`, and (Go before its
/// 059) a source CHECK without FUNCTION.
async fn make_it_look_go_migrated_before_059(pool: &PgPool) {
    exec(
        pool,
        "DROP TABLE fnr_secrets, fnr_config, fnr_trigger_objects, fnr_routes, fnr_domains, \
                    fnr_client_policies, fnr_hosts, fnr_aliases, fnr_versions, fnr_functions; \
         DROP TABLE _schema_migrations; \
         ALTER TABLE msg_subscriptions DROP CONSTRAINT chk_msg_subscriptions_source; \
         ALTER TABLE msg_subscriptions ADD CONSTRAINT chk_msg_subscriptions_source \
             CHECK (source IN ('CODE', 'API', 'UI'));",
    )
    .await;
}

/// Rust first, then Go's 059, then Rust again: Go's `fn_*` are created in
/// Go's shape next to `fnr_*`, and Rust's second run does nothing to them.
#[tokio::test]
#[ignore = "requires Docker"]
async fn rust_then_go_then_rust() {
    let (pool, _container) = fresh_postgres().await;
    migrate(&pool).await;
    assert_rust_fnr_tables(&pool).await;

    apply_go_059(&pool).await;
    migrate(&pool).await;

    assert_go_fn_tables(&pool).await;
    assert_rust_fnr_tables(&pool).await;
    assert!(source_check(&pool).await.contains("'FUNCTION'"));
}

/// A Go-migrated database: Go's 059 first, then Rust (first contact, the
/// pre-tracker backfill path), then Go's 059 again. Rust never runs a
/// retired migration, so Go's `fn_*` stay Go's; 062 creates `fnr_*`.
#[tokio::test]
#[ignore = "requires Docker"]
async fn go_then_rust() {
    let (pool, _container) = fresh_postgres().await;
    migrate(&pool).await;
    make_it_look_go_migrated_before_059(&pool).await;

    apply_go_059(&pool).await;
    migrate(&pool).await;
    apply_go_059(&pool).await;
    migrate(&pool).await;

    assert_go_fn_tables(&pool).await;
    assert_rust_fnr_tables(&pool).await;
    let tracked = strings(
        &pool,
        "SELECT migration_id::text FROM _schema_migrations \
         WHERE migration_id LIKE '0%function%' ORDER BY migration_id",
    )
    .await;
    assert_eq!(tracked, ["062_function_registry_fnr"]);
}

/// A Go database before its 059 meets Rust first: 062 widens the source
/// CHECK to FUNCTION, and Go's 059 runs cleanly afterwards.
#[tokio::test]
#[ignore = "requires Docker"]
async fn rust_widens_a_go_source_check_then_go_059_runs() {
    let (pool, _container) = fresh_postgres().await;
    migrate(&pool).await;
    make_it_look_go_migrated_before_059(&pool).await;
    assert!(!source_check(&pool).await.contains("'FUNCTION'"));

    migrate(&pool).await;
    assert!(source_check(&pool).await.contains("'FUNCTION'"));
    assert_rust_fnr_tables(&pool).await;

    apply_go_059(&pool).await;
    migrate(&pool).await;
    assert_go_fn_tables(&pool).await;
    assert_rust_fnr_tables(&pool).await;
}

/// A dev database from before decision #48: 034, 037 and 056 applied (their
/// SQL run and their rows recorded as the runner records them), with a
/// function registered in Rust's `fn_*`. The runner neither re-runs nor
/// drift-checks the retired rows (even one whose checksum no longer
/// matches), leaves them and `fn_*` untouched, copies nothing, and creates
/// `fnr_*` with the retired migrations' end state renamed.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_database_that_applied_the_retired_migrations_migrates_cleanly() {
    let (pool, _container) = fresh_postgres().await;
    migrate(&pool).await;

    // Rewind to before 062 and replay the old migrations as the runner did.
    exec(
        &pool,
        "DROP TABLE fnr_secrets, fnr_config, fnr_trigger_objects, fnr_routes, fnr_domains, \
                    fnr_client_policies, fnr_hosts, fnr_aliases, fnr_versions, fnr_functions; \
         DELETE FROM _schema_migrations WHERE migration_id = '062_function_registry_fnr';",
    )
    .await;
    for (id, sql) in RETIRED {
        sqlx::raw_sql(sql).execute(&pool).await.expect(id);
        sqlx::query(
            "INSERT INTO _schema_migrations (migration_id, duration_ms, checksum) \
             VALUES ($1, 1, encode(sha256(convert_to($2, 'UTF8')), 'hex'))",
        )
        .bind(id)
        .bind(sql)
        .execute(&pool)
        .await
        .unwrap();
    }
    // A checksum that no longer matches its file is not drift for a retired
    // migration: it is never compared.
    exec(
        &pool,
        "UPDATE _schema_migrations SET checksum = 'edited' \
         WHERE migration_id = '056_function_js_runtime'",
    )
    .await;
    exec(
        &pool,
        "INSERT INTO fn_functions (id, application_id, application_code, service_name, name, runtime) \
         VALUES ('fn_old', 'app_1', 'app-code', 'svc', 'old', 'JS')",
    )
    .await;
    let recorded_before = strings(
        &pool,
        "SELECT migration_id || ':' || checksum FROM _schema_migrations \
         WHERE migration_id IN ('034_functions', '037_function_component_runtime', \
                                '056_function_js_runtime') ORDER BY migration_id",
    )
    .await;
    assert_eq!(recorded_before.len(), 3);

    migrate(&pool).await;
    migrate(&pool).await;

    assert_rust_fnr_tables(&pool).await;
    let recorded_after = strings(
        &pool,
        "SELECT migration_id || ':' || checksum FROM _schema_migrations \
         WHERE migration_id IN ('034_functions', '037_function_component_runtime', \
                                '056_function_js_runtime') ORDER BY migration_id",
    )
    .await;
    assert_eq!(recorded_after, recorded_before, "retired rows untouched");
    assert_eq!(
        strings(&pool, "SELECT id::text FROM fn_functions").await,
        ["fn_old"],
        "fn_ rows stay where they are"
    );
    assert!(
        strings(&pool, "SELECT id::text FROM fnr_functions")
            .await
            .is_empty(),
        "nothing is copied into fnr_"
    );

    // fnr_* is the retired migrations' end state, renamed: the same columns,
    // constraints and indexes.
    let schema = |prefix: &'static str| {
        let pool = pool.clone();
        async move {
            let pattern = format!("{prefix}\\_%");
            let mut out = strings(
                &pool,
                &format!(
                    "SELECT table_name || '.' || column_name || ' ' || ordinal_position || ' ' \
                            || data_type || ' ' || coalesce(character_maximum_length::text, '-') \
                            || ' ' || is_nullable || ' ' || coalesce(column_default, '-') \
                     FROM information_schema.columns \
                     WHERE table_schema = 'public' AND table_name LIKE '{pattern}'"
                ),
            )
            .await;
            out.extend(
                strings(
                    &pool,
                    &format!(
                        "SELECT conrelid::regclass::text || ' ' || conname || ' ' || \
                                pg_get_constraintdef(oid) \
                         FROM pg_constraint WHERE conrelid::regclass::text LIKE '{pattern}'"
                    ),
                )
                .await,
            );
            out.extend(
                strings(
                    &pool,
                    &format!(
                        "SELECT indexname || ' ' || indexdef FROM pg_indexes \
                         WHERE schemaname = 'public' AND tablename LIKE '{pattern}'"
                    ),
                )
                .await,
            );
            out
        }
    };
    let mut renamed: Vec<String> = schema("fn")
        .await
        .into_iter()
        .map(|s| s.replace("fn_", "fnr_"))
        .collect();
    let mut fnr = schema("fnr").await;
    renamed.sort();
    fnr.sort();
    assert!(!fnr.is_empty());
    assert_eq!(fnr, renamed);
}
