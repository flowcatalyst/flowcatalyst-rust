//! The status guard on requeue and redrive, the double confirmation and the
//! split write permissions (Go `ad37de21`,
//! `dispatchjob/api/window_guard_pg_test.go`): `POST /api/dispatch-jobs/requeue`,
//! `requeue-by-filter` and its preview, cancel, complete and edit-payload.
//! Requires a Postgres (see `support::db`).

use crate::support;

use axum::http::StatusCode;
use fc_platform::role::entity::AuthRole;
use fc_platform_core::permissions::admin as perms;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use support::{read_json, TestApp};

const REQUEUE: &str = perms::DISPATCH_JOB_REQUEUE;
const REDRIVE: &str = perms::DISPATCH_JOB_REDRIVE_COMPLETED;

/// One job in the write table and the read projection (the projector does not
/// run in these tests), `code` shared by a test's jobs. Ids are 13 characters.
async fn seed(app: &TestApp, id: &str, code: &str, status: &str) {
    sqlx::query(
        "INSERT INTO msg_dispatch_jobs
             (id, code, source, target_url, status, attempt_count, last_error, payload, created_at, updated_at)
         VALUES ($1, $2, 'guard-test', 'http://example.invalid/hook', $3, 3, 'boom', '{}',
                 now() - interval '1 hour', now() - interval '1 hour')",
    )
    .bind(id)
    .bind(code)
    .bind(status)
    .execute(&app.pool)
    .await
    .expect("seed write row");
    sqlx::query(
        "INSERT INTO msg_dispatch_jobs_read
             (id, code, target_url, kind, protocol, mode, status, max_retries, attempt_count,
              sequence, timeout_seconds, retry_strategy, created_at, updated_at)
         VALUES ($1, $2, 'http://example.invalid/hook', 'EVENT', 'HTTP_WEBHOOK', 'IMMEDIATE', $3, 3, 3,
                 99, 30, 'exponential', now() - interval '1 hour', now() - interval '1 hour')",
    )
    .bind(id)
    .bind(code)
    .bind(status)
    .execute(&app.pool)
    .await
    .expect("seed read row");
}

/// The projector's job: the read rows take the write rows' status.
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

struct Guard {
    code: String,
    requeueable: Vec<String>,
    queued: String,
    processing: String,
    completed: String,
    failed: String,
}

impl Guard {
    fn all(&self) -> Vec<String> {
        let mut v = self.requeueable.clone();
        v.extend([
            self.queued.clone(),
            self.processing.clone(),
            self.completed.clone(),
        ]);
        v
    }
}

/// PENDING, FAILED, CANCELLED, EXPIRED and the legacy ERROR (requeueable),
/// QUEUED and PROCESSING (in flight), one COMPLETED.
async fn seed_guard(app: &TestApp, p: &str) -> Guard {
    assert_eq!(p.len(), 4);
    let code = format!("grdtest:{p}:job:x");
    let mut requeueable = Vec::new();
    let one = |suffix: &str, status: &str| {
        let id = format!("{p}{suffix}000000000")[..13].to_string();
        (id, status.to_string())
    };
    let mut rows = Vec::new();
    for (suffix, status) in [
        ("p", "PENDING"),
        ("f", "FAILED"),
        ("c", "CANCELLED"),
        ("e", "EXPIRED"),
        ("x", "ERROR"),
    ] {
        let (id, st) = one(suffix, status);
        requeueable.push(id.clone());
        rows.push((id, st));
    }
    let (queued, st_q) = one("q", "QUEUED");
    let (processing, st_r) = one("r", "PROCESSING");
    let (completed, st_d) = one("d", "COMPLETED");
    rows.extend([
        (queued.clone(), st_q),
        (processing.clone(), st_r),
        (completed.clone(), st_d),
    ]);
    for (id, status) in &rows {
        seed(app, id, &code, status).await;
    }
    Guard {
        code,
        failed: requeueable[1].clone(),
        requeueable,
        queued,
        processing,
        completed,
    }
}

async fn status_of(app: &TestApp, id: &str) -> String {
    sqlx::query_scalar("SELECT status FROM msg_dispatch_jobs WHERE id = $1")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .unwrap()
}

