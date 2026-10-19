//! Go F20: the event and dispatch-job reads an application's own admin pages
//! call as its service account (Go `event/api/app_read_pg_test.go`,
//! `dispatchjob/api/app_read_pg_test.go`, through the HTTP stack).
//!
//! `GET /api/events`, `/api/events/{id}`, `GET /api/dispatch-jobs`, `/{id}`,
//! `/{id}/attempts`, `/event/{eventId}` (and Rust's `/by-event/{eventId}`)
//! additionally admit `platform:application-service:event:view` /
//! `:dispatch-job:view`. A caller admitted only by the application-service
//! permission sees only rows whose application (the first segment of the
//! event type or job code) it is bound to; its own filters can only narrow
//! within that. A credential bound to no application is 403 `FORBIDDEN`. A
//! caller holding the console permission is unchanged. Requires Docker (or
//! `FC_TEST_PG_BIN`).

use std::collections::BTreeSet;

use axum::http::StatusCode;
use serde_json::Value;

use crate::support::{self, read_json, TestApp};
use fc_platform::application::entity::Application;
use fc_platform::client::entity::Client;
use fc_platform::domain::{Principal, UserScope};
use fc_platform::role::entity::AuthRole;
use fc_platform::service_account::entity::RoleAssignment;

const EVENT_VIEW: &str = "platform:application-service:event:view";
const JOB_VIEW: &str = "platform:application-service:dispatch-job:view";

struct Fixture {
    app: TestApp,
    own: Application,
    other: Application,
    client_a: Client,
    client_b: Client,
    /// own-application ids at client A, client B, and platform-scoped
    own_a: String,
    own_b: String,
    own_platform: String,
    foreign: String,
}

async fn put_event(
    app: &TestApp,
    id: &str,
    application: &str,
    client: Option<&Client>,
    at_secs: i64,
) {
    sqlx::query(
        "INSERT INTO msg_events_read (id, type, source, time, client_id, application, created_at) \
         VALUES ($1, $2, 'test://appread', NOW(), $3, $4, NOW() - interval '1 hour' + ($5 || ' seconds')::interval)",
    )
    .bind(id)
    .bind(format!("{application}:sub:agg:happened"))
    .bind(client.map(|c| c.id.as_str().to_string()))
    .bind(application)
    .bind(at_secs.to_string())
    .execute(&app.pool)
    .await
    .unwrap();
}

async fn put_job(
    app: &TestApp,
    id: &str,
    application: &str,
    client: Option<&Client>,
    event: Option<&str>,
    at_secs: i64,
) {
    let client = client.map(|c| c.id.as_str().to_string());
    let code = format!("{application}:sub:agg:happened");
    sqlx::query(
        "INSERT INTO msg_dispatch_jobs (id, code, target_url, status, client_id, event_id, attempt_count, created_at, updated_at) \
         VALUES ($1, $2, 'http://example.invalid/hook', 'COMPLETED', $3, $4, 1, \
                 NOW() - interval '1 hour' + ($5 || ' seconds')::interval, \
                 NOW() - interval '1 hour' + ($5 || ' seconds')::interval)",
    )
    .bind(id)
    .bind(&code)
    .bind(&client)
    .bind(event)
    .bind(at_secs.to_string())
    .execute(&app.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO msg_dispatch_jobs_read \
         (id, code, target_url, client_id, event_id, application, kind, protocol, mode, status, max_retries, attempt_count, \
          sequence, timeout_seconds, retry_strategy, created_at, updated_at) \
         VALUES ($1, $2, 'http://example.invalid/hook', $3, $4, $5, 'EVENT', 'HTTP_WEBHOOK', 'IMMEDIATE', 'COMPLETED', 3, 1, \
                 99, 30, 'exponential', \
                 NOW() - interval '1 hour' + ($6 || ' seconds')::interval, \
                 NOW() - interval '1 hour' + ($6 || ' seconds')::interval)",
    )
    .bind(id)
    .bind(&code)
    .bind(&client)
    .bind(event)
    .bind(application)
    .bind(at_secs.to_string())
    .execute(&app.pool)
    .await
    .unwrap();
}

