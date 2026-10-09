//! Application confinement on the SDK ingest endpoints: an application
//! service account holds the three batch-ingest permissions, and for a caller
//! that is not all-applications the endpoints refuse items belonging to other
//! applications. Callers with all-applications access are unaffected.
//! Requires Docker.

use crate::application_scope_test::create_app;
use crate::support;

use axum::http::StatusCode;
use serde_json::{json, Value};

use fc_platform::domain::{Principal, UserScope};
use fc_platform::permissions;
use fc_platform::shared::tsid;
use support::{read_json, TestApp};

const OWN_ERR: &str = "event type belongs to an application this credential cannot ingest for";

struct Fixture {
    app: TestApp,
    own: String,
    other: String,
    own_sub: String,
    other_sub: String,
    scoped: String,
    admin: String,
}

fn perms() -> Vec<String> {
    [
        permissions::admin::BATCH_EVENTS_WRITE,
        permissions::admin::BATCH_DISPATCH_JOBS_WRITE,
        permissions::admin::BATCH_AUDIT_LOGS_WRITE,
    ]
    .iter()
    .map(|p| p.to_string())
    .collect()
}

async fn create_subscription(app: &TestApp, code: &str, application_code: &str) -> String {
    let mut sub = fc_platform::Subscription::new(code, code, "https://receiver.example.test/hook");
    sub.application_code = Some(application_code.to_string());
    app.repos
        .subscription_repo
        .insert(&sub)
        .await
        .expect("insert subscription");
    sub.id.to_string()
}

async fn setup() -> Fixture {
    let app = TestApp::setup().await;
    let own_app = create_app(&app, "ownapp").await;
    create_app(&app, "otherapp").await;

    // The application's own service account: bound to `ownapp` only.
    let sac = tsid::generate(fc_platform::EntityType::ServiceAccount);
    sqlx::query(
        "INSERT INTO iam_service_accounts (id, code, name, application_id, active) VALUES ($1, 'ownapp-sa', 'ownapp-sa', $2, true)",
    )
    .bind(&sac)
    .bind(own_app.id.as_str())
    .execute(&app.pool)
    .await
    .expect("insert account");
    let mut sa = Principal::new_service(&sac, "ownapp-sa", UserScope::Anchor)
        .with_application_id(own_app.id.clone());
    app.repos.principal_repo.insert(&sa).await.expect("sa");
    sa.accessible_application_ids = vec![own_app.id.clone()];
    app.repos.principal_repo.update(&sa).await.expect("grant");
    let scoped = app
        .auth_service
        .generate_access_token_with_scope(&sa, &perms(), None)
        .expect("token");

    // An all-applications caller (users, by default).
    let admin_p = Principal::new_user("ingest-admin@flowcatalyst.test", UserScope::Anchor);
    app.repos
        .principal_repo
        .insert(&admin_p)
        .await
        .expect("admin");
    // A super-admin: an application's jobs are signed by that application, so
    // only it (or a super-admin) may ingest them.
    let mut admin_perms = perms();
    admin_perms.push(permissions::ADMIN_ALL.to_string());
    let admin = app
        .auth_service
        .generate_access_token_with_scope(&admin_p, &admin_perms, None)
        .expect("token");

    let own_sub = create_subscription(&app, "s-own", "ownapp").await;
    let other_sub = create_subscription(&app, "s-other", "otherapp").await;
    Fixture {
        app,
        own: "ownapp".into(),
        other: "otherapp".into(),
        own_sub,
        other_sub,
        scoped,
        admin,
    }
}

async fn post(f: &Fixture, path: &str, token: &str, body: Value) -> (StatusCode, Value) {
    read_json(f.app.post(path, token, body).await).await
}

fn event(t: &str) -> Value {
    json!({"type": t, "source": "test", "data": {"n": 1}})
}

fn job(code: &str, sub: Option<&str>) -> Value {
    let mut j = json!({
        "code": code,
        "targetUrl": "https://receiver.example.test/hook",
        "payload": "{}",
        "serviceAccountId": "sac_unused"
    });
    if let Some(s) = sub {
        j["subscriptionId"] = json!(s);
    }
    j
}