/// An anchor-scoped caller holding exactly `held`.
async fn caller(app: &TestApp, held: &[&str]) -> String {
    use fc_platform::domain::{Principal, UserScope};
    use fc_platform::service_account::entity::RoleAssignment;
    let name = format!("grd-{}", held.join("+").replace([':', '*'], "_"));
    let role_code = format!("grd:{name}");
    if app
        .repos
        .role_repo
        .find_by_name(&role_code)
        .await
        .unwrap()
        .is_none()
    {
        let mut role = AuthRole::new("grd", name, "Guard role");
        for p in held {
            role = role.with_permission(*p);
        }
        app.repos.role_repo.insert(&role).await.unwrap();
    }
    let mut user = Principal::new_user("who@grd.test", UserScope::Anchor);
    user.roles = vec![RoleAssignment::new(role_code)];
    app.auth_service.generate_access_token(&user).unwrap()
}

async fn requeue(app: &TestApp, token: &str, body: Value) -> (StatusCode, Value) {
    read_json(app.post("/api/dispatch-jobs/requeue", token, body).await).await
}

fn skipped_reasons(body: &Value) -> BTreeMap<String, String> {
    body["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            assert!(s["status"].as_str().is_some_and(|s| !s.is_empty()));
            (
                s["id"].as_str().unwrap().to_string(),
                s["reason"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn by_ids_in_flight_and_completed_jobs_are_skipped_and_reported() {
    let app = TestApp::setup().await;
    let g = seed_guard(&app, "grd1").await;
    let token = caller(&app, &[REQUEUE]).await;

    let (status, body) = requeue(&app, &token, json!({ "ids": g.all() })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["requeued"], 5, "{body}");
    assert_eq!(body["redriven"], 0);
    assert_eq!(body["skippedInFlight"], 2);
    assert_eq!(body["skippedCompleted"], 1);
    assert_eq!(
        skipped_reasons(&body),
        BTreeMap::from([
            (g.queued.clone(), "IN_FLIGHT".to_string()),
            (g.processing.clone(), "IN_FLIGHT".to_string()),
            (
                g.completed.clone(),
                "COMPLETED_REDRIVE_NOT_CONFIRMED".to_string()
            ),
        ])
    );
    for id in &g.requeueable {
        assert_eq!(status_of(&app, id).await, "PENDING", "{id}");
    }
    assert_eq!(status_of(&app, &g.queued).await, "QUEUED");
    assert_eq!(status_of(&app, &g.processing).await, "PROCESSING");
    assert_eq!(status_of(&app, &g.completed).await, "COMPLETED");

    // An in-flight-only batch is not an error either; nor is an empty one.
    let (status, body) = requeue(&app, &token, json!({ "ids": [g.queued] })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        (body["requeued"].clone(), body["skippedInFlight"].clone()),
        (json!(0), json!(1))
    );
    let (status, body) = requeue(&app, &token, json!({ "ids": [] })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        json!({ "requeued": 0, "redriven": 0, "skippedInFlight": 0, "skippedCompleted": 0, "skipped": [] })
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_redrive_needs_both_fields_and_both_permissions() {
    let app = TestApp::setup().await;
    let g = seed_guard(&app, "grd2").await;
    let both = caller(&app, &[REQUEUE, REDRIVE]).await;

    // The flag alone, the word alone, or the wrong case: COMPLETED is skipped and reported.
    for req in [
        json!({ "ids": [g.completed], "redriveCompleted": true }),
        json!({ "ids": [g.completed], "confirmRedriveCompleted": "REDRIVE" }),
        json!({ "ids": [g.completed], "redriveCompleted": true, "confirmRedriveCompleted": "redrive" }),
    ] {
        let (status, body) = requeue(&app, &both, req).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            (body["requeued"].clone(), body["skippedCompleted"].clone()),
            (json!(0), json!(1))
        );
        assert_eq!(status_of(&app, &g.completed).await, "COMPLETED");
    }

    // Asking to redrive without the redrive permission is a 403, not a quiet
    // skip; and redrive-completed alone is not a requeue permission.
    let confirmed = json!({ "ids": [g.completed], "redriveCompleted": true, "confirmRedriveCompleted": "REDRIVE" });
    for held in [vec![REQUEUE], vec![REDRIVE]] {
        let token = caller(&app, &held).await;
        let (status, body) = requeue(&app, &token, confirmed.clone()).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{held:?}: {body}");
        assert_eq!(body["error"], "PERMISSION_REQUIRED", "{held:?}: {body}");
    }
    assert_eq!(status_of(&app, &g.completed).await, "COMPLETED");

    // Both fields and both permissions: redriven, with the in-flight still skipped.
    let mut ids = g.all();
    ids.push("nosuchjob".into());
    let (status, body) = requeue(
        &app,
        &both,
        json!({ "ids": ids, "redriveCompleted": true, "confirmRedriveCompleted": "REDRIVE" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["requeued"], 6, "5 ordinary + 1 redriven: {body}");
    assert_eq!(body["redriven"], 1);
    assert_eq!(body["skippedInFlight"], 2);
    assert_eq!(body["skippedCompleted"], 0);
    assert_eq!(status_of(&app, &g.completed).await, "PENDING");
    assert_eq!(status_of(&app, &g.queued).await, "QUEUED");
    assert_eq!(status_of(&app, &g.processing).await, "PROCESSING");
    let attempts: i32 =
        sqlx::query_scalar("SELECT attempt_count FROM msg_dispatch_jobs WHERE id = $1")
            .bind(&g.completed)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(attempts, 0, "a redriven job gets a fresh retry budget");

    // The audit trail tells a redrive apart: the event and the audit row say so.
    let (redrive, count, redriven_ids): (bool, i32, Value) = sqlx::query_as(
        "SELECT (data::jsonb->>'redrive')::bool, (data::jsonb->>'redrivenCount')::int, data::jsonb->'redrivenIds'
           FROM msg_events WHERE type = 'platform:messaging:dispatch-jobs:resent' AND data::jsonb->'ids' ? $1",
    )
    .bind(&g.completed)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert!(redrive);
    assert_eq!(count, 1);
    assert_eq!(redriven_ids, json!([g.completed]));
    let (flagged,): (bool,) = sqlx::query_as(
        "SELECT (operation_json::jsonb->>'redriveCompleted')::bool FROM aud_logs
          WHERE operation = 'ResendCommand' AND operation_json::jsonb->'ids' ? $1
            AND operation_json::jsonb->>'redriveCompleted' = 'true'",
    )
    .bind(&g.completed)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert!(flagged, "the audit row carries redriveCompleted=true");

    // An ordinary requeue's event says redrive=false and leaves redrivenIds out.
    let (status, body) = requeue(&app, &both, json!({ "ids": [g.failed] })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (data,): (Value,) = sqlx::query_as(
        "SELECT data::jsonb FROM msg_events
          WHERE type = 'platform:messaging:dispatch-jobs:resent' AND data::jsonb->'ids' ? $1
          ORDER BY id DESC LIMIT 1",
    )
    .bind(&g.failed)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(data["redrive"], false);
    assert!(data.get("redrivenIds").is_none(), "{data}");
    assert_eq!(data["skippedInFlight"], 0);
}

async fn preview(app: &TestApp, token: &str, body: Value) -> Value {
    let (status, got) = read_json(
        app.post("/api/dispatch-jobs/requeue-by-filter/preview", token, body)
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{got}");
    got
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
async fn by_filter_preview_reports_completed_and_in_flight_apart_and_the_run_guards_them() {
    let app = TestApp::setup().await;
    let g = seed_guard(&app, "grd4").await;
    let filter = json!({ "codes": g.code });
    let token = caller(&app, &[REQUEUE, REDRIVE]).await;

    let pv = preview(&app, &token, filter.clone()).await;
    assert_eq!(pv["count"], 8, "{pv}");
    assert_eq!(
        pv["eligibleCount"], 5,
        "pending, failed, cancelled, expired, error: {pv}"
    );
    assert_eq!(
        pv["completedCount"], 1,
        "reported separately so a UI can ask twice"
    );
    assert_eq!(pv["inFlightCount"], 2);
    assert_eq!(pv["redriveEligibleCount"], 6);
    assert_eq!(pv["byStatus"]["COMPLETED"], 1);

    // An ordinary run: completed and in-flight are skipped and counted.
    let mut body = filter.clone();
    body["confirmCount"] = json!(5);
    let (status, res) = run(&app, &token, body).await;
    assert_eq!(status, StatusCode::OK, "{res}");
    assert_eq!(
        (
            res["requeued"].clone(),
            res["redriven"].clone(),
            res["skippedInFlight"].clone(),
            res["skippedCompleted"].clone()
        ),
        (json!(5), json!(0), json!(2), json!(1)),
        "{res}"
    );
    assert_eq!(res["truncated"], false);
    assert_eq!(status_of(&app, &g.completed).await, "COMPLETED");
    assert_eq!(status_of(&app, &g.queued).await, "QUEUED");

    // Redrive asked for but only half-confirmed: COMPLETED stays COMPLETED.
    sync_projection(&app, &g.code).await;
    let mut body = filter.clone();
    body["confirmCount"] = json!(10);
    body["redriveCompleted"] = json!(true);
    let (status, res) = run(&app, &token, body).await;
    assert_eq!(status, StatusCode::OK, "{res}");
    assert_eq!(res["redriven"], 0);
    assert_eq!(status_of(&app, &g.completed).await, "COMPLETED");

    // A confirmed redrive confirms eligible + completed; fewer than that is a 409
    // whose details carry every count.
    let pv = preview(&app, &token, filter.clone()).await;
    let mut body = filter.clone();
    body["confirmCount"] = pv["eligibleCount"].clone();
    body["redriveCompleted"] = json!(true);
    body["confirmRedriveCompleted"] = json!("REDRIVE");
    let (status, err) = run(&app, &token, body).await;
    assert_eq!(status, StatusCode::CONFLICT, "{err}");
    assert_eq!(err["error"], "FILTER_MATCH_CHANGED");
    for key in [
        "count",
        "eligibleCount",
        "completedCount",
        "inFlightCount",
        "redriveEligibleCount",
        "byStatus",
    ] {
        assert!(
            err["details"].get(key).is_some(),
            "details lack {key}: {err}"
        );
    }
    assert_eq!(
        err["details"]["redriveEligibleCount"],
        pv["redriveEligibleCount"]
    );
    assert_eq!(status_of(&app, &g.completed).await, "COMPLETED");

    let mut body = filter.clone();
    body["confirmCount"] = pv["redriveEligibleCount"].clone();
    body["redriveCompleted"] = json!(true);
    body["confirmRedriveCompleted"] = json!("REDRIVE");
    let (status, res) = run(&app, &token, body).await;
    assert_eq!(status, StatusCode::OK, "{res}");
    assert_eq!(res["redriven"], 1, "{res}");
    assert_eq!(status_of(&app, &g.completed).await, "PENDING");
    assert_eq!(
        status_of(&app, &g.queued).await,
        "QUEUED",
        "in flight is never reset, even by a redrive"
    );
    assert_eq!(status_of(&app, &g.processing).await, "PROCESSING");

    // Without the redrive permission, asking is a 403 and nothing runs.
    let plain = caller(&app, &[REQUEUE]).await;
    let (status, err) = run(
        &app,
        &plain,
        json!({ "codes": g.code, "confirmCount": 99, "redriveCompleted": true, "confirmRedriveCompleted": "REDRIVE" }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{err}");
    assert_eq!(err["error"], "PERMISSION_REQUIRED");
}

/// Each write permission, held alone, allows exactly its own action; view
/// allows no write.
#[tokio::test]
#[ignore = "requires Docker"]
async fn each_write_action_has_its_own_permission() {
    let app = TestApp::setup().await;
    let mut n = 0;
    let mut fresh_failed = |app: &TestApp| {
        n += 1;
        let (id, code) = (format!("grdm{n:09}"), format!("grdtest:m{n}:job:x"));
        let app_pool = app.pool.clone();
        async move {
            sqlx::query(
                "INSERT INTO msg_dispatch_jobs (id, code, source, target_url, status, attempt_count, payload, created_at, updated_at)
                 VALUES ($1, $2, 'guard-test', 'http://example.invalid/hook', 'FAILED', 1, '{}', now(), now())",
            )
            .bind(&id)
            .bind(&code)
            .execute(&app_pool)
            .await
            .unwrap();
            id
        }
    };
    const ACTIONS: [&str; 5] = ["requeue", "redrive", "cancel", "complete", "edit"];
    let permission_of = |action: &str| match action {
        "requeue" => REQUEUE,
        "redrive" => REDRIVE,
        "cancel" => perms::DISPATCH_JOB_CANCEL,
        "complete" => perms::DISPATCH_JOB_COMPLETE,
        _ => perms::DISPATCH_JOB_EDIT_PAYLOAD,
    };
    // Whether `token` is allowed `action` (anything but 403).
    let mut try_action = async |app: &TestApp, token: &str, action: &str, bff: bool| -> bool {
        let id = fresh_failed(app).await;
        let base = if bff {
            "/bff/dispatch-jobs"
        } else {
            "/api/dispatch-jobs"
        };
        let resp = match action {
            "requeue" => app.post(&format!("{base}/requeue"), token, json!({ "ids": [id] })).await,
            "redrive" => {
                app.post(
                    &format!("{base}/requeue"),
                    token,
                    json!({ "ids": [id], "redriveCompleted": true, "confirmRedriveCompleted": "REDRIVE" }),
                )
                .await
            }
            "cancel" => app.post(&format!("{base}/{id}/cancel"), token, json!({})).await,
            "complete" => app.post(&format!("{base}/{id}/complete"), token, json!({})).await,
            _ => app.put(&format!("{base}/{id}/payload"), token, json!({ "payload": "{\"a\":1}" })).await,
        };
        let (status, body) = read_json(resp).await;
        if status == StatusCode::FORBIDDEN {
            assert_eq!(body["error"], "PERMISSION_REQUIRED", "{action}: {body}");
            return false;
        }
        assert!(
            status.is_success(),
            "{action} failed for a reason other than permission: {status} {body}"
        );
        true
    };

    for held in ACTIONS {
        let token = caller(&app, &[permission_of(held)]).await;
        for action in ACTIONS {
            let allowed = try_action(&app, &token, action, false).await;
            // A redrive request also resets what is not COMPLETED, so
            // redrive-completed alone is not enough for it.
            assert_eq!(
                allowed,
                held == action && action != "redrive",
                "holding only {held}, attempting {action}"
            );
        }
    }
    let both = caller(&app, &[REQUEUE, REDRIVE]).await;
    assert!(
        try_action(&app, &both, "redrive", false).await,
        "requeue + redrive-completed allows a redrive"
    );
    // `dispatch-job:view` and view-raw allow no write; the /bff mounts gate the same.
    let view = caller(
        &app,
        &[perms::DISPATCH_JOB_READ, perms::DISPATCH_JOB_VIEW_RAW],
    )
    .await;
    for action in ACTIONS {
        assert!(
            !try_action(&app, &view, action, false).await,
            "view alone must not allow {action}"
        );
        assert!(
            !try_action(&app, &view, action, true).await,
            "view alone must not allow {action} on /bff"
        );
    }
    // The wildcard (super-admin) holds all of them.
    let admin = app.anchor_admin_token_all_applications().await;
    assert!(try_action(&app, &admin, "redrive", false).await);
    assert!(try_action(&app, &admin, "edit", true).await);
}

/// The preview is part of requeueing and is also a read of counts: requeue or
/// view opens it, nothing else does.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_preview_admits_requeue_or_view_only() {
    let app = TestApp::setup().await;
    for p in [REQUEUE, perms::DISPATCH_JOB_READ] {
        let token = caller(&app, &[p]).await;
        let (status, body) = read_json(
            app.post(
                "/api/dispatch-jobs/requeue-by-filter/preview",
                &token,
                json!({ "codes": "grdtest:none:job:x" }),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{p}: {body}");
    }
    for p in [
        perms::DISPATCH_JOB_CANCEL,
        perms::DISPATCH_JOB_COMPLETE,
        perms::DISPATCH_JOB_EDIT_PAYLOAD,
        REDRIVE,
    ] {
        let token = caller(&app, &[p]).await;
        let (status, body) = read_json(
            app.post(
                "/api/dispatch-jobs/requeue-by-filter/preview",
                &token,
                json!({}),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{p}: {body}");
        assert_eq!(body["error"], "PERMISSION_REQUIRED");
    }
}
