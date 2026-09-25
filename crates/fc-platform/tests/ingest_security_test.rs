//! Ingest security: who may write events and dispatch jobs, under which
//! client, and signed by whom (docs/parity/java-2026-09-25-triage.md S1, S5,
//! S6; owner decision #24). Requires Docker.

#[path = "support/mod.rs"]
mod support;

use axum::http::StatusCode;
use serde_json::{json, Value};

use fc_platform::client::entity::Client;
use fc_platform::domain::{Principal, UserScope};
use fc_platform::permissions;
use support::{read_json, TestApp};

const EVENTS_WRITE: &str = permissions::admin::BATCH_EVENTS_WRITE;
const JOBS_WRITE: &str = permissions::admin::BATCH_DISPATCH_JOBS_WRITE;

/// A token for `principal` granting exactly `perms` on its `scope` claim
/// (none: the principal's roles, of which it has none).
fn token_for(app: &TestApp, principal: &Principal, perms: &[&str]) -> String {
    let granted: Vec<String> = perms.iter().map(|p| p.to_string()).collect();
    app.auth_service
        .generate_access_token_with_scope(principal, &granted, None)
        .expect("token")
}

fn anchor_user() -> Principal {
    Principal::new_user("anchor@flowcatalyst.test", UserScope::Anchor)
}

/// A client-scoped user whose token names its client as `id:identifier`.
fn client_user(client_id: &str, identifier: &str) -> Principal {
    let mut p = Principal::new_user("client@flowcatalyst.test", UserScope::Client)
        .with_client_id(client_id);
    p.client_identifier_map
        .insert(client_id.to_string(), identifier.to_string());
    p
}

fn partner_user(client_ids: &[&str]) -> Principal {
    let mut p = Principal::new_user("partner@flowcatalyst.test", UserScope::Partner);
    p.assigned_clients = client_ids.iter().map(|c| c.to_string()).collect();
    p
}

async fn create_client(app: &TestApp, identifier: &str) -> String {
    let client = Client::new(identifier.to_uppercase(), identifier);
    app.repos
        .client_repo
        .insert(&client)
        .await
        .expect("insert client");
    client.id
}

fn event_item(event_type: &str) -> Value {
    json!({"type": event_type, "source": "test", "data": {"k": "v"}})
}

fn job_item(code: &str) -> Value {
    json!({
        "code": code,
        "targetUrl": "https://receiver.example.test/hook",
        "payload": "{\"k\":\"v\"}",
        "serviceAccountId": "sac_unused"
    })
}

async fn post(app: &TestApp, path: &str, token: &str, body: Value) -> (StatusCode, Value) {
    read_json(app.post(path, token, body).await).await
}

async fn count(app: &TestApp, sql: &str) -> i64 {
    let (n,): (i64,) = sqlx::query_as(sql)
        .fetch_one(&app.pool)
        .await
        .expect("count");
    n
}

// ── S1: the ingest permissions ──────────────────────────────────────────────