async fn setup() -> Fixture {
    support::set_app_key();
    let app = TestApp::setup().await;
    let own = Application::new("rdown", "Read Own");
    let other = Application::new("rdoth", "Read Other");
    for a in [&own, &other] {
        app.repos.application_repo.insert(a).await.unwrap();
    }
    let mut clients = Vec::new();
    for tag in ["a", "b"] {
        let c = Client::new(format!("Read {tag}"), format!("rd-{tag}"));
        app.repos.client_repo.insert(&c).await.unwrap();
        clients.push(c);
    }
    let (client_b, client_a) = (clients.pop().unwrap(), clients.pop().unwrap());
    let event = "evt_appread".to_string();
    let f = Fixture {
        own_a: "rd-e-own-a".into(),
        own_b: "rd-e-own-b".into(),
        own_platform: "rd-e-own-p".into(),
        foreign: "rd-e-foreign".into(),
        own,
        other,
        client_a,
        client_b,
        app,
    };
    // events
    put_event(&f.app, &f.own_a, "rdown", Some(&f.client_a), 1).await;
    put_event(&f.app, &f.own_b, "rdown", Some(&f.client_b), 2).await;
    put_event(&f.app, &f.own_platform, "rdown", None, 3).await;
    put_event(&f.app, &f.foreign, "rdoth", Some(&f.client_a), 4).await;
    // dispatch jobs: same layout; the first own job and the foreign one share an event
    put_job(
        &f.app,
        "rd-j-own-a",
        "rdown",
        Some(&f.client_a),
        Some(&event),
        1,
    )
    .await;
    put_job(&f.app, "rd-j-own-b", "rdown", Some(&f.client_b), None, 2).await;
    put_job(&f.app, "rd-j-own-p", "rdown", None, None, 3).await;
    put_job(
        &f.app,
        "rd-j-foreign",
        "rdoth",
        Some(&f.client_a),
        Some(&event),
        4,
    )
    .await;
    f
}

enum Reach<'a> {
    Bound(&'a [&'a Application]),
    All,
}

impl Fixture {
    async fn token(
        &self,
        name: &str,
        reach: Reach<'_>,
        perms: &[&str],
        client: Option<&Client>,
    ) -> String {
        let mut role = AuthRole::new("rdcaller", name, name);
        for p in perms {
            role = role.with_permission(*p);
        }
        self.app.repos.role_repo.insert(&role).await.unwrap();
        let scope = if client.is_some() {
            UserScope::Client
        } else {
            UserScope::Anchor
        };
        let mut principal = Principal::new_service(name, name, scope);
        if let Some(c) = client {
            principal = principal.with_client_id(c.id.clone());
        }
        principal.roles = vec![RoleAssignment::new(role.name.clone())];
        self.app
            .repos
            .principal_repo
            .insert(&principal)
            .await
            .unwrap();
        match reach {
            Reach::All => principal.all_applications = true,
            Reach::Bound(apps) => {
                principal.accessible_application_ids = apps.iter().map(|a| a.id.clone()).collect()
            }
        }
        self.app
            .repos
            .principal_repo
            .update(&principal)
            .await
            .unwrap();
        self.app
            .auth_service
            .generate_access_token(&principal)
            .unwrap()
    }

    async fn svc(&self, name: &str, perms: &[&str]) -> String {
        self.token(name, Reach::Bound(&[&self.own]), perms, None)
            .await
    }

    async fn get(&self, token: &str, path: &str) -> (StatusCode, Value) {
        read_json(self.app.get(path, token).await).await
    }

    /// The fixture rows (not other rows the platform wrote) a list holds,
    /// in order.
    async fn ids(&self, token: &str, path: &str) -> Vec<String> {
        let (status, body) = self.get(token, path).await;
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        body.as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_str().unwrap().to_string())
            .filter(|id| id.starts_with("rd-"))
            .collect()
    }
}

fn set(ids: &[&str]) -> BTreeSet<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

fn as_set(ids: Vec<String>) -> BTreeSet<String> {
    ids.into_iter().collect()
}

