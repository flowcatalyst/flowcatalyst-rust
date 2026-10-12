//! Naming the client of an ingested dispatch job or event by its
//! `clientCode` (Go F14: `fe25501`, `abd2ae9`; Java `IngestClients`; owner
//! ruling 2026-10-12). Requires Docker (or `FC_TEST_PG_BIN`).
//!
//! Dispatch jobs (`POST /api/dispatch-jobs/batch`, `POST /api/dispatch-jobs`,
//! and `POST /bff/dispatch-jobs/batch`, which takes the same item): an
//! explicit `clientId` wins; a code naming a client the caller reaches
//! resolves to it; a code naming another client or none is 403 "No access to
//! client: <code>" for a non-anchor and 400 `CLIENT_NOT_FOUND` for an
//! anchor. Any refusal rejects the whole batch.

use crate::support;

use axum::http::StatusCode;
use serde_json::{json, Value};

use fc_platform::client::entity::Client;
use fc_platform::domain::{Principal, UserScope};
use fc_platform::permissions;
use fc_platform::shared::tsid;
use support::{read_json, TestApp};

const EVENTS_WRITE: &str = permissions::admin::BATCH_EVENTS_WRITE;
const JOBS_WRITE: &str = permissions::admin::BATCH_DISPATCH_JOBS_WRITE;

/// Two clients (a third for the partner) whose identifiers differ from
/// their ids, so a message naming the id is told apart from one naming the
/// code; and four callers: an anchor, a caller confined to `a`, a partner
/// of `a` and `c`, each stored with the all-applications flag.
struct Fixture {
    app: TestApp,
    a: Client,
    b: Client,
    c: Client,
    anchor: String,
    single: String,
    partner: String,
}

impl Fixture {
    async fn new() -> Self {
        let app = TestApp::setup().await;
        let (a, b, c) = (
            Client::new("Acme", "acme"),
            Client::new("Beta", "beta"),
            Client::new("Coda", "coda"),
        );
        for client in [&a, &b, &c] {
            app.repos.client_repo.insert(client).await.expect("client");
        }
        let perms = [EVENTS_WRITE, JOBS_WRITE];
        let anchor = token(
            &app,
            Principal::new_user("anchor@flowcatalyst.test", UserScope::Anchor),
            &perms,
        )
        .await;
        let single = token(
            &app,
            Principal::new_user("single@flowcatalyst.test", UserScope::Client)
                .with_client_id(a.id.clone()),
            &perms,
        )
        .await;
        let mut partner = Principal::new_user("partner@flowcatalyst.test", UserScope::Partner);
        partner.assigned_clients = vec![a.id.clone(), c.id.clone()];
        let partner = token(&app, partner, &perms).await;
        Self {
            app,
            a,
            b,
            c,
            anchor,
            single,
            partner,
        }
    }

    async fn post(&self, token: &str, path: &str, body: Value) -> (StatusCode, Value) {
        read_json(self.app.post(path, token, body).await).await
    }

