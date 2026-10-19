//! Requeue by filter (Go `dispatchjob/api/requeue_by_filter_pg_test.go`): a
//! preview of what a list filter matches, and a confirmed, capped, batched
//! reset of every job it matches. Requires a Postgres (see `support::db`).

use crate::support;

use axum::http::StatusCode;
use fc_platform::role::entity::AuthRole;
use fc_platform_core::permissions;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use support::{read_json, TestApp};

const CLIENT_A: &str = "clt_rbfclient00a";
const CLIENT_B: &str = "clt_rbfclient00b";

/// Write `n` jobs into BOTH the write table (what the use case resets) and the
/// read projection (what the filter counts and walks): the projector does not
/// run in these tests. Ids are prefix + a zero-padded counter, so id order is
/// seeding order. Returns the ids.
async fn seed_jobs(
    app: &TestApp,
    prefix: &str,
    code: &str,
    status: &str,
    client_id: Option<&str>,
    n: i32,
) -> Vec<String> {
    assert_eq!(prefix.len(), 5, "ids are 5-char prefix + 8 digits");
    let created = chrono::Utc::now() - chrono::Duration::hours(1);
    sqlx::query(
        "INSERT INTO msg_dispatch_jobs
             (id, code, source, target_url, status, client_id, attempt_count, last_error, created_at, updated_at)
         SELECT $1 || lpad(n::text, 8, '0'), $2, 'rbf-test', 'http://example.invalid/hook', $3, $4, 3, 'boom', $6, $6
           FROM generate_series(1, $5::int) n",
    )
    .bind(prefix)
    .bind(code)
    .bind(status)
    .bind(client_id)
    .bind(n)
    .bind(created)
    .execute(&app.pool)
    .await
    .expect("seed write table");
    sqlx::query(
        "INSERT INTO msg_dispatch_jobs_read
             (id, code, target_url, client_id, kind, protocol, mode, status, max_retries, attempt_count, created_at, updated_at)
         SELECT $1 || lpad(n::text, 8, '0'), $2, 'http://example.invalid/hook', $4, 'EVENT', 'HTTP_WEBHOOK', 'IMMEDIATE', $3, 3, 3, $6, $6
           FROM generate_series(1, $5::int) n",
    )
    .bind(prefix)
    .bind(code)
    .bind(status)
    .bind(client_id)
    .bind(n)
    .bind(created)
    .execute(&app.pool)
    .await
    .expect("seed read projection");
    (1..=n).map(|i| format!("{prefix}{i:08}")).collect()
}

/// Do the projector's job for `code`: the read rows take the write rows' status.
async fn sync_projection(app: &TestApp, code: &str) {
    sqlx::query(
        "UPDATE msg_dispatch_jobs_read r SET status = j.status, attempt_count = j.attempt_count
           FROM msg_dispatch_jobs j WHERE r.id = j.id AND r.created_at = j.created_at AND r.code = $1",
    )
    .bind(code)
    .execute(&app.pool)
    .await
    .expect("sync projection");
}

/// Status counts of the write-table rows for `ids`.
async fn statuses_of(app: &TestApp, ids: &[String]) -> BTreeMap<String, i64> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT status, count(*) FROM msg_dispatch_jobs WHERE id = ANY($1) GROUP BY status",
    )
    .bind(ids)
    .fetch_all(&app.pool)
    .await
    .unwrap();
    rows.into_iter().collect()
}

fn map(pairs: &[(&str, i64)]) -> BTreeMap<String, i64> {
    pairs.iter().map(|(k, v)| ((*k).to_string(), *v)).collect()
}

async fn setup() -> (TestApp, String) {
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token_all_applications().await;
    (app, admin)
}

/// A client-scoped caller that holds only the dispatch-job view permission.
async fn client_viewer(app: &TestApp, client: &str, perms: &[&str]) -> String {
    use fc_platform::domain::{Principal, UserScope};
    use fc_platform::service_account::entity::RoleAssignment;
    use fc_platform_core::shared::id::ClientId;
    let role_code = format!("rbf:viewer-{}", perms.len());
    if app
        .repos
        .role_repo
        .find_by_name(&role_code)
        .await
        .unwrap()
        .is_none()
    {
        let mut role = AuthRole::new("rbf", format!("viewer-{}", perms.len()), "RBF viewer");
        for p in perms {
            role = role.with_permission(*p);
        }
        app.repos.role_repo.insert(&role).await.unwrap();
    }
    let mut user = Principal::new_user("viewer@rbf.test", UserScope::Client)
        .with_client_id(ClientId::parse(client).unwrap());
    user.roles = vec![RoleAssignment::new(role_code)];
    app.auth_service.generate_access_token(&user).unwrap()
}

