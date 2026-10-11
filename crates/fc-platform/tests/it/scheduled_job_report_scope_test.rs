//! Go's F1 (`5d63669`, docs/platform-bug-fixes.md in flowcatalyst-go): an
//! application's SDK reports on the firings it receives (log lines, the
//! completion) with its own service account, which holds the seeded
//! application-service role (`scheduled-job-instance:write`), not the
//! messaging scheduled-job grants. It reports on its own application's jobs
//! only; the messaging grants keep reporting on any firing; nothing else is
//! admitted. Requires Docker (or `FC_TEST_PG_BIN`).

use axum::http::StatusCode;
use serde_json::{json, Value};

use crate::application_scope_test::{create_app, token_for};
use crate::support::{read_json, TestApp};
use fc_common::tsid::{self, EntityType};
use fc_platform::domain::{Principal, UserScope};
use fc_platform::role::entity::permissions::admin;

/// An anchor-tier token whose `scope` grants exactly `perms` (a principal
/// that is not stored: it reaches no application).
fn anchor_holding(app: &TestApp, perms: &[&str]) -> String {
    let p = Principal::new_user("sj-report@flowcatalyst.test", UserScope::Anchor);
    let granted: Vec<String> = perms.iter().map(|s| s.to_string()).collect();
    app.auth_service
        .generate_access_token_with_scope(&p, &granted, None)
        .expect("token")
}

/// A job of `application_id` (none: a job of no application) and one
/// delivered firing of it; returns the firing's id.
async fn firing(app: &TestApp, code: &str, application_id: Option<&str>) -> String {
    let job_id = tsid::generate(EntityType::ScheduledJob);
    sqlx::query(
        "INSERT INTO msg_scheduled_jobs (id, code, name, status, crons, application_id)
         VALUES ($1, $2, $2, 'ACTIVE', '{}', $3)",
    )
    .bind(&job_id)
    .bind(code)
    .bind(application_id)
    .execute(&app.pool)
    .await
    .expect("insert job");
    let instance_id = tsid::generate(EntityType::ScheduledJobInstance);
    sqlx::query(
        "INSERT INTO msg_scheduled_job_instances
           (id, scheduled_job_id, job_code, trigger_kind, fired_at, status, delivery_attempts,
            created_at)
         VALUES ($1, $2, $3, 'MANUAL', NOW(), 'DELIVERED', 1, NOW())",
    )
    .bind(&instance_id)
    .bind(&job_id)
    .bind(code)
    .execute(&app.pool)
    .await
    .expect("insert instance");
    instance_id
}

/// Log a line on the firing, then complete it.
async fn report(app: &TestApp, token: &str, instance_id: &str) -> [(StatusCode, Value); 2] {
    let log = read_json(
        app.post(
            &format!("/api/scheduled-jobs/instances/{instance_id}/log"),
            token,
            json!({ "level": "INFO", "message": "ran" }),
        )
        .await,
    )
    .await;
    let complete = read_json(
        app.post(
            &format!("/api/scheduled-jobs/instances/{instance_id}/complete"),
            token,
            json!({ "status": "SUCCESS" }),
        )
        .await,
    )
    .await;
    [log, complete]
}

fn refused_with(results: &[(StatusCode, Value); 2], code: &str, what: &str) {
    for (status, body) in results {
        assert_eq!(*status, StatusCode::FORBIDDEN, "{what}: {body}");
        assert_eq!(body["error"], code, "{what}: {body}");
    }
}

fn accepted(results: &[(StatusCode, Value); 2], what: &str) {
    for (status, body) in results {
        assert_eq!(*status, StatusCode::NO_CONTENT, "{what}: {body}");
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn application_service_account_reports_only_on_its_own_jobs() {
    let app = TestApp::setup().await;
    let own = create_app(&app, "sjrpt-own").await;
    let other = create_app(&app, "sjrpt-other").await;
    let own_firing = firing(&app, "sjrpt-own-job", Some(own.id.as_str())).await;
    let other_firing = firing(&app, "sjrpt-other-job", Some(other.id.as_str())).await;
    let no_app_firing = firing(&app, "sjrpt-noapp-job", None).await;

    // The application's service account: the seeded application-service
    // role, bound to its own application only.
    let svc = Principal::new_service("sjrpt-svc", "sjrpt-svc", UserScope::Anchor)
        .with_application_id(own.id.clone());
    let svc = token_for(&app, svc, &[&own]).await;

    // Its own application's firing: logged and completed.
    accepted(&report(&app, &svc, &own_firing).await, "own firing");
    let (status, logs): (String, i64) = sqlx::query_as(
        "SELECT status, (SELECT COUNT(*) FROM msg_scheduled_job_instance_logs WHERE instance_id = $1)
           FROM msg_scheduled_job_instances WHERE id = $1",
    )
    .bind(&own_firing)
    .fetch_one(&app.pool)
    .await
    .expect("read firing");
    assert_eq!(status, "COMPLETED");
    assert_eq!(logs, 1);

    // Another application's firing, and a firing of a job of no application.
    for (id, what) in [
        (&other_firing, "another application's firing"),
        (&no_app_firing, "a firing of no application"),
    ] {
        refused_with(
            &report(&app, &svc, id).await,
            "APPLICATION_ACCESS_REQUIRED",
            what,
        );
    }

    // Neither grant (the job view only): refused.
    let viewer = anchor_holding(&app, &[admin::SCHEDULED_JOB_READ]);
    refused_with(
        &report(&app, &viewer, &own_firing).await,
        "PERMISSION_REQUIRED",
        "viewer",
    );

    // A messaging scheduled-job write grant reports on any firing, as before.
    let writer = anchor_holding(&app, &[admin::SCHEDULED_JOB_UPDATE]);
    for (id, what) in [
        (&other_firing, "writer, another application"),
        (&no_app_firing, "writer, no application"),
    ] {
        accepted(&report(&app, &writer, id).await, what);
    }
}