fn code(body: &Value) -> &str {
    body["error"].as_str().unwrap_or_default()
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn an_application_service_account_reads_only_its_own_applications_events() {
    let f = setup().await;
    let svc = f.svc("svc-events", &[EVENT_VIEW]).await;
    let mine = set(&["rd-e-own-a", "rd-e-own-b", "rd-e-own-p"]);

    // own application only
    assert_eq!(as_set(f.ids(&svc, "/api/events?size=1000").await), mine);

    // a caller filter cannot widen it
    assert!(f
        .ids(&svc, "/api/events?size=1000&applications=rdoth")
        .await
        .is_empty());
    assert_eq!(
        as_set(
            f.ids(&svc, "/api/events?size=1000&applications=rdown,rdoth")
                .await
        ),
        mine
    );
    assert!(f
        .ids(&svc, "/api/events?size=1000&types=rdoth:sub:agg:happened")
        .await
        .is_empty());
    assert!(f
        .ids(&svc, "/api/events?size=1000&clientIds=none-such")
        .await
        .is_empty());

    // newest first, paged
    assert_eq!(
        f.ids(&svc, "/api/events?size=2&offset=0").await,
        ["rd-e-own-p", "rd-e-own-b"]
    );
    assert_eq!(
        f.ids(&svc, "/api/events?size=2&offset=2").await,
        ["rd-e-own-a"]
    );
    assert!(f.ids(&svc, "/api/events?size=2&offset=3").await.is_empty());

    // a caller bound to the other application sees only that one
    let oth = f
        .token("svc-ev-oth", Reach::Bound(&[&f.other]), &[EVENT_VIEW], None)
        .await;
    assert_eq!(
        as_set(f.ids(&oth, "/api/events?size=1000").await),
        set(&["rd-e-foreign"])
    );

    // an all-applications caller sees every application
    let all = f.token("svc-ev-all", Reach::All, &[EVENT_VIEW], None).await;
    let seen = as_set(f.ids(&all, "/api/events?size=1000").await);
    assert_eq!(
        seen,
        set(&["rd-e-own-a", "rd-e-own-b", "rd-e-own-p", "rd-e-foreign"])
    );

    // the console's messaging view stays unconfined (bound to own, but the console permission)
    let console = f
        .svc("svc-ev-console", &["platform:messaging:event:view"])
        .await;
    assert_eq!(as_set(f.ids(&console, "/api/events?size=1000").await), seen);

    // a client-scoped caller keeps its client reach on top: own application AND
    // (client A or platform-scoped); not client B, not the other application
    let cs = f
        .token(
            "svc-ev-client",
            Reach::Bound(&[&f.own]),
            &[EVENT_VIEW],
            Some(&f.client_a),
        )
        .await;
    assert_eq!(
        as_set(f.ids(&cs, "/api/events?size=1000").await),
        set(&["rd-e-own-a", "rd-e-own-p"])
    );
    let (status, _) = f.get(&cs, "/api/events/rd-e-own-b").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn an_event_by_id_is_the_applications_or_not_found() {
    let f = setup().await;
    let svc = f.svc("svc-ev-id", &[EVENT_VIEW]).await;
    let (status, body) = f.get(&svc, "/api/events/rd-e-own-a").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["id"], "rd-e-own-a");

    let (s_other, b_other) = f.get(&svc, "/api/events/rd-e-foreign").await;
    let (s_missing, b_missing) = f.get(&svc, "/api/events/rd-e-missing").await;
    assert_eq!(s_other, StatusCode::NOT_FOUND, "{b_other}");
    assert_eq!(s_missing, StatusCode::NOT_FOUND, "{b_missing}");
    // No existence leak: only the id echoed in the message differs.
    assert_eq!(code(&b_other), code(&b_missing));
    assert_eq!(
        b_other["message"]
            .as_str()
            .unwrap()
            .replace("rd-e-foreign", "X"),
        b_missing["message"]
            .as_str()
            .unwrap()
            .replace("rd-e-missing", "X")
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn an_unbound_credential_is_refused_not_answered_an_empty_page() {
    let f = setup().await;
    let unbound = f
        .token(
            "svc-unbound",
            Reach::Bound(&[]),
            &[EVENT_VIEW, JOB_VIEW],
            None,
        )
        .await;
    for path in [
        "/api/events",
        "/api/events/rd-e-own-a",
        "/api/dispatch-jobs",
        "/api/dispatch-jobs/rd-j-own-a",
        "/api/dispatch-jobs/rd-j-own-a/attempts",
        "/api/dispatch-jobs/event/evt_appread",
        "/api/dispatch-jobs/by-event/evt_appread",
    ] {
        let (status, body) = f.get(&unbound, path).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {body}");
        assert_eq!(code(&body), "FORBIDDEN", "{path}: {body}");
        assert!(
            body["message"]
                .as_str()
                .unwrap()
                .contains("not bound to an application"),
            "{path}: {body}"
        );
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn each_read_asks_its_own_permission() {
    let f = setup().await;
    // event:view does not read dispatch jobs, and the other way round
    let events_only = f.svc("svc-p-ev", &[EVENT_VIEW]).await;
    let jobs_only = f.svc("svc-p-job", &[JOB_VIEW]).await;
    for path in [
        "/api/dispatch-jobs",
        "/api/dispatch-jobs/rd-j-own-a",
        "/api/dispatch-jobs/rd-j-own-a/attempts",
        "/api/dispatch-jobs/event/evt_appread",
    ] {
        let (status, body) = f.get(&events_only, path).await;
        assert_eq!(
            (status, code(&body)),
            (StatusCode::FORBIDDEN, "PERMISSION_REQUIRED"),
            "{path}: {body}"
        );
    }
    for path in ["/api/events", "/api/events/rd-e-own-a"] {
        let (status, body) = f.get(&jobs_only, path).await;
        assert_eq!(
            (status, code(&body)),
            (StatusCode::FORBIDDEN, "PERMISSION_REQUIRED"),
            "{path}: {body}"
        );
    }
    // the application-service permissions do not open the other read routes
    for path in [
        "/api/events/filter-options",
        "/api/events/list-raw",
        "/api/dispatch-jobs/filter-options",
        "/api/dispatch-jobs/list-raw",
        "/api/dispatch-jobs/rd-j-own-a/raw",
    ] {
        for token in [&events_only, &jobs_only] {
            let (status, body) = f.get(token, path).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {body}");
        }
    }
    let resp = f.app.get_unauth("/api/events").await;
    assert!(
        matches!(
            resp.status(),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ),
        "{}",
        resp.status()
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn an_application_service_account_reads_only_its_own_applications_dispatch_jobs() {
    let f = setup().await;
    let svc = f.svc("svc-jobs", &[JOB_VIEW]).await;
    let mine = set(&["rd-j-own-a", "rd-j-own-b", "rd-j-own-p"]);

    assert_eq!(
        as_set(f.ids(&svc, "/api/dispatch-jobs?size=1000").await),
        mine
    );

    // a caller filter cannot widen it
    for q in [
        "applications=rdoth",
        "code=rdoth:sub:agg:happened",
        "codes=rdoth:sub:agg:happened",
    ] {
        assert!(
            f.ids(&svc, &format!("/api/dispatch-jobs?size=1000&{q}"))
                .await
                .is_empty(),
            "{q}"
        );
    }

    // newest first, paged
    assert_eq!(
        f.ids(&svc, "/api/dispatch-jobs?size=2&offset=0").await,
        ["rd-j-own-p", "rd-j-own-b"]
    );
    assert_eq!(
        f.ids(&svc, "/api/dispatch-jobs?size=2&offset=2").await,
        ["rd-j-own-a"]
    );
    assert!(f
        .ids(&svc, "/api/dispatch-jobs?size=2&offset=3")
        .await
        .is_empty());

    // an all-applications caller sees every application
    let all = f.token("svc-jobs-all", Reach::All, &[JOB_VIEW], None).await;
    let seen = as_set(f.ids(&all, "/api/dispatch-jobs?size=1000").await);
    assert_eq!(
        seen,
        set(&["rd-j-own-a", "rd-j-own-b", "rd-j-own-p", "rd-j-foreign"])
    );

    // the console's messaging view stays unconfined
    let console = f
        .svc(
            "svc-jobs-console",
            &["platform:messaging:dispatch-job:view"],
        )
        .await;
    assert_eq!(
        as_set(f.ids(&console, "/api/dispatch-jobs?size=1000").await),
        seen
    );

    // a client-scoped caller keeps its client reach on top
    let cs = f
        .token(
            "svc-jobs-client",
            Reach::Bound(&[&f.own]),
            &[JOB_VIEW],
            Some(&f.client_a),
        )
        .await;
    assert_eq!(
        as_set(f.ids(&cs, "/api/dispatch-jobs?size=1000").await),
        set(&["rd-j-own-a", "rd-j-own-p"]),
        "own application AND (client A or platform-scoped)"
    );
    let (status, body) = f.get(&cs, "/api/dispatch-jobs/rd-j-own-b").await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::FORBIDDEN, "SCOPE_FORBIDDEN"),
        "{body}"
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_job_by_id_attempts_and_event_is_the_applications_or_not_found() {
    let f = setup().await;
    let svc = f.svc("svc-job-id", &[JOB_VIEW]).await;
    let (status, body) = f.get(&svc, "/api/dispatch-jobs/rd-j-own-a").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["id"], "rd-j-own-a");
    let (status, body) = f.get(&svc, "/api/dispatch-jobs/rd-j-own-a/attempts").await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (s_other, b_other) = f.get(&svc, "/api/dispatch-jobs/rd-j-foreign").await;
    let (s_missing, b_missing) = f.get(&svc, "/api/dispatch-jobs/rd-j-missing").await;
    assert_eq!(s_other, StatusCode::NOT_FOUND, "{b_other}");
    assert_eq!(s_missing, StatusCode::NOT_FOUND, "{b_missing}");
    assert_eq!(code(&b_other), code(&b_missing));
    assert_eq!(
        b_other["message"]
            .as_str()
            .unwrap()
            .replace("rd-j-foreign", "X"),
        b_missing["message"]
            .as_str()
            .unwrap()
            .replace("rd-j-missing", "X")
    );
    let (status, body) = f
        .get(&svc, "/api/dispatch-jobs/rd-j-foreign/attempts")
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // by event returns only the application's jobs
    for path in [
        "/api/dispatch-jobs/event/evt_appread",
        "/api/dispatch-jobs/by-event/evt_appread",
    ] {
        assert_eq!(f.ids(&svc, path).await, ["rd-j-own-a"], "{path}");
    }
    let all = f.token("svc-job-all", Reach::All, &[JOB_VIEW], None).await;
    assert_eq!(
        as_set(f.ids(&all, "/api/dispatch-jobs/event/evt_appread").await),
        set(&["rd-j-own-a", "rd-j-foreign"])
    );
}