async fn preview(app: &TestApp, token: &str, body: Value) -> (StatusCode, Value) {
    read_json(
        app.post("/api/dispatch-jobs/requeue-by-filter/preview", token, body)
            .await,
    )
    .await
}

async fn run(app: &TestApp, token: &str, body: Value) -> (StatusCode, Value) {
    read_json(
        app.post("/api/dispatch-jobs/requeue-by-filter", token, body)
            .await,
    )
    .await
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn preview_counts_and_breaks_down_by_status() {
    let (app, admin) = setup().await;
    let code = "rbftest:preview:job:x";
    seed_jobs(&app, "rbpaf", code, "FAILED", Some(CLIENT_A), 3).await;
    seed_jobs(&app, "rbpac", code, "COMPLETED", Some(CLIENT_A), 2).await;
    seed_jobs(&app, "rbpap", code, "PENDING", Some(CLIENT_A), 1).await;
    seed_jobs(&app, "rbppf", code, "FAILED", None, 2).await;
    seed_jobs(&app, "rbpbf", code, "FAILED", Some(CLIENT_B), 1).await;

    let (status, got) = preview(&app, &admin, json!({ "codes": code })).await;
    assert_eq!(status, StatusCode::OK, "{got}");
    assert_eq!(
        got,
        json!({ "count": 9, "byStatus": { "FAILED": 6, "COMPLETED": 2, "PENDING": 1 }, "eligibleCount": 9 })
    );

    let (_, got) = preview(&app, &admin, json!({ "codes": code, "statuses": "FAILED" })).await;
    assert_eq!(
        (got["count"].as_i64(), got["eligibleCount"].as_i64()),
        (Some(6), Some(6))
    );

    // A tenant's caller sees its own jobs plus platform-scoped ones (the list's
    // rule) but may requeue only its own (the use case's rule).
    let viewer = client_viewer(&app, CLIENT_A, &[permissions::admin::DISPATCH_JOB_READ]).await;
    let (status, got) = preview(&app, &viewer, json!({ "codes": code })).await;
    assert_eq!(status, StatusCode::OK, "{got}");
    assert_eq!(got["count"], 8);
    assert_eq!(
        got["byStatus"],
        json!({ "FAILED": 5, "COMPLETED": 2, "PENDING": 1 })
    );
    assert_eq!(got["eligibleCount"], 6);

    // Asking for another tenant's jobs finds none.
    let (_, got) = preview(
        &app,
        &viewer,
        json!({ "codes": code, "clientIds": CLIENT_B }),
    )
    .await;
    assert_eq!(
        got,
        json!({ "count": 0, "byStatus": {}, "eligibleCount": 0 })
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_confirm_count_below_the_eligible_count_is_a_409_that_resets_nothing() {
    let (app, admin) = setup().await;
    let code = "rbftest:conflict:job:x";
    let ids = seed_jobs(&app, "rbcfl", code, "FAILED", None, 5).await;
    let filter = json!({ "codes": code, "statuses": "FAILED" });

    let mut body = filter.clone();
    body["confirmCount"] = json!(4);
    let (status, err) = run(&app, &admin, body).await;
    assert_eq!(status, StatusCode::CONFLICT, "{err}");
    assert_eq!(err["error"], "FILTER_MATCH_CHANGED", "{err}");
    assert_eq!(
        err["details"],
        json!({ "count": 5, "eligibleCount": 5, "byStatus": { "FAILED": 5 } })
    );
    assert_eq!(
        statuses_of(&app, &ids).await,
        map(&[("FAILED", 5)]),
        "a refused run resets nothing"
    );

    // Fewer matches than confirmed is fine (some were handled meanwhile).
    let mut body = filter;
    body["confirmCount"] = json!(7);
    let (status, res) = run(&app, &admin, body).await;
    assert_eq!(status, StatusCode::OK, "{res}");
    assert_eq!(
        res,
        json!({ "requeued": 5, "truncated": false, "remaining": 0 })
    );
}

/// 1,250 matches, limit 1,000: two batches of 500 run, 250 are left, and each
/// batch is one requeue run with its own event and audit row.
#[tokio::test]
#[ignore = "requires Docker"]
async fn it_walks_in_id_order_in_batches_and_truncates_at_the_limit() {
    let (app, admin) = setup().await;
    let code = "rbftest:batch:job:x";
    let ids = seed_jobs(&app, "rbbat", code, "FAILED", None, 1250).await;
    let filter = json!({ "codes": code, "statuses": "FAILED" });

    let (_, pv) = preview(&app, &admin, filter.clone()).await;
    assert_eq!(pv["eligibleCount"], 1250);
    let mut body = filter.clone();
    body["confirmCount"] = pv["eligibleCount"].clone();
    body["limit"] = json!(1000);
    let (status, res) = run(&app, &admin, body).await;
    assert_eq!(status, StatusCode::OK, "{res}");
    assert_eq!(
        res,
        json!({ "requeued": 1000, "truncated": true, "remaining": 250 })
    );
    // The walk is in id order: the first 1,000 ids were reset, the rest not.
    assert_eq!(
        statuses_of(&app, &ids[..1000]).await,
        map(&[("PENDING", 1000)])
    );
    assert_eq!(
        statuses_of(&app, &ids[1000..]).await,
        map(&[("FAILED", 250)])
    );

    let sizes: Vec<(i32,)> = sqlx::query_as(
        "SELECT jsonb_array_length(data::jsonb->'ids') FROM msg_events
          WHERE type = 'platform:messaging:dispatch-jobs:resent' AND data::jsonb->'ids' ?| $1
          ORDER BY 1",
    )
    .bind(&ids)
    .fetch_all(&app.pool)
    .await
    .unwrap();
    assert_eq!(
        sizes.into_iter().map(|(n,)| n).collect::<Vec<_>>(),
        [500, 500],
        "one resent event per batch"
    );
    let (audits,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM aud_logs WHERE operation = 'ResendCommand' AND operation_json::jsonb->'ids' ?| $1",
    )
    .bind(&ids)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(audits, 2, "one audit row per batch");

    // Once the projection has caught up, running again finishes the rest.
    sync_projection(&app, code).await;
    let (_, pv) = preview(&app, &admin, filter.clone()).await;
    assert_eq!(pv["eligibleCount"], 250);
    let mut body = filter;
    body["confirmCount"] = json!(250);
    body["limit"] = json!(1000);
    let (_, res) = run(&app, &admin, body).await;
    assert_eq!(
        res,
        json!({ "requeued": 250, "truncated": false, "remaining": 0 })
    );
    assert_eq!(statuses_of(&app, &ids).await, map(&[("PENDING", 1250)]));
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn only_the_matching_rows_are_reset_and_fully() {
    let (app, admin) = setup().await;
    let code = "rbftest:only:job:x";
    let failed = seed_jobs(&app, "rbof1", code, "FAILED", None, 4).await;
    let completed = seed_jobs(&app, "rbof2", code, "COMPLETED", None, 3).await;
    let other = seed_jobs(&app, "rbof3", "rbftest:only:job:other", "FAILED", None, 2).await;

    let (status, res) = run(
        &app,
        &admin,
        json!({ "codes": code, "statuses": "FAILED", "confirmCount": 4 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{res}");
    assert_eq!(
        res,
        json!({ "requeued": 4, "truncated": false, "remaining": 0 })
    );
    assert_eq!(statuses_of(&app, &failed).await, map(&[("PENDING", 4)]));
    assert_eq!(
        statuses_of(&app, &completed).await,
        map(&[("COMPLETED", 3)])
    );
    assert_eq!(statuses_of(&app, &other).await, map(&[("FAILED", 2)]));

    let (reset,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM msg_dispatch_jobs WHERE id = ANY($1) AND attempt_count = 0 AND last_error IS NULL",
    )
    .bind(&failed)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(
        reset, 4,
        "the requeue's full reset ran on every matched job"
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_tenant_caller_resets_only_its_own_jobs() {
    let (app, _) = setup().await;
    let code = "rbftest:tenant:job:x";
    let own = seed_jobs(&app, "rbtna", code, "FAILED", Some(CLIENT_A), 3).await;
    let theirs = seed_jobs(&app, "rbtnb", code, "FAILED", Some(CLIENT_B), 3).await;
    let platform = seed_jobs(&app, "rbtnp", code, "FAILED", None, 3).await;
    let viewer = client_viewer(&app, CLIENT_A, &[permissions::admin::DISPATCH_JOB_READ]).await;

    let (_, pv) = preview(&app, &viewer, json!({ "codes": code })).await;
    assert_eq!(
        (pv["count"].as_i64(), pv["eligibleCount"].as_i64()),
        (Some(6), Some(3))
    );
    let (status, res) = run(
        &app,
        &viewer,
        json!({ "codes": code, "confirmCount": pv["eligibleCount"] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{res}");
    assert_eq!(
        res,
        json!({ "requeued": 3, "truncated": false, "remaining": 0 })
    );
    assert_eq!(statuses_of(&app, &own).await, map(&[("PENDING", 3)]));
    assert_eq!(statuses_of(&app, &theirs).await, map(&[("FAILED", 3)]));
    assert_eq!(statuses_of(&app, &platform).await, map(&[("FAILED", 3)]));

    // Naming the other tenant explicitly reaches nothing either.
    let (_, res) = run(
        &app,
        &viewer,
        json!({ "codes": code, "clientIds": CLIENT_B, "confirmCount": 0 }),
    )
    .await;
    assert_eq!(
        res,
        json!({ "requeued": 0, "truncated": false, "remaining": 0 })
    );
    assert_eq!(statuses_of(&app, &theirs).await, map(&[("FAILED", 3)]));
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn without_the_view_permission_both_routes_are_refused() {
    let (app, _) = setup().await;
    let code = "rbftest:denied:job:x";
    let ids = seed_jobs(&app, "rbden", code, "FAILED", None, 2).await;
    let token = client_viewer(&app, CLIENT_A, &[permissions::admin::EVENT_READ]).await;

    let (status, body) = preview(&app, &token, json!({})).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let (status, body) = run(&app, &token, json!({ "codes": code, "confirmCount": 2 })).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(statuses_of(&app, &ids).await, map(&[("FAILED", 2)]));
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn bad_timestamps_and_limits_are_400() {
    let (app, admin) = setup().await;
    let (status, body) = preview(&app, &admin, json!({ "since": "yesterday" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "INVALID_TIMESTAMP", "{body}");
    let (status, body) = run(
        &app,
        &admin,
        json!({ "until": "2026-13-01", "confirmCount": 0 }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "INVALID_TIMESTAMP", "{body}");
    let (status, body) = run(&app, &admin, json!({ "confirmCount": 0, "limit": 10001 })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "INVALID_LIMIT", "{body}");
    let (status, body) = run(&app, &admin, json!({ "confirmCount": -1 })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "INVALID_CONFIRM_COUNT", "{body}");
}

/// The wire: both route prefixes, the JSON shapes, the required `confirmCount`
/// and the 409 envelope with the new counts.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_wire_on_both_tiers() {
    let (app, admin) = setup().await;
    let code = "rbftest:wire:job:x";
    let ids = seed_jobs(&app, "rbwir", code, "FAILED", None, 3).await;

    let (status, body) = read_json(
        app.post(
            "/bff/dispatch-jobs/requeue-by-filter/preview",
            &admin,
            json!({ "codes": code, "statuses": "FAILED" }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        json!({ "count": 3, "byStatus": { "FAILED": 3 }, "eligibleCount": 3 })
    );

    let (status, body) = run(&app, &admin, json!({ "codes": code })).await;
    assert!(
        status.is_client_error(),
        "confirmCount is required: {status} {body}"
    );
    assert_eq!(statuses_of(&app, &ids).await, map(&[("FAILED", 3)]));

    let (status, body) = run(&app, &admin, json!({ "codes": code, "confirmCount": 2 })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "FILTER_MATCH_CHANGED");
    assert_eq!(
        body["details"],
        json!({ "count": 3, "eligibleCount": 3, "byStatus": { "FAILED": 3 } })
    );

    let (status, body) = read_json(
        app.post(
            "/bff/dispatch-jobs/requeue-by-filter",
            &admin,
            json!({ "codes": code, "confirmCount": 3 }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        json!({ "requeued": 3, "truncated": false, "remaining": 0 })
    );
    assert_eq!(statuses_of(&app, &ids).await, map(&[("PENDING", 3)]));
}

/// A filter with no restricting member is "every job": it works, under the
/// same confirmCount check. The test owns its database.
#[tokio::test]
#[ignore = "requires Docker"]
async fn an_empty_filter_means_every_job() {
    let (app, admin) = setup().await;
    let failed = seed_jobs(&app, "rbnf1", "rbftest:nofilter:job:a", "FAILED", None, 20).await;
    let done = seed_jobs(
        &app,
        "rbnf2",
        "rbftest:nofilter:job:b",
        "COMPLETED",
        None,
        10,
    )
    .await;

    let (_, pv) = preview(&app, &admin, json!({})).await;
    assert_eq!(
        pv,
        json!({ "count": 30, "byStatus": { "FAILED": 20, "COMPLETED": 10 }, "eligibleCount": 30 })
    );
    let (status, _) = run(&app, &admin, json!({ "confirmCount": 29 })).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (_, res) = run(&app, &admin, json!({ "confirmCount": 30 })).await;
    assert_eq!(
        res,
        json!({ "requeued": 30, "truncated": false, "remaining": 0 })
    );
    let all: Vec<String> = failed.into_iter().chain(done).collect();
    assert_eq!(statuses_of(&app, &all).await, map(&[("PENDING", 30)]));
}