    /// Whether job `id` was written, and its client.
    async fn job_client(&self, id: &str) -> Option<Option<String>> {
        sqlx::query_as::<_, (Option<String>,)>(
            "SELECT client_id FROM msg_dispatch_jobs WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.app.pool)
        .await
        .expect("read job")
        .map(|(c,)| c)
    }
}

async fn token(app: &TestApp, principal: Principal, perms: &[&str]) -> String {
    app.persist_all_applications(&principal).await;
    let granted: Vec<String> = perms.iter().map(|p| p.to_string()).collect();
    app.auth_service
        .generate_access_token_with_scope(&principal, &granted, None)
        .expect("token")
}

fn id_of(client: &Client) -> String {
    client.id.as_str().to_string()
}

/// A batch dispatch-job item with a fixed id and `members` merged in.
fn job(id: &str, members: Value) -> Value {
    let mut item = json!({
        "id": id,
        "code": "cc:a:b:c",
        "targetUrl": "https://receiver.example.test/hook",
        "payload": "{}",
    });
    for (k, v) in members.as_object().unwrap() {
        item[k] = v.clone();
    }
    item
}

/// The singular create's body with `members` merged in.
fn single_job(members: Value) -> Value {
    let mut item = job("unused", members);
    item.as_object_mut().unwrap().remove("id");
    item["serviceAccountId"] = json!("sac_unused");
    item
}

fn new_id() -> String {
    tsid::generate_untyped()
}

fn assert_refused(status: StatusCode, body: &Value, want: (StatusCode, &str, String)) {
    assert_eq!(status, want.0, "{body}");
    assert_eq!(body["error"], want.1, "{body}");
    assert_eq!(body["message"], json!(want.2), "{body}");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_dispatch_job_names_its_client_by_code() {
    let f = Fixture::new().await;
    const BATCH: &str = "/api/dispatch-jobs/batch";

    // Anchor: a known code resolves, for every item naming it.
    let (one, two) = (new_id(), new_id());
    let (status, body) = f
        .post(
            &f.anchor,
            BATCH,
            json!({"items": [job(&one, json!({"clientCode": "acme"})),
                             job(&two, json!({"clientCode": "acme"}))]}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(f.job_client(&one).await, Some(Some(id_of(&f.a))));
    assert_eq!(f.job_client(&two).await, Some(Some(id_of(&f.a))));

    // An explicit clientId wins over the code.
    let id = new_id();
    let (status, body) = f
        .post(
            &f.anchor,
            BATCH,
            json!({"items": [job(&id, json!({"clientId": id_of(&f.b), "clientCode": "acme"}))]}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(f.job_client(&id).await, Some(Some(id_of(&f.b))));

    // A blank code, and a blank clientId beside a code, are no value.
    let (blank, blank_id) = (new_id(), new_id());
    let (status, body) = f
        .post(
            &f.anchor,
            BATCH,
            json!({"items": [job(&blank, json!({"clientCode": "  "})),
                             job(&blank_id, json!({"clientId": " ", "clientCode": "acme"}))]}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(f.job_client(&blank).await, Some(None));
    assert_eq!(f.job_client(&blank_id).await, Some(Some(id_of(&f.a))));

    // Anchor, unknown code: 400, the whole batch refused, nothing written.
    let (good, bad) = (new_id(), new_id());
    let (status, body) = f
        .post(
            &f.anchor,
            BATCH,
            json!({"items": [job(&good, json!({"clientCode": "acme"})),
                             job(&bad, json!({"clientCode": "nosuch-client"}))]}),
        )
        .await;
    assert_refused(
        status,
        &body,
        (
            StatusCode::BAD_REQUEST,
            "CLIENT_NOT_FOUND",
            "Client not found: nosuch-client".into(),
        ),
    );
    assert_eq!(
        f.job_client(&good).await,
        None,
        "a refused batch writes nothing"
    );

    // Client-scoped caller: its own client's code resolves; its own id wins
    // over another client's code.
    let (own, own_id) = (new_id(), new_id());
    let (status, body) = f
        .post(
            &f.single,
            BATCH,
            json!({"items": [job(&own, json!({"clientCode": "acme"})),
                             job(&own_id, json!({"clientId": id_of(&f.a), "clientCode": "beta"}))]}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(f.job_client(&own).await, Some(Some(id_of(&f.a))));
    assert_eq!(f.job_client(&own_id).await, Some(Some(id_of(&f.a))));

    // Another client's code and an unknown code: the same 403 naming the
    // code, never the other client's id; a valid item before it not written.
    for code in ["beta", "nosuch-client"] {
        let (good, bad) = (new_id(), new_id());
        let (status, body) = f
            .post(
                &f.single,
                BATCH,
                json!({"items": [job(&good, json!({})), job(&bad, json!({"clientCode": code}))]}),
            )
            .await;
        assert_refused(
            status,
            &body,
            (
                StatusCode::FORBIDDEN,
                "FORBIDDEN",
                format!("No access to client: {code}"),
            ),
        );
        assert!(!body.to_string().contains(f.b.id.as_str()), "{body}");
        assert_eq!(f.job_client(&good).await, None, "{code}");
        assert_eq!(f.job_client(&bad).await, None, "{code}");
    }

    // A partner names one of its clients by code (it must name one).
    let id = new_id();
    let (status, body) = f
        .post(
            &f.partner,
            BATCH,
            json!({"items": [job(&id, json!({"clientCode": "coda"}))]}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(f.job_client(&id).await, Some(Some(id_of(&f.c))));

    // The singular create: the same rules.
    let created_job = |body: &Value| body["id"].as_str().unwrap().to_string();
    let (status, body) = f
        .post(
            &f.anchor,
            "/api/dispatch-jobs",
            single_job(json!({"clientCode": "acme"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(
        f.job_client(&created_job(&body)).await,
        Some(Some(id_of(&f.a)))
    );
    let (status, body) = f
        .post(
            &f.anchor,
            "/api/dispatch-jobs",
            single_job(json!({"clientCode": "nosuch-client"})),
        )
        .await;
    assert_refused(
        status,
        &body,
        (
            StatusCode::BAD_REQUEST,
            "CLIENT_NOT_FOUND",
            "Client not found: nosuch-client".into(),
        ),
    );
    for code in ["beta", "nosuch-client"] {
        let (status, body) = f
            .post(
                &f.single,
                "/api/dispatch-jobs",
                single_job(json!({"clientCode": code})),
            )
            .await;
        assert_refused(
            status,
            &body,
            (
                StatusCode::FORBIDDEN,
                "FORBIDDEN",
                format!("No access to client: {code}"),
            ),
        );
    }
    let (status, body) = f
        .post(
            &f.single,
            "/api/dispatch-jobs",
            single_job(json!({"clientId": id_of(&f.a), "clientCode": "beta"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(
        f.job_client(&created_job(&body)).await,
        Some(Some(id_of(&f.a)))
    );
    let (status, body) = f
        .post(
            &f.partner,
            "/api/dispatch-jobs",
            single_job(json!({"clientCode": "coda"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(
        f.job_client(&created_job(&body)).await,
        Some(Some(id_of(&f.c)))
    );

    // The BFF batch takes the same item and the same rule.
    let (status, body) = f
        .post(
            &f.partner,
            "/bff/dispatch-jobs/batch",
            json!({"jobs": [single_job(json!({"clientCode": "coda"}))]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["jobs"][0]["clientId"], json!(id_of(&f.c)), "{body}");
    let (status, body) = f
        .post(
            &f.single,
            "/bff/dispatch-jobs/batch",
            json!({"jobs": [single_job(json!({"clientCode": "beta"}))]}),
        )
        .await;
    assert_refused(
        status,
        &body,
        (
            StatusCode::FORBIDDEN,
            "FORBIDDEN",
            "No access to client: beta".into(),
        ),
    );

    // Nothing of any refused request was written: every stored job is one
    // of the accepted ones above (two, explicit id, two blanks, two own, the
    // partner's, and four singular/BFF creates).
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM msg_dispatch_jobs")
        .fetch_one(&f.app.pool)
        .await
        .unwrap();
    assert_eq!(n, 12);
}
