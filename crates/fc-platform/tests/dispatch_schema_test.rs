//! Migration 039: the dispatch job's own queue priority and an attempt's
//! request summary, as Go's 054 and 057 add them. Requires Docker.

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
