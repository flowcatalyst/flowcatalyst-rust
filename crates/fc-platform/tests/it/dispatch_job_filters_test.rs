//! Event type, subscription and connection filters of the dispatch-job reads,
//! the projected `connection_id`, and the cached filter-options (Go
//! `bbbb4bb3`, `dispatchjob/api/filter_refs_pg_test.go`,
//! `filter_params_test.go`, `stream/dispatch_jobs_projection_pg_test.go`).
//! Requires a Postgres (see `support::db`).

use crate::support;

use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use fc_platform::domain::{Principal, UserScope};
use fc_platform::role::entity::AuthRole;
use fc_platform::service_account::entity::RoleAssignment;
use fc_platform_core::permissions::admin as perms;
use fc_platform_core::shared::id::ClientId;
use fc_stream::dispatch_job_projection;
use fc_stream::health::StreamHealth;
use serde_json::json;
use support::{read_json, TestApp};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

const CLIENT_A: &str = "clt_flt00client0a";
const CLIENT_B: &str = "clt_flt00client0b";

async fn connection(app: &TestApp, id: &str, code: &str, client: Option<&str>) {
    sqlx::query(
        "INSERT INTO msg_connections (id, code, name, service_account_id, client_id) VALUES ($1, $2, $2, 'sac_flt', $3)",
    )
    .bind(id)
    .bind(code)
    .bind(client)
    .execute(&app.pool)
    .await
    .expect("connection");
}

async fn subscription(
    app: &TestApp,
    id: &str,
    code: &str,
    name: &str,
    client: Option<&str>,
    conn: Option<&str>,
) {
    sqlx::query(
        "INSERT INTO msg_subscriptions (id, code, name, target, client_id, connection_id) VALUES ($1, $2, $3, 'http://subscriber.test/h', $4, $5)",
    )
    .bind(id)
    .bind(code)
    .bind(name)
    .bind(client)
    .bind(conn)
    .execute(&app.pool)
    .await
    .expect("subscription");
}

/// One read-projection row an hour old.
async fn job(
    app: &TestApp,
    id: &str,
    code: &str,
    sub: Option<&str>,
    conn: Option<&str>,
    client: Option<&str>,
) {
    sqlx::query(
        "INSERT INTO msg_dispatch_jobs_read
             (id, code, target_url, kind, protocol, mode, status, max_retries, attempt_count,
              sequence, timeout_seconds, retry_strategy, subscription_id, connection_id, client_id, created_at, updated_at)
         VALUES ($1, $2, 'http://example.invalid/hook', 'EVENT', 'HTTP_WEBHOOK', 'IMMEDIATE', 'FAILED', 3, 1,
                 99, 30, 'exponential', $3, $4, $5, now() - interval '1 hour', now() - interval '1 hour')",
    )
    .bind(id)
    .bind(code)
    .bind(sub)
    .bind(conn)
    .bind(client)
    .execute(&app.pool)
    .await
    .expect("job");
}

async fn ids_of(app: &TestApp, token: &str, query: &str) -> Vec<String> {
    let (status, body) =
        read_json(app.get(&format!("/api/dispatch-jobs?{query}"), token).await).await;
    assert_eq!(status, StatusCode::OK, "{query}: {body}");
    let mut v: Vec<String> = body
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_string())
        .collect();
    v.sort();
    v
}