async fn count(f: &Fixture, sql: &str) -> i64 {
    let (n,): (i64,) = sqlx::query_as(sql).fetch_one(&f.app.pool).await.unwrap();
    n
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn events_are_confined_to_the_callers_applications() {
    let f = setup().await;
    let (own, other) = (&f.own, &f.other);

    // Mixed batch: own persisted, other / unknown refused per item, positional.
    let (status, body) = post(
        &f,
        "/api/events/batch",
        &f.scoped,
        json!({"items": [
            event(&format!("{own}:a:b:c")),
            event(&format!("{other}:a:b:c")),
            event("nosuchapp:a:b:c"),
            event(&format!("{own}:a:b:d")),
        ]}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let r = body["results"].as_array().unwrap();
    assert_eq!(r.len(), 4);
    assert_eq!(r[0]["status"], "SUCCESS");
    assert_eq!(r[1]["status"], "BAD_REQUEST");
    assert_eq!(r[1]["error"], OWN_ERR);
    assert_eq!(r[2]["status"], "BAD_REQUEST");
    assert_eq!(r[2]["error"], OWN_ERR);
    assert_eq!(r[3]["status"], "SUCCESS");
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) FROM msg_events WHERE type LIKE 'otherapp:%'"
        )
        .await,
        0
    );
    assert_eq!(count(&f, "SELECT COUNT(*) FROM msg_events").await, 2);

    // Singular create: own ok, other / unknown 403.
    let single = |t: &str| json!({"eventType": t, "source": "t", "data": {"n": 1}});
    let (status, body) = post(
        &f,
        "/api/events",
        &f.scoped,
        single(&format!("{own}:a:b:e")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    for t in [format!("{other}:a:b:e"), "nosuchapp:a:b:e".to_string()] {
        let (status, body) = post(&f, "/api/events", &f.scoped, single(&t)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{t}: {body}");
        assert_eq!(body["error"], "FORBIDDEN", "{body}");
    }

    // Non-scoped caller unaffected, including an unknown application code.
    let (status, body) = post(
        &f,
        "/api/events/batch",
        &f.admin,
        json!({"items": [event(&format!("{other}:a:b:c")), event("nosuchapp:a:b:c")]}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["results"][0]["status"], "SUCCESS");
    assert_eq!(body["results"][1]["status"], "SUCCESS");
    let (status, body) = post(
        &f,
        "/api/events",
        &f.admin,
        single(&format!("{other}:a:b:z")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn dispatch_jobs_are_confined_to_the_callers_applications() {
    let f = setup().await;
    let (own, other) = (&f.own, &f.other);
    let batch = "/api/dispatch-jobs/batch";

    // Own app, with and without an own-app subscription: accepted.
    let (status, body) = post(
        &f,
        batch,
        &f.scoped,
        json!({"items": [
            job(&format!("{own}:a:b:c"), None),
            job(&format!("{own}:a:b:d"), Some(&f.own_sub)),
        ]}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // Mixed: the other-app item refuses the whole batch, naming its index.
    let (status, body) = post(
        &f,
        batch,
        &f.scoped,
        json!({"items": [job(&format!("{own}:a:b:x"), None), job(&format!("{other}:a:b:x"), None)]}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(
        body["message"].as_str().unwrap().contains("items[1]"),
        "{body}"
    );

    // Own code through another application's subscription, an unknown
    // subscription, and an unknown application: refused.
    for (item, idx) in [
        (job(&format!("{own}:a:b:y"), Some(&f.other_sub)), "items[0]"),
        (
            job(&format!("{own}:a:b:y"), Some("sub_missing")),
            "items[0]",
        ),
        (job("nosuchapp:a:b:y", None), "items[0]"),
    ] {
        let (status, body) = post(&f, batch, &f.scoped, json!({"items": [item]})).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert!(body["message"].as_str().unwrap().contains(idx), "{body}");
    }
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) FROM msg_dispatch_jobs WHERE code NOT LIKE 'ownapp:%'"
        )
        .await,
        0
    );

    // Singular: own ok, other 403 without the items prefix, foreign sub 403.
    let (status, body) = post(
        &f,
        "/api/dispatch-jobs",
        &f.scoped,
        job(&format!("{own}:a:b:s"), None),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (status, body) = post(
        &f,
        "/api/dispatch-jobs",
        &f.scoped,
        job(&format!("{other}:a:b:s"), None),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(
        !body["message"].as_str().unwrap().contains("items["),
        "{body}"
    );
    let (status, _) = post(
        &f,
        "/api/dispatch-jobs",
        &f.scoped,
        job(&format!("{own}:a:b:s"), Some(&f.other_sub)),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Non-scoped caller unaffected: other app, unknown app, foreign sub.
    let (status, body) = post(
        &f,
        batch,
        &f.admin,
        json!({"items": [
            job(&format!("{other}:a:b:c"), Some(&f.own_sub)),
            job("nosuchapp:a:b:c", None),
        ]}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (status, body) = post(
        &f,
        "/api/dispatch-jobs",
        &f.admin,
        job(&format!("{other}:a:b:s2"), None),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn audit_logs_are_confined_to_the_callers_applications() {
    let f = setup().await;
    let path = "/api/audit-logs/batch";
    let item = |app: Option<&str>, eid: &str| {
        let mut i = json!({"entityType": "widget", "entityId": eid, "operation": "CREATE", "principalId": "prn_x"});
        if let Some(a) = app {
            i["applicationCode"] = json!(a);
        }
        i
    };

    let (status, body) = post(
        &f,
        path,
        &f.scoped,
        json!({"items": [
            item(Some(&f.own), "a1"),
            item(Some(&f.other), "a2"),
            item(None, "a3"),
            item(Some("nosuchapp"), "a4"),
        ]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let r = body["results"].as_array().unwrap();
    assert_eq!(r.len(), 4);
    assert_eq!(r[0]["status"], "SUCCESS");
    for (i, msg) in [
        (
            1,
            "audit entry belongs to an application this credential cannot write for",
        ),
        (
            2,
            "applicationCode is required for an application-scoped credential",
        ),
        (
            3,
            "audit entry belongs to an application this credential cannot write for",
        ),
    ] {
        assert_eq!(r[i]["status"], "BAD_REQUEST", "{i}");
        assert_eq!(r[i]["error"], msg, "{i}");
    }
    assert_eq!(
        count(
            &f,
            "SELECT COUNT(*) FROM aud_logs WHERE entity_id IN ('a2','a3','a4')"
        )
        .await,
        0
    );

    // Non-scoped caller unchanged: other ok, no code ok, unknown SKIPPED.
    let (status, body) = post(
        &f,
        path,
        &f.admin,
        json!({"items": [item(Some(&f.other), "b1"), item(None, "b2"), item(Some("nosuchapp"), "b3")]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["results"][0]["status"], "SUCCESS");
    assert_eq!(body["results"][1]["status"], "SUCCESS");
    assert_eq!(body["results"][2]["status"], "SKIPPED");
}
