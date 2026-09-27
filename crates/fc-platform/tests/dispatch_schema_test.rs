//! Migrations 039 and 057: Go's dispatch-job columns (054 and 057). 039 adds
//! the job's own queue priority and an attempt's request summary; 057 the
//! descriptor and the read projection's metadata. Requires Docker.

use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

use fc_platform::shared::database::{create_pool, run_migrations, MigrationProfile};

#[tokio::test]
#[ignore = "requires Docker"]
async fn migration_039_adds_gos_columns_and_is_recognised_when_already_applied() {
    let container = Postgres::default()
        .with_db_name("fc")
        .with_user("test")
        .with_password("test")
        .start()
        .await
        .unwrap();
    let url = format!(
        "postgresql://test:test@{}:{}/fc",
        container.get_host().await.unwrap(),
        container.get_host_port_ipv4(5432).await.unwrap()
    );
    let pool = create_pool(&url).await.unwrap();
    run_migrations(&pool, MigrationProfile::Production)
        .await
        .unwrap();

    let columns: Vec<(String, String)> = sqlx::query_as(
        "SELECT table_name::text, column_name::text FROM information_schema.columns \
         WHERE table_schema = 'public' \
           AND ((table_name = 'msg_dispatch_jobs' AND column_name = 'queue') \
             OR (table_name = 'msg_dispatch_job_attempts' AND column_name = 'request_info')) \
         ORDER BY 1",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        columns,
        vec![
            (
                "msg_dispatch_job_attempts".to_string(),
                "request_info".to_string()
            ),
            ("msg_dispatch_jobs".to_string(), "queue".to_string()),
        ]
    );

    // Re-running is a no-op, and a database Go already migrated (the
    // columns present, no tracker row) is recognised rather than re-run.
    sqlx::raw_sql(include_str!(
        "../../../migrations/039_dispatch_job_queue_and_attempt_request.sql"
    ))
    .execute(&pool)
    .await
    .expect("re-running 039 is a no-op");
    sqlx::query("DELETE FROM _schema_migrations")
        .execute(&pool)
        .await
        .unwrap();
    run_migrations(&pool, MigrationProfile::Production)
        .await
        .expect("migrations over an already-migrated schema");
    let (tracked,): (bool,) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM _schema_migrations \
         WHERE migration_id = '039_dispatch_job_queue_and_attempt_request')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(tracked, "the probe recognises an applied 039");
}

/// Migration 057: the rest of Go's 057 — `descriptor` on both dispatch-job
/// tables and `metadata` on the read projection, with Go's types,
/// nullability and defaults.
#[tokio::test]
#[ignore = "requires Docker"]
async fn migration_057_adds_gos_descriptor_and_read_metadata() {
    let container = Postgres::default()
        .with_db_name("fc")
        .with_user("test")
        .with_password("test")
        .start()
        .await
        .unwrap();
    let url = format!(
        "postgresql://test:test@{}:{}/fc",
        container.get_host().await.unwrap(),
        container.get_host_port_ipv4(5432).await.unwrap()
    );
    let pool = create_pool(&url).await.unwrap();
    run_migrations(&pool, MigrationProfile::Production)
        .await
        .unwrap();

    let columns: Vec<(String, String, String, Option<i32>, String, Option<String>)> =
        sqlx::query_as(
            "SELECT table_name::text, column_name::text, data_type::text, \
                    character_maximum_length::int, is_nullable::text, column_default::text \
             FROM information_schema.columns \
             WHERE table_schema = 'public' \
               AND table_name IN ('msg_dispatch_jobs', 'msg_dispatch_jobs_read') \
               AND column_name IN ('descriptor', 'metadata') \
             ORDER BY 1, 2",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
    let col = |t: &str, c: &str, ty: &str, len: Option<i32>, null: &str, def: Option<&str>| {
        (
            t.to_string(),
            c.to_string(),
            ty.to_string(),
            len,
            null.to_string(),
            def.map(str::to_string),
        )
    };
    assert_eq!(
        columns,
        vec![
            col(
                "msg_dispatch_jobs",
                "descriptor",
                "character varying",
                Some(255),
                "YES",
                None
            ),
            // The write table's metadata predates 057 (migration 019).
            col(
                "msg_dispatch_jobs",
                "metadata",
                "jsonb",
                None,
                "YES",
                Some("'[]'::jsonb")
            ),
            col(
                "msg_dispatch_jobs_read",
                "descriptor",
                "character varying",
                Some(255),
                "YES",
                None
            ),
            col(
                "msg_dispatch_jobs_read",
                "metadata",
                "jsonb",
                None,
                "NO",
                Some("'[]'::jsonb")
            ),
        ]
    );

    // Re-running is a no-op, and a database Go already migrated (the
    // columns present, no tracker row) is recognised rather than re-run.
    sqlx::raw_sql(include_str!(
        "../../../migrations/057_dispatch_job_descriptor_and_read_metadata.sql"
    ))
    .execute(&pool)
    .await
    .expect("re-running 057 is a no-op");
    sqlx::query("DELETE FROM _schema_migrations")
        .execute(&pool)
        .await
        .unwrap();
    run_migrations(&pool, MigrationProfile::Production)
        .await
        .expect("migrations over an already-migrated schema");
    let (tracked,): (bool,) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM _schema_migrations \
         WHERE migration_id = '057_dispatch_job_descriptor_and_read_metadata')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(tracked, "the probe recognises an applied 057");
}
