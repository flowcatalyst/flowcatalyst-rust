//! A dispatch job's `serviceAccountId` given as a principal id (Go F49): the
//! id an application carries as its own `serviceAccountId` is its
//! principal's (`prn_`), but a delivery is signed by the service account
//! (`sac_`), so job creation resolves the first to the second. A principal
//! that is not a service account, or does not exist, is 400
//! `SERVICE_ACCOUNT_NOT_FOUND`; a `sac_` id passes through unchanged. The
//! list row carries `serviceAccountId`. Requires Docker (or `FC_TEST_PG_BIN`).

use crate::support;

use axum::http::StatusCode;
use serde_json::{json, Value};

use fc_platform::domain::{Principal, UserScope};
use fc_platform::permissions;
use fc_platform::shared::tsid;
use support::{read_json, TestApp};

const BATCH: &str = "/api/dispatch-jobs/batch";
const SINGLE: &str = "/api/dispatch-jobs";
const SAC: &str = "sac_0HZXEQ5Y8JY5Z";

struct Fixture {
    app: TestApp,
    token: String,
    /// A SERVICE principal fronting `SAC`.
    service_principal: String,
    /// A USER principal: not a service account.
    user_principal: String,
}

impl Fixture {
    async fn new() -> Self {
        let app = TestApp::setup().await;
        let caller = Principal::new_user("anchor@flowcatalyst.test", UserScope::Anchor);
        app.persist_all_applications(&caller).await;
        let granted = vec![
            permissions::admin::BATCH_DISPATCH_JOBS_WRITE.to_string(),
            permissions::admin::DISPATCH_JOB_READ.to_string(),
        ];
        let token = app
            .auth_service
            .generate_access_token_with_scope(&caller, &granted, None)
            .expect("token");

        let service = Principal::new_service(SAC, "svc", UserScope::Anchor);
        let user = Principal::new_user("plain@flowcatalyst.test", UserScope::Anchor);
        for p in [&service, &user] {
            app.repos.principal_repo.insert(p).await.expect("principal");
        }
        Self {
            token,
            service_principal: service.id.as_str().to_string(),
            user_principal: user.id.as_str().to_string(),
            app,
        }
    }

    async fn post(&self, path: &str, body: Value) -> (StatusCode, Value) {
        read_json(self.app.post(path, &self.token, body).await).await
    }

    /// The `service_account_id` stored on job `id`, `None` if no such job.
    async fn stored(&self, id: &str) -> Option<Option<String>> {
        sqlx::query_as::<_, (Option<String>,)>(
            "SELECT service_account_id FROM msg_dispatch_jobs WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.app.pool)
        .await
        .expect("read job")
        .map(|(a,)| a)
    }
}

fn item(id: &str, account: &str) -> Value {
    json!({
        "id": id,
        "code": "cc:a:b:c",
        "targetUrl": "https://receiver.example.test/hook",
        "payload": "{}",
        "serviceAccountId": account,
    })
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_batch_resolves_a_principal_id_to_its_service_account() {
    let f = Fixture::new().await;
    let (via_principal, direct) = (tsid::generate_untyped(), tsid::generate_untyped());
    let (status, body) = f
        .post(
            BATCH,
            json!({"items": [item(&via_principal, &f.service_principal), item(&direct, SAC)]}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(f.stored(&via_principal).await, Some(Some(SAC.to_string())));
    assert_eq!(f.stored(&direct).await, Some(Some(SAC.to_string())));
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_batch_refuses_a_principal_that_is_not_a_service_account() {
    let f = Fixture::new().await;
    for bad in [f.user_principal.clone(), "prn_0HZXEQ5Y8NOSUCH".to_string()] {
        let (good, refused) = (tsid::generate_untyped(), tsid::generate_untyped());
        let (status, body) = f
            .post(
                BATCH,
                json!({"items": [item(&good, SAC), item(&refused, &bad)]}),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["error"], "SERVICE_ACCOUNT_NOT_FOUND", "{body}");
        assert_eq!(f.stored(&good).await, None, "the whole batch is refused");
        assert_eq!(f.stored(&refused).await, None);
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn the_single_create_resolves_and_refuses_the_same_way() {
    let f = Fixture::new().await;
    let single = |account: &str| {
        let mut body = item("unused", account);
        body.as_object_mut().unwrap().remove("id");
        body
    };

    let (status, body) = f.post(SINGLE, single(&f.service_principal)).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["id"].as_str().expect("job id").to_string();
    assert_eq!(f.stored(&id).await, Some(Some(SAC.to_string())));

    let (status, body) = f.post(SINGLE, single(&f.user_principal)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "SERVICE_ACCOUNT_NOT_FOUND", "{body}");

    // The list row carries it, so signability can be checked from the list.
    // The list reads the projection the stream fills; seed the row it would
    // hold for this job.
    sqlx::query(
        "INSERT INTO msg_dispatch_jobs_read
             (id, code, target_url, kind, protocol, mode, status, max_retries, attempt_count,
              sequence, timeout_seconds, retry_strategy, service_account_id, created_at, updated_at)
         SELECT id, code, target_url, 'EVENT', 'HTTP_WEBHOOK', 'IMMEDIATE', status, 3, 0,
                99, 30, 'exponential', service_account_id, created_at, updated_at
         FROM msg_dispatch_jobs WHERE id = $1",
    )
    .bind(&id)
    .execute(&f.app.pool)
    .await
    .expect("seed read row");
    let (status, list) = read_json(f.app.get(SINGLE, &f.token).await).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let row = list
        .as_array()
        .and_then(|rows| rows.iter().find(|r| r["id"] == id.as_str()))
        .unwrap_or_else(|| panic!("job {id} not in the list: {list}"));
    assert_eq!(row["serviceAccountId"], SAC, "{row}");
}
