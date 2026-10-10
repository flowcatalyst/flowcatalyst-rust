//! Go's read fixes for dispatch jobs and events (docs/platform-bug-fixes.md
//! in flowcatalyst-go): the filter options are scoped to the caller's
//! tenants (bug 18, 563102a), and a list `size` above 1,000 is clamped to
//! 1,000 instead of falling back to 100 (bug 21, 858f02c). Requires Docker
//! (or `FC_TEST_PG_BIN`).

use crate::support;

use axum::http::StatusCode;
use serde_json::Value;
use std::slice::from_ref;

use fc_platform::client::entity::Client;
use fc_platform::domain::{Principal, UserScope};
use fc_platform::role::entity::permissions::admin;
use fc_platform_core::shared::id::ClientId;
use support::{assert_status, TestApp};

async fn create_client(app: &TestApp, identifier: &str) -> String {
    let client = Client::new(identifier.to_uppercase(), identifier);
    app.repos
        .client_repo
        .insert(&client)
        .await
        .expect("insert client");
    client.id.to_string()
}

/// `n` read-projection jobs of `code` for `client` (none: platform), ids
/// prefixed `prefix` (13 characters in all).
async fn insert_jobs(app: &TestApp, prefix: &str, code: &str, client: Option<&str>, n: i32) {
    sqlx::query(
        "INSERT INTO msg_dispatch_jobs_read
           (id, code, client_id, target_url, kind, protocol, mode, status, max_retries,
            retry_strategy, is_completed, is_terminal, created_at, updated_at)
         SELECT $1 || lpad(g::text, 13 - length($1), '0'), $2, $3, 'https://receiver.example.test/hook',
                'EVENT', 'HTTP_WEBHOOK', 'IMMEDIATE', 'PENDING', 3, 'exponential', false, false,
                NOW(), NOW()
           FROM generate_series(1, $4) g",
    )
    .bind(prefix)
    .bind(code)
    .bind(client)
    .bind(n)
    .execute(&app.pool)
    .await
    .expect("insert jobs");
}

/// `n` read-projection events of `event_type` for `client` (none: platform).
async fn insert_events(
    app: &TestApp,
    prefix: &str,
    event_type: &str,
    client: Option<&str>,
    n: i32,
) {
    let application = event_type.split(':').next().unwrap();
    sqlx::query(
        "INSERT INTO msg_events_read (id, type, source, time, client_id, application, created_at)
         SELECT $1 || lpad(g::text, 13 - length($1), '0'), $2, 'test://scope', NOW(), $3, $4, NOW()
           FROM generate_series(1, $5) g",
    )
    .bind(prefix)
    .bind(event_type)
    .bind(client)
    .bind(application)
    .bind(n)
    .execute(&app.pool)
    .await
    .expect("insert events");
}

fn client_holding(app: &TestApp, client: &str, perms: &[&str]) -> String {
    let p = Principal::new_user("scope@read-scope.test", UserScope::Client)
        .with_client_id(ClientId::parse(client).unwrap());
    let granted: Vec<String> = perms.iter().map(|s| s.to_string()).collect();
    app.auth_service
        .generate_access_token_with_scope(&p, &granted, None)
        .expect("token")
}

fn values(v: &Value) -> Vec<String> {
    let mut out: Vec<String> = v
        .as_array()
        .unwrap_or_else(|| panic!("not an array: {v}"))
        .iter()
        .map(|x| {
            // A plain string, or a {label, value} option.
            x.as_str()
                .or_else(|| x["value"].as_str())
                .unwrap()
                .to_string()
        })
        .collect();
    out.sort();
    out
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn filter_options_are_scoped_to_the_callers_tenants() {
    let app = TestApp::setup().await;
    let mine = create_client(&app, "mine").await;
    let theirs = create_client(&app, "theirs").await;
    insert_jobs(&app, "djmine", "mine:orders:order:created", Some(&mine), 1).await;
    insert_jobs(
        &app,
        "djtheirs",
        "theirs:orders:order:created",
        Some(&theirs),
        1,
    )
    .await;
    insert_jobs(&app, "djplat", "platform:iam:user:created", None, 1).await;
    insert_events(&app, "evmine", "mine:orders:order:created", Some(&mine), 1).await;
    insert_events(
        &app,
        "evtheirs",
        "theirs:orders:order:created",
        Some(&theirs),
        1,
    )
    .await;
    insert_events(&app, "evplat", "platform:iam:user:created", None, 1).await;

    let token = client_holding(&app, &mine, &[admin::DISPATCH_JOB_READ, admin::EVENT_READ]);
    let jobs = assert_status(
        app.get("/api/dispatch-jobs/filter-options", &token).await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        values(&jobs["codes"]),
        ["mine:orders:order:created", "platform:iam:user:created"],
        "{jobs}"
    );
    assert_eq!(values(&jobs["clientIds"]), from_ref(&mine), "{jobs}");

    let events = assert_status(
        app.get("/api/events/filter-options", &token).await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        values(&events["eventTypes"]),
        ["mine:orders:order:created", "platform:iam:user:created"],
        "{events}"
    );
    assert_eq!(
        values(&events["applications"]),
        ["mine", "platform"],
        "{events}"
    );

    // An anchor sees every tenant's.
    let admin = app.anchor_admin_token().await;
    let jobs = assert_status(
        app.get("/api/dispatch-jobs/filter-options", &admin).await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(values(&jobs["codes"]).len(), 3, "{jobs}");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_size_above_the_cap_is_clamped_not_reset() {
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token().await;
    insert_jobs(&app, "djclp", "clamp:jobs:list:made", None, 150).await;
    insert_events(&app, "evclp", "clamp:events:list:made", None, 150).await;

    for size in [1000, 1001, 5000] {
        let jobs = assert_status(
            app.get(
                &format!("/api/dispatch-jobs?codes=clamp:jobs:list:made&size={size}"),
                &admin,
            )
            .await,
            StatusCode::OK,
        )
        .await;
        assert_eq!(
            jobs.as_array().map(Vec::len),
            Some(150),
            "jobs, size {size}"
        );
        let events = assert_status(
            app.get(
                &format!("/api/events?types=clamp:events:list:made&size={size}"),
                &admin,
            )
            .await,
            StatusCode::OK,
        )
        .await;
        assert_eq!(
            events.as_array().map(Vec::len),
            Some(150),
            "events, size {size}"
        );
    }
    let jobs = assert_status(
        app.get("/api/dispatch-jobs?codes=clamp:jobs:list:made", &admin)
            .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        jobs.as_array().map(Vec::len),
        Some(100),
        "no size: the default 100"
    );
}