async fn tenant_viewer(app: &TestApp, client: &str) -> String {
    let role_code = "flt:viewer";
    if app
        .repos
        .role_repo
        .find_by_name(role_code)
        .await
        .unwrap()
        .is_none()
    {
        let role = AuthRole::new("flt", "viewer", "Filter viewer")
            .with_permission(perms::DISPATCH_JOB_READ);
        app.repos.role_repo.insert(&role).await.unwrap();
    }
    let mut user = Principal::new_user("who@flt.test", UserScope::Client)
        .with_client_id(ClientId::parse(client).unwrap());
    user.roles = vec![RoleAssignment::new(role_code)];
    app.auth_service.generate_access_token(&user).unwrap()
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn event_types_filter_the_code_and_are_capped() {
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token_all_applications().await;
    job(
        &app,
        "fltet0000001",
        "flt:et:order:created",
        None,
        None,
        None,
    )
    .await;
    job(
        &app,
        "fltet0000002",
        "flt:et:order:shipped",
        None,
        None,
        None,
    )
    .await;
    job(
        &app,
        "fltet0000003",
        "flt:et:invoice:paid",
        None,
        None,
        None,
    )
    .await;

    assert_eq!(
        ids_of(&app, &admin, "eventType=flt:et:order:created").await,
        ["fltet0000001"]
    );
    assert_eq!(
        ids_of(
            &app,
            &admin,
            "eventTypes=flt:et:order:created,flt:et:invoice:paid"
        )
        .await,
        ["fltet0000001", "fltet0000003"]
    );
    // eventType joins eventTypes; with codes all must hold.
    assert_eq!(
        ids_of(
            &app,
            &admin,
            "eventType=flt:et:order:shipped&eventTypes=flt:et:order:created"
        )
        .await,
        ["fltet0000001", "fltet0000002"]
    );
    assert_eq!(
        ids_of(
            &app,
            &admin,
            "eventTypes=flt:et:order:created,flt:et:order:shipped&codes=flt:et:order:shipped"
        )
        .await,
        ["fltet0000002"]
    );
    // Eleven event types: 400.
    let eleven = (0..11)
        .map(|i| format!("t{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let (status, body) = read_json(
        app.get(&format!("/api/dispatch-jobs?eventTypes={eleven}"), &admin)
            .await,
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("TOO_MANY_EVENT_TYPES")),
        "{body}"
    );
    // The same on the bulk requeue's filter.
    let (status, body) = read_json(
        app.post(
            "/api/dispatch-jobs/requeue-by-filter/preview",
            &admin,
            json!({ "eventTypes": eleven }),
        )
        .await,
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("TOO_MANY_EVENT_TYPES")),
        "{body}"
    );
    let (count, _) = (
        read_json(
            app.post(
                "/api/dispatch-jobs/requeue-by-filter/preview",
                &admin,
                json!({ "eventType": "flt:et:order:created" }),
            )
            .await,
        )
        .await
        .1["count"]
            .clone(),
        (),
    );
    assert_eq!(count, 1);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn subscription_ids_codes_and_names_are_resolved_first() {
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token_all_applications().await;
    subscription(
        &app,
        "sub_flt000001",
        "flt-orders-main",
        "Orders Main",
        None,
        None,
    )
    .await;
    subscription(
        &app,
        "sub_flt000002",
        "flt-orders-audit",
        "Orders Audit",
        None,
        None,
    )
    .await;
    subscription(&app, "sub_flt000003", "flt-billing", "Billing", None, None).await;
    for (id, sub) in [
        ("fltsb0000001", "sub_flt000001"),
        ("fltsb0000002", "sub_flt000002"),
        ("fltsb0000003", "sub_flt000003"),
    ] {
        job(&app, id, "flt:sb:x:y", Some(sub), None, None).await;
    }
    job(&app, "fltsb0000004", "flt:sb:x:y", None, None, None).await;

    assert_eq!(
        ids_of(&app, &admin, "subscriptionIds=sub_flt000001,sub_flt000003").await,
        ["fltsb0000001", "fltsb0000003"]
    );
    // Singular and plural both hold.
    assert_eq!(
        ids_of(
            &app,
            &admin,
            "subscriptionId=sub_flt000001&subscriptionIds=sub_flt000001,sub_flt000003"
        )
        .await,
        ["fltsb0000001"]
    );
    assert_eq!(
        ids_of(&app, &admin, "subscriptionCode=flt-billing").await,
        ["fltsb0000003"]
    );
    assert_eq!(
        ids_of(&app, &admin, "subscriptionCode=flt-orders-*").await,
        ["fltsb0000001", "fltsb0000002"],
        "a trailing * is a prefix"
    );
    assert_eq!(
        ids_of(&app, &admin, "subscriptionName=orders%20main").await,
        ["fltsb0000001"],
        "names fold case"
    );
    assert_eq!(
        ids_of(&app, &admin, "subscriptionName=ORDERS*").await,
        ["fltsb0000001", "fltsb0000002"]
    );
    assert_eq!(
        ids_of(
            &app,
            &admin,
            "subscriptionCode=flt-orders-*&subscriptionName=Orders%20Audit"
        )
        .await,
        ["fltsb0000002"],
        "code and name both hold"
    );
    // A name that matches nothing lists nothing (not everything).
    assert!(ids_of(&app, &admin, "subscriptionCode=no-such-code")
        .await
        .is_empty());
    // % and _ in a prefix are literal.
    assert!(ids_of(&app, &admin, "subscriptionCode=flt%25*")
        .await
        .is_empty());
    assert!(ids_of(&app, &admin, "subscriptionCode=flt_orders*")
        .await
        .is_empty());

    // 51 ids: 400; 201 matches: 400 TOO_MANY_MATCHES.
    let many = (0..51)
        .map(|i| format!("s{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let (status, body) = read_json(
        app.get(
            &format!("/api/dispatch-jobs?subscriptionIds={many}"),
            &admin,
        )
        .await,
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("TOO_MANY_SUBSCRIPTION_IDS")),
        "{body}"
    );
    sqlx::query(
        "INSERT INTO msg_subscriptions (id, code, name, target) SELECT 'sub_mny' || lpad(n::text, 6, '0'), 'flt-many-' || n, 'Many ' || n, 'http://s.test/h' FROM generate_series(1, 201) n",
    )
    .execute(&app.pool)
    .await
    .unwrap();
    let (status, body) = read_json(
        app.get("/api/dispatch-jobs?subscriptionCode=flt-many-*", &admin)
            .await,
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("TOO_MANY_MATCHES")),
        "{body}"
    );
    let (status, body) = read_json(
        app.post(
            "/api/dispatch-jobs/requeue-by-filter/preview",
            &admin,
            json!({ "subscriptionCode": "flt-many-*" }),
        )
        .await,
    )
    .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("TOO_MANY_MATCHES")),
        "{body}"
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_connection_is_its_subscriptions_jobs_plus_the_jobs_naming_it() {
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token_all_applications().await;
    connection(&app, "con_flt0000001", "flt-conn-a", None).await;
    connection(&app, "con_flt0000002", "flt-conn-b", None).await;
    subscription(
        &app,
        "sub_flt100001",
        "flt-ca-sub",
        "CA",
        None,
        Some("con_flt0000001"),
    )
    .await;
    subscription(
        &app,
        "sub_flt100002",
        "flt-cb-sub",
        "CB",
        None,
        Some("con_flt0000002"),
    )
    .await;
    job(
        &app,
        "fltcn0000001",
        "flt:cn:a:y",
        Some("sub_flt100001"),
        None,
        None,
    )
    .await; // via the subscription
    job(
        &app,
        "fltcn0000002",
        "flt:cn:a:y",
        None,
        Some("con_flt0000001"),
        None,
    )
    .await; // names the connection
    job(
        &app,
        "fltcn0000003",
        "flt:cn:b:y",
        Some("sub_flt100002"),
        None,
        None,
    )
    .await;
    job(
        &app,
        "fltcn0000004",
        "flt:cn:b:y",
        None,
        Some("con_flt0000002"),
        None,
    )
    .await;

    for q in ["connectionId=con_flt0000001", "connectionCode=flt-conn-a"] {
        assert_eq!(
            ids_of(&app, &admin, q).await,
            ["fltcn0000001", "fltcn0000002"],
            "{q}"
        );
    }
    assert_eq!(
        ids_of(&app, &admin, "connectionCode=flt-conn-*").await,
        [
            "fltcn0000001",
            "fltcn0000002",
            "fltcn0000003",
            "fltcn0000004"
        ]
    );
    // Both given: the id must be one the code names.
    assert!(ids_of(
        &app,
        &admin,
        "connectionId=con_flt0000002&connectionCode=flt-conn-a"
    )
    .await
    .is_empty());
    assert_eq!(
        ids_of(
            &app,
            &admin,
            "connectionId=con_flt0000001&connectionCode=flt-conn-a"
        )
        .await,
        ["fltcn0000001", "fltcn0000002"]
    );
    assert!(ids_of(&app, &admin, "connectionCode=nope").await.is_empty());

    // The bulk requeue's filter takes the same members.
    let (status, body) = read_json(
        app.post(
            "/api/dispatch-jobs/requeue-by-filter/preview",
            &admin,
            json!({ "connectionCode": "flt-conn-b" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["count"], 2, "{body}");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn resolution_is_confined_to_the_callers_tenants() {
    let app = TestApp::setup().await;
    subscription(
        &app,
        "sub_flt200001",
        "flt-tn-a",
        "Tenant A",
        Some(CLIENT_A),
        None,
    )
    .await;
    subscription(
        &app,
        "sub_flt200002",
        "flt-tn-b",
        "Tenant B",
        Some(CLIENT_B),
        None,
    )
    .await;
    subscription(&app, "sub_flt200003", "flt-tn-p", "Platform", None, None).await;
    job(
        &app,
        "flttn0000001",
        "flt:tn:x:y",
        Some("sub_flt200001"),
        None,
        Some(CLIENT_A),
    )
    .await;
    job(
        &app,
        "flttn0000002",
        "flt:tn:x:y",
        Some("sub_flt200002"),
        None,
        Some(CLIENT_B),
    )
    .await;
    job(
        &app,
        "flttn0000003",
        "flt:tn:x:y",
        Some("sub_flt200003"),
        None,
        None,
    )
    .await;
    let viewer = tenant_viewer(&app, CLIENT_A).await;
    let admin = app.anchor_admin_token_all_applications().await;

    // The client user resolves its own and the platform's names, never another tenant's.
    assert_eq!(
        ids_of(&app, &viewer, "subscriptionCode=flt-tn-*").await,
        ["flttn0000001", "flttn0000003"]
    );
    assert!(ids_of(&app, &viewer, "subscriptionCode=flt-tn-b")
        .await
        .is_empty());
    assert_eq!(
        ids_of(&app, &admin, "subscriptionCode=flt-tn-*").await,
        ["flttn0000001", "flttn0000002", "flttn0000003"]
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn filter_options_list_event_types_and_are_cached_for_a_minute() {
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token_all_applications().await;
    job(&app, "fltfo0000001", "flt:fo:first:x", None, None, None).await;
    let (status, first) =
        read_json(app.get("/api/dispatch-jobs/filter-options", &admin).await).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["eventTypes"], first["codes"]);
    assert!(first["eventTypes"]
        .as_array()
        .unwrap()
        .contains(&json!("flt:fo:first:x")));

    // A job created after is not in the next answer inside the minute...
    job(&app, "fltfo0000002", "flt:fo:second:x", None, None, None).await;
    let (_, again) = read_json(app.get("/api/dispatch-jobs/filter-options", &admin).await).await;
    assert_eq!(again, first, "served from the cache");
    // ...a different window or tenant scope is its own entry.
    let since = (chrono::Utc::now() - chrono::Duration::hours(30))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let (_, other_window) = read_json(
        app.get(
            &format!("/api/dispatch-jobs/filter-options?since={since}"),
            &admin,
        )
        .await,
    )
    .await;
    assert!(other_window["eventTypes"]
        .as_array()
        .unwrap()
        .contains(&json!("flt:fo:second:x")));
    let viewer = tenant_viewer(&app, CLIENT_A).await;
    let (_, scoped) = read_json(app.get("/api/dispatch-jobs/filter-options", &viewer).await).await;
    assert!(
        scoped["eventTypes"]
            .as_array()
            .unwrap()
            .contains(&json!("flt:fo:second:x")),
        "platform jobs are visible to a tenant"
    );
}

/// The projector copies the write row's `connection_id` to the read row, on
/// insert and on every later projection.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_projector_carries_connection_id_to_the_read_table() {
    let app = TestApp::setup().await;
    sqlx::query(
        "INSERT INTO msg_dispatch_jobs (id, code, source, target_url, status, connection_id, created_at, updated_at)
         VALUES ('fltpj0000001', 'flt:pj:x:y', 't', 'http://example.invalid/h', 'PENDING', 'con_fltpj000001', now(), now()),
                ('fltpj0000002', 'flt:pj:x:y', 't', 'http://example.invalid/h', 'PENDING', NULL, now(), now())",
    )
    .execute(&app.pool)
    .await
    .unwrap();
    let project = || async {
        let cancel = CancellationToken::new();
        let task = tokio::spawn(dispatch_job_projection::run(
            app.pool.clone(),
            200,
            Arc::new(StreamHealth::new("dispatch-job-projection".into())),
            cancel.clone(),
        ));
        sleep(Duration::from_millis(1500)).await;
        cancel.cancel();
        task.await.unwrap();
    };
    project().await;
    let read = || async {
        sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT id, connection_id FROM msg_dispatch_jobs_read WHERE id LIKE 'fltpj%' ORDER BY id",
        )
        .fetch_all(&app.pool)
        .await
        .unwrap()
    };
    assert_eq!(
        read().await,
        [
            (
                "fltpj0000001".to_string(),
                Some("con_fltpj000001".to_string())
            ),
            ("fltpj0000002".to_string(), None)
        ]
    );
    // The conflict arm refreshes it too.
    sqlx::query("UPDATE msg_dispatch_jobs SET connection_id = 'con_fltpj000002', status = 'FAILED', updated_at = now() WHERE id = 'fltpj0000002'")
        .execute(&app.pool)
        .await
        .unwrap();
    project().await;
    assert_eq!(read().await[1].1.as_deref(), Some("con_fltpj000002"));
}