/// Every ingest route asks for Go's permission and answers Go's body without
/// it; the application-service `event:create` grant is not it.
#[tokio::test]
#[ignore = "requires Docker"]
async fn ingest_routes_require_the_batch_write_permissions() {
    let app = TestApp::setup().await;
    let none = token_for(&app, &anchor_user(), &[]);
    let app_event_create = token_for(
        &app,
        &anchor_user(),
        &[permissions::application_service::EVENT_CREATE],
    );

    let event = json!({"eventType": "x:y:z:created", "source": "t", "data": {}});
    for (path, body, needs) in [
        (
            "/api/events/batch",
            json!({"items": [event_item("x:y:z:created")]}),
            EVENTS_WRITE,
        ),
        ("/api/events", event.clone(), EVENTS_WRITE),
        (
            "/api/dispatch-jobs/batch",
            json!({"items": [job_item("x:y:z:job")]}),
            JOBS_WRITE,
        ),
        ("/api/dispatch-jobs", job_item("x:y:z:job"), JOBS_WRITE),
    ] {
        for token in [&none, &app_event_create] {
            let (status, body) = post(&app, path, token, body.clone()).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {body}");
            assert_eq!(body["error"], "PERMISSION_REQUIRED", "{path}: {body}");
            assert_eq!(
                body["message"],
                format!("permission required: {needs}"),
                "{path}"
            );
        }
    }
    assert_eq!(count(&app, "SELECT COUNT(*) FROM msg_events").await, 0);
    assert_eq!(
        count(&app, "SELECT COUNT(*) FROM msg_dispatch_jobs").await,
        0
    );

    // With the permissions, the same requests are written.
    let ingest = token_for(&app, &anchor_user(), &[EVENTS_WRITE, JOBS_WRITE]);
    let (status, body) = post(
        &app,
        "/api/events/batch",
        &ingest,
        json!({"items": [event_item("x:y:z:created")]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = post(
        &app,
        "/api/dispatch-jobs/batch",
        &ingest,
        json!({"items": [job_item("x:y:z:job")]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

// ── Decision #24: ingest tenancy ────────────────────────────────────────────

/// A non-anchor writes only under a client it can access: no client means
/// its only client, or a refusal when it has several; an unknown
/// `clientCode` is refused. Nothing of a refused batch is written.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_non_anchor_ingests_only_under_a_client_it_can_access() {
    let app = TestApp::setup().await;
    let acme = create_client(&app, "acme").await;
    let other = create_client(&app, "other").await;

    let single = token_for(
        &app,
        &client_user(&acme, "acme"),
        &[EVENTS_WRITE, JOBS_WRITE],
    );
    let partner = token_for(
        &app,
        &partner_user(&[&acme, &other]),
        &[EVENTS_WRITE, JOBS_WRITE],
    );

    // A single-client caller's absent client is its client, for both events
    // and jobs, and on the single-event route too.
    let (status, body) = post(
        &app,
        "/api/events/batch",
        &single,
        json!({"items": [event_item("t:a:b:single")]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = post(
        &app,
        "/api/dispatch-jobs/batch",
        &single,
        json!({"items": [job_item("t:a:b:single")]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = post(
        &app,
        "/api/events",
        &single,
        json!({"eventType": "t:a:b:single-one", "source": "t", "data": {}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["event"]["clientId"], json!(acme), "{body}");
    let (client_id,): (Option<String>,) =
        sqlx::query_as("SELECT client_id FROM msg_events WHERE type = 't:a:b:single'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(client_id.as_deref(), Some(acme.as_str()));
    let (client_id,): (Option<String>,) =
        sqlx::query_as("SELECT client_id FROM msg_dispatch_jobs WHERE code = 't:a:b:single'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(client_id.as_deref(), Some(acme.as_str()));

    // A caller with several clients must name one.
    let mut named = event_item("t:a:b:named");
    named["clientId"] = json!(other);
    for (path, body) in [
        (
            "/api/events/batch",
            json!({"items": [named.clone(), event_item("t:a:b:unnamed")]}),
        ),
        (
            "/api/dispatch-jobs/batch",
            json!({"items": [job_item("t:a:b:unnamed")]}),
        ),
    ] {
        let (status, body) = post(&app, path, &partner, body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {body}");
        assert!(
            body["message"]
                .as_str()
                .unwrap()
                .contains("clientId is required"),
            "{path}: {body}"
        );
    }

    // An unknown clientCode is no client, refused; so is another tenant's.
    let mut unknown = event_item("t:a:b:unknown");
    unknown["clientCode"] = json!("no-such-tenant");
    let (status, body) = post(
        &app,
        "/api/events/batch",
        &single,
        json!({"items": [unknown]}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["message"], "No access to client: no-such-tenant");
    let mut foreign = event_item("t:a:b:foreign");
    foreign["clientCode"] = json!("other");
    let (status, body) = post(
        &app,
        "/api/events/batch",
        &single,
        json!({"items": [foreign]}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let mut foreign_job = job_item("t:a:b:foreign");
    foreign_job["clientId"] = json!(other);
    let (status, body) = post(
        &app,
        "/api/dispatch-jobs/batch",
        &single,
        json!({"items": [foreign_job]}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    // None of the refused batches wrote anything.
    assert_eq!(
        count(
            &app,
            "SELECT COUNT(*) FROM msg_events WHERE type IN \
             ('t:a:b:named', 't:a:b:unnamed', 't:a:b:unknown', 't:a:b:foreign')"
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &app,
            "SELECT COUNT(*) FROM msg_dispatch_jobs WHERE code <> 't:a:b:single'"
        )
        .await,
        0
    );

    // A named, accessible client is written as named.
    let (status, body) = post(
        &app,
        "/api/events/batch",
        &partner,
        json!({"items": [named]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // An anchor keeps today's behaviour: no client is platform-scoped, and
    // an unknown code leaves the event unlinked.
    let anchor = token_for(&app, &anchor_user(), &[EVENTS_WRITE]);
    let mut unknown = event_item("t:a:b:anchor");
    unknown["clientCode"] = json!("no-such-tenant");
    let (status, body) = post(
        &app,
        "/api/events/batch",
        &anchor,
        json!({"items": [unknown]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (client_id,): (Option<String>,) =
        sqlx::query_as("SELECT client_id FROM msg_events WHERE type = 't:a:b:anchor'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(client_id, None);
}

// ── Decision #24: supplied ids ──────────────────────────────────────────────

/// A dispatch-job id the SDK supplies is the job's. One that already names a
/// job, or repeats within the batch, refuses the whole batch 409
/// `DUPLICATE_ID` and writes nothing; one that cannot fit the column is 400.
#[tokio::test]
#[ignore = "requires Docker"]
async fn supplied_dispatch_job_ids_are_honoured_and_never_reused() {
    let app = TestApp::setup().await;
    let token = token_for(&app, &anchor_user(), &[JOBS_WRITE]);
    let with_id = |id: &str, code: &str| {
        let mut j = job_item(code);
        j["id"] = json!(id);
        j
    };

    let (status, body) = post(
        &app,
        "/api/dispatch-jobs/batch",
        &token,
        json!({"items": [with_id("0SUPPLIED0001", "t:j:a:first"), job_item("t:j:a:minted")]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["results"][0]["id"], "0SUPPLIED0001");
    assert_ne!(body["results"][1]["id"], "0SUPPLIED0001");
    assert_eq!(
        count(
            &app,
            "SELECT COUNT(*) FROM msg_dispatch_jobs WHERE id = '0SUPPLIED0001' AND code = 't:j:a:first'"
        )
        .await,
        1
    );

    // Re-sent: the id is taken, so nothing in the batch is written.
    let (status, body) = post(
        &app,
        "/api/dispatch-jobs/batch",
        &token,
        json!({"items": [job_item("t:j:a:second"), with_id("0SUPPLIED0001", "t:j:a:again")]}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "DUPLICATE_ID");
    assert_eq!(
        body["message"],
        "dispatch job id already exists: 0SUPPLIED0001"
    );

    // Repeated within one batch.
    let (status, body) = post(
        &app,
        "/api/dispatch-jobs/batch",
        &token,
        json!({"items": [with_id("0SUPPLIED0002", "t:j:a:second"), with_id("0SUPPLIED0002", "t:j:a:again")]}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "DUPLICATE_ID");

    // Too long for VARCHAR(13): a 400, not a 500.
    let (status, body) = post(
        &app,
        "/api/dispatch-jobs/batch",
        &token,
        json!({"items": [with_id("0SUPPLIED00030", "t:j:a:second")]}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "INVALID_ID");

    assert_eq!(
        count(
            &app,
            "SELECT COUNT(*) FROM msg_dispatch_jobs WHERE code IN ('t:j:a:second', 't:j:a:again')"
        )
        .await,
        0
    );
}

/// An event id the SDK supplies is the event's; the same event re-sent (an
/// outbox retry, with no deduplication id of its own) is acknowledged and
/// stored once.
#[tokio::test]
#[ignore = "requires Docker"]
async fn supplied_event_ids_are_honoured_and_idempotent() {
    let app = TestApp::setup().await;
    let token = token_for(&app, &anchor_user(), &[EVENTS_WRITE]);
    let mut item = event_item("t:e:a:supplied");
    item["id"] = json!("0EVENTSUPP001");

    for attempt in 0..2 {
        let (status, body) = post(
            &app,
            "/api/events/batch",
            &token,
            json!({"items": [item.clone()]}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "attempt {attempt}: {body}");
        assert_eq!(body["results"][0]["id"], "0EVENTSUPP001", "{body}");
        assert_eq!(body["results"][0]["status"], "SUCCESS", "{body}");
    }
    // Even with a different deduplication id, a stored id is not written twice.
    item["deduplicationId"] = json!("another");
    let (status, body) = post(&app, "/api/events/batch", &token, json!({"items": [item]})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        count(
            &app,
            "SELECT COUNT(*) FROM msg_events WHERE id = '0EVENTSUPP001'"
        )
        .await,
        1
    );
    let (dedup,): (Option<String>,) =
        sqlx::query_as("SELECT deduplication_id FROM msg_events WHERE id = '0EVENTSUPP001'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(dedup.as_deref(), Some("t:e:a:supplied-0EVENTSUPP001"));

    let mut bad = event_item("t:e:a:bad");
    bad["id"] = json!("not an id");
    let (status, body) = post(&app, "/api/events/batch", &token, json!({"items": [bad]})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "INVALID_ID");
}
