//! Members Go's contract documents that this platform lacked (the "platform
//! gaps" of `docs/sdks.md`), each end to end: stored, written through its use
//! case, answered on the reads Go answers them on. Where Go documents a member
//! but never fills it, the test pins the evident intent instead (the doc
//! comment says so). Requires Docker:
//!   cargo test -p fc-platform --test it go_field_gaps_test:: -- --ignored

use crate::support;

use axum::http::StatusCode;
use serde_json::{json, Value};

use fc_platform::application::entity::Application;
use fc_platform::client::entity::Client;
use fc_platform::domain::Principal;
use fc_platform::domain::UserScope;
use fc_platform::service_account::entity::RoleAssignment;
use fc_platform::shared::database;
use fc_platform::shared::database::MigrationProfile;
use fc_stream::dispatch_job_projection;
use fc_stream::event_projection;
use fc_stream::health::StreamHealth;
use std::sync::Arc;
use std::time::Duration;
use support::{assert_status, read_json, TestApp};
use tokio::time;
use tokio_util::sync::CancellationToken;

async fn setup() -> TestApp {
    support::set_app_key();
    TestApp::setup().await
}

async fn insert_client(app: &TestApp, identifier: &str) -> String {
    let c = Client::new(identifier.to_uppercase(), identifier);
    app.repos.client_repo.insert(&c).await.unwrap();
    c.id.to_string()
}

async fn get_json(app: &TestApp, path: &str, token: &str) -> Value {
    assert_status(app.get(path, token).await, StatusCode::OK).await
}

// ── Migrations ───────────────────────────────────────────────────────────

/// Each field-gap migration is a no-op when re-run, and its probe
/// recognises it on a database that has the columns but no tracker row.
#[tokio::test]
#[ignore = "requires Docker"]
async fn field_gap_migrations_rerun_as_no_ops_and_are_recognised_when_applied() {
    let app = setup().await;
    let migrations: &[(&str, &str)] = &[
        (
            "058_app_client_config_overrides",
            include_str!("../../../../migrations/058_app_client_config_overrides.sql"),
        ),
        (
            "059_principal_role_assigned_by",
            include_str!("../../../../migrations/059_principal_role_assigned_by.sql"),
        ),
        (
            "060_service_account_webhook_credential_members",
            include_str!(
                "../../../../migrations/060_service_account_webhook_credential_members.sql"
            ),
        ),
        (
            "061_dispatch_job_read_queue",
            include_str!("../../../../migrations/061_dispatch_job_read_queue.sql"),
        ),
    ];
    for (id, sql) in migrations {
        sqlx::raw_sql(sql)
            .execute(&app.pool)
            .await
            .unwrap_or_else(|e| panic!("re-running {id}: {e}"));
    }
    sqlx::query("DELETE FROM _schema_migrations")
        .execute(&app.pool)
        .await
        .unwrap();
    database::run_migrations(&app.pool, MigrationProfile::Production)
        .await
        .expect("migrations over an already-migrated schema");
    for (id, _) in migrations {
        let (tracked,): (bool,) = sqlx::query_as(
            "SELECT EXISTS (SELECT 1 FROM _schema_migrations WHERE migration_id = $1)",
        )
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
        assert!(tracked, "the probe recognises an applied {id}");
    }
}

// ── 1. Client config: baseUrlOverride, configJson ────────────────────────

/// Go documents `baseUrlOverride` and `configJson` on `ClientConfigResponse`
/// but stores neither. They are stored here, set through the config PUT, and
/// answered by both of Go's reads; enabling or disabling the application
/// keeps them.
#[tokio::test]
#[ignore = "requires Docker"]
async fn client_config_keeps_and_answers_the_base_url_override_and_document() {
    let app = setup().await;
    let token = app.anchor_admin_token().await;
    let application = Application::new("cfgapp", "Cfg App");
    app.repos
        .application_repo
        .insert(&application)
        .await
        .unwrap();
    let client_id = insert_client(&app, "cfgclient").await;
    let base = format!("/api/applications/{}/clients", application.id);
    let one = format!("{base}/{client_id}");

    let (s, _) = read_json(app.post(&format!("{one}/enable"), &token, json!({})).await).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let fresh = get_json(&app, &one, &token).await;
    assert!(fresh.get("baseUrlOverride").is_none(), "{fresh}");
    assert!(fresh.get("configJson").is_none(), "{fresh}");

    let document = json!({"featureFlags": {"beta": true}, "tier": 2});
    let put = assert_status(
        app.put(
            &one,
            &token,
            json!({"baseUrlOverride": "https://acme.example.com", "configJson": document}),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(put["baseUrlOverride"], "https://acme.example.com");
    assert_eq!(put["effectiveBaseUrl"], "https://acme.example.com");
    assert_eq!(put["configJson"], document);
    assert_eq!(put["config"], document, "the older member is a copy");

    let read = get_json(&app, &one, &token).await;
    assert_eq!(read["baseUrlOverride"], "https://acme.example.com");
    assert_eq!(read["configJson"], document);
    let list = get_json(&app, &base, &token).await;
    assert_eq!(
        list["items"][0]["baseUrlOverride"],
        "https://acme.example.com"
    );
    assert_eq!(list["items"][0]["configJson"], document);

    // Disabling rewrites the row through the same persist: the overrides stay.
    let (s, _) = read_json(app.post(&format!("{one}/disable"), &token, json!({})).await).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let disabled = get_json(&app, &one, &token).await;
    assert_eq!(disabled["enabled"], false);
    assert_eq!(disabled["baseUrlOverride"], "https://acme.example.com");
    assert_eq!(disabled["configJson"], document);

    // `""` clears the override; an absent document is left as it is.
    let cleared = assert_status(
        app.put(&one, &token, json!({"baseUrlOverride": ""})).await,
        StatusCode::OK,
    )
    .await;
    assert!(cleared.get("configJson").is_some());
    let read = get_json(&app, &one, &token).await;
    assert!(read.get("baseUrlOverride").is_none(), "{read}");
    assert_eq!(read["configJson"], document);

    assert_eq!(
        app.event_count_by_type("platform:iam:application:client-config-updated")
            .await,
        2
    );
}

// ── 2. Event types: clientScoped ─────────────────────────────────────────

/// Go honours `clientScoped` on event-type create and update (`/api`) and on
/// the BFF create; absent on update leaves it as it is. The BFF read answers
/// it (the SPA's subscription editor filters on it); Go's `/api` response
/// does not carry it, and neither does this platform's.
#[tokio::test]
#[ignore = "requires Docker"]
async fn event_type_client_scoped_is_stored_on_create_and_update() {
    let app = setup().await;
    let token = app.anchor_admin_token().await;
    let stored = |id: String| {
        let repo = app.repos.event_type_repo.clone();
        async move {
            repo.find_by_id(&id)
                .await
                .unwrap()
                .expect("event type")
                .client_scoped
        }
    };

    let created = assert_status(
        app.post(
            "/api/event-types",
            &token,
            json!({"code": "gaps:orders:order:placed", "name": "Placed", "clientScoped": true}),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    let id = created["id"].as_str().unwrap().to_string();
    assert!(stored(id.clone()).await);
    let read = get_json(&app, &format!("/api/event-types/{id}"), &token).await;
    assert!(
        read.get("clientScoped").is_none(),
        "Go's /api shape: {read}"
    );

    // Absent leaves it; false clears it.
    let path = format!("/api/event-types/{id}");
    let (s, _) = read_json(app.put(&path, &token, json!({"name": "Placed 2"})).await).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    assert!(stored(id.clone()).await);
    let (s, _) = read_json(
        app.put(
            &path,
            &token,
            json!({"name": "Placed 3", "clientScoped": false}),
        )
        .await,
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    assert!(!stored(id.clone()).await);

    // Absent on create is false.
    let plain = assert_status(
        app.post(
            "/api/event-types",
            &token,
            json!({"code": "gaps:orders:order:shipped", "name": "Shipped"}),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    assert!(!stored(plain["id"].as_str().unwrap().to_string()).await);

    // The BFF create honours it and its answer carries it.
    let bff = assert_status(
        app.post(
            "/bff/event-types",
            &token,
            json!({"code": "gaps:orders:order:returned", "name": "Returned", "clientScoped": true}),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(bff["clientScoped"], true, "{bff}");
    let bff_id = bff["id"].as_str().unwrap().to_string();
    let bff_read = get_json(&app, &format!("/bff/event-types/{bff_id}"), &token).await;
    assert_eq!(bff_read["clientScoped"], true);
}

// ── 3. Events: contextData on the detail read ────────────────────────────

/// Run the event projector for a moment.
async fn project_events(pool: &sqlx::PgPool) {
    let cancel = CancellationToken::new();
    let projector = tokio::spawn(event_projection::run(
        pool.clone(),
        200,
        Arc::new(StreamHealth::new("event-projection".into())),
        cancel.clone(),
    ));
    time::sleep(Duration::from_millis(1500)).await;
    cancel.cancel();
    time::timeout(Duration::from_secs(10), projector)
        .await
        .expect("projector stops")
        .unwrap();
}

/// Go documents `contextData` on `GET /api/events/{id}` but reads only the
/// projection, which has no column for it. The detail answers the event's
/// stored context entries; an event without any leaves the member out.
#[tokio::test]
#[ignore = "requires Docker"]
async fn event_detail_answers_the_events_context_data() {
    let app = setup().await;
    let token = app.anchor_admin_token().await;
    let entries = json!([{"key": "orderId", "value": "ord_42"}, {"key": "region", "value": "eu"}]);
    let with = assert_status(
        app.post(
            "/api/events",
            &token,
            json!({
                "eventType": "gaps:orders:order:placed",
                "source": "urn:gaps",
                "subject": "orders.order.ord_42",
                "data": {"total": 12},
                "contextData": entries
            }),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    let without = assert_status(
        app.post(
            "/api/events",
            &token,
            json!({
                "eventType": "gaps:orders:order:placed",
                "source": "urn:gaps",
                "subject": "orders.order.ord_43",
                "data": {"total": 13}
            }),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    project_events(&app.pool).await;

    let id = with["event"]["id"].as_str().expect("event id");
    let detail = get_json(&app, &format!("/api/events/{id}"), &token).await;
    assert_eq!(detail["contextData"], entries, "{detail}");
    assert_eq!(detail["type"], "gaps:orders:order:placed");

    let id = without["event"]["id"].as_str().expect("event id");
    let detail = get_json(&app, &format!("/api/events/{id}"), &token).await;
    assert!(detail.get("contextData").is_none(), "{detail}");
}

// ── 4. OAuth client create: principalId ──────────────────────────────────

async fn client_credentials(app: &TestApp, client_id: &str, secret: &str) -> (StatusCode, Value) {
    use axum::body::Body;
    use axum::http::{Method, Request};
    use tower::ServiceExt;
    let body = format!(
        "grant_type=client_credentials&client_id={client_id}&client_secret={}",
        urlencoding::encode(secret)
    );
    let resp = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/oauth/token")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    read_json(resp).await
}

/// Go's `principalId` links the new client to the principal it
/// authenticates as on `client_credentials` (`serviceAccountPrincipalId` on
/// reads). It must be a service account's principal: Go takes any id the
/// foreign key accepts (an unknown one is a 500 there), and a user's would be
/// refused at every token request anyway.
#[tokio::test]
#[ignore = "requires Docker"]
async fn oauth_client_create_links_the_service_principal_it_names() {
    let app = setup().await;
    let token = app.anchor_admin_token().await;
    let sa = assert_status(
        app.post(
            "/api/service-accounts",
            &token,
            json!({"code": "gaps-oauth-sa", "name": "Gaps OAuth SA"}),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    let principal_id = sa["principalId"].as_str().expect("principalId").to_string();

    let created = assert_status(
        app.post(
            "/api/oauth-clients",
            &token,
            json!({
                "clientName": "Gaps machine client",
                "clientType": "CONFIDENTIAL",
                "grantTypes": ["client_credentials"],
                "principalId": principal_id
            }),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(
        created["client"]["serviceAccountPrincipalId"],
        principal_id.as_str()
    );
    let id = created["client"]["id"].as_str().unwrap();
    let read = get_json(&app, &format!("/api/oauth-clients/{id}"), &token).await;
    assert_eq!(read["serviceAccountPrincipalId"], principal_id.as_str());

    // The link is what the client_credentials grant authenticates as.
    let (s, body) = client_credentials(
        &app,
        created["client"]["clientId"].as_str().unwrap(),
        created["clientSecret"].as_str().unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{body}");
    assert!(body["access_token"].is_string());

    let (s, body) = read_json(
        app.post(
            "/api/oauth-clients",
            &token,
            json!({"clientName": "X", "clientType": "CONFIDENTIAL", "principalId": "prn_nobody"}),
        )
        .await,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["error"], "Principal_NOT_FOUND");

    let user = Principal::new_user("gaps-user@flowcatalyst.test", UserScope::Anchor);
    app.repos.principal_repo.insert(&user).await.unwrap();
    let (s, body) = read_json(
        app.post(
            "/api/oauth-clients",
            &token,
            json!({"clientName": "X", "clientType": "CONFIDENTIAL", "principalId": user.id}),
        )
        .await,
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "PRINCIPAL_NOT_SERVICE_ACCOUNT");
}

// ── 5. Service-account role assignments: assignedBy, clientId ────────────

/// An anchor admin (ADMIN_ALL) whose principal id the test knows.
async fn known_admin(app: &TestApp) -> (String, String) {
    app.anchor_admin_token().await; // seeds `platform:test-admin`
    let mut admin = Principal::new_user("gaps-admin@flowcatalyst.test", UserScope::Anchor);
    admin.roles = vec![RoleAssignment::new("platform:test-admin")];
    let token = app
        .auth_service
        .generate_access_token(&admin)
        .expect("token");
    (token, admin.id.to_string())
}

/// Go documents `assignedBy` and `clientId` on a service account's role
/// assignments and fills neither. `assignedBy` is the administrator who
/// assigned the roles; `clientId` stays absent because role grants are not
/// client-scoped (a role applies wherever the account reaches).
#[tokio::test]
#[ignore = "requires Docker"]
async fn service_account_role_assignments_record_who_assigned_them() {
    let app = setup().await;
    let (token, admin_id) = known_admin(&app).await;
    let sa = assert_status(
        app.post(
            "/api/service-accounts",
            &token,
            json!({"code": "gaps-roles-sa", "name": "Gaps Roles SA"}),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    let id = sa["serviceAccount"]["id"].as_str().unwrap().to_string();
    let roles_path = format!("/api/service-accounts/{id}/roles");

    let assigned = assert_status(
        app.put(
            &roles_path,
            &token,
            json!({"roles": ["platform:test-admin"]}),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let row = &assigned["roles"][0];
    assert_eq!(row["roleName"], "platform:test-admin");
    assert_eq!(row["assignedBy"], admin_id.as_str(), "{assigned}");
    assert_eq!(row["assignmentSource"], "ADMIN_ASSIGNED");
    assert!(row.get("clientId").is_none());

    let listed = get_json(&app, &roles_path, &token).await;
    assert_eq!(listed["roles"][0]["assignedBy"], admin_id.as_str());

    // A row written without it (Go, a sync, or before the column) reads
    // without the member.
    sqlx::query("UPDATE iam_principal_roles SET assigned_by = NULL")
        .execute(&app.pool)
        .await
        .unwrap();
    let listed = get_json(&app, &roles_path, &token).await;
    assert!(listed["roles"][0].get("assignedBy").is_none(), "{listed}");
}

// ── 6. Service-account update: webhookCredentials ────────────────────────

/// Go's update replaces the account's webhook credentials with the
/// `webhookCredentials` it is sent, storing the type, token, signing secret
/// and algorithm and dropping the other four members; they are all stored
/// here (secrets sealed, as the token and signing secret always are). Every
/// member is write-only; the delivery path signs with the new token and
/// secret; the audit row masks the secrets.
#[tokio::test]
#[ignore = "requires Docker"]
async fn service_account_update_replaces_the_webhook_credentials() {
    use fc_platform::service_account::outbound_credentials::{ById, OutboundCredentialsResolver};
    use fc_platform::shared::encryption_service::EncryptionService;

    let app = setup().await;
    let token = app.anchor_admin_token().await;
    let sa = assert_status(
        app.post(
            "/api/service-accounts",
            &token,
            json!({"code": "gaps-webhook-sa", "name": "Gaps Webhook SA"}),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    let id = sa["serviceAccount"]["id"].as_str().unwrap().to_string();
    let path = format!("/api/service-accounts/{id}");

    let (s, body) = read_json(
        app.put(
            &path,
            &token,
            json!({"webhookCredentials": {
                "authType": "BASIC_AUTH",
                "token": "tok-123",
                "username": "svc-user",
                "password": "pw-456",
                "headerName": "X-Api-Key",
                "signingSecret": "sig-789",
                "signingAlgorithm": "HMAC_SHA256",
                "signatureHeader": "X-Signature"
            }}),
        )
        .await,
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT, "{body}");

    let enc = EncryptionService::from_env().expect("app key");
    #[allow(clippy::type_complexity)]
    let row: (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT wh_auth_type, wh_auth_token_ref, wh_username, wh_password_ref, \
         wh_header_name, wh_signing_secret_ref, wh_signing_algorithm, wh_signature_header \
         FROM iam_service_accounts WHERE id = $1",
    )
    .bind(&id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(row.0.as_deref(), Some("BASIC_AUTH"));
    assert_eq!(
        enc.decrypt_ref(row.1.as_deref().unwrap()).unwrap(),
        "tok-123"
    );
    assert_eq!(row.2.as_deref(), Some("svc-user"));
    let password_ref = row.3.as_deref().unwrap();
    assert!(password_ref.starts_with("encrypted:"), "never plaintext");
    assert_eq!(enc.decrypt_ref(password_ref).unwrap(), "pw-456");
    assert_eq!(row.4.as_deref(), Some("X-Api-Key"));
    assert_eq!(
        enc.decrypt_ref(row.5.as_deref().unwrap()).unwrap(),
        "sig-789"
    );
    assert_eq!(row.6.as_deref(), Some("HMAC_SHA256"));
    assert_eq!(row.7.as_deref(), Some("X-Signature"));

    // Reads answer the type only.
    let read = get_json(&app, &path, &token).await;
    assert_eq!(read["authType"], "BASIC_AUTH");
    for member in ["webhookCredentials", "token", "password", "signingSecret"] {
        assert!(read.get(member).is_none(), "{member}: {read}");
    }

    // Deliveries are signed with the new token and secret.
    let resolver = OutboundCredentialsResolver::new(
        Arc::new(fc_platform::ServiceAccountRepository::new(&app.pool)),
        Some(Arc::new(enc)),
    );
    let resolved = resolver.by_service_account_id(&id).await.unwrap();
    let ById::Found(creds) = resolved else {
        panic!("expected credentials, got {resolved:?}");
    };
    assert_eq!(creds.token.as_deref(), Some("tok-123"));
    assert_eq!(creds.signing_secret.as_deref(), Some("sig-789"));

    // The audit row keeps the shape and masks the secrets.
    let (audited,): (Value,) = sqlx::query_as(
        "SELECT operation_json FROM aud_logs WHERE operation_json ? 'webhookCredentials' \
         ORDER BY performed_at DESC LIMIT 1",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    let wh = &audited["webhookCredentials"];
    assert_eq!(wh["username"], "svc-user");
    for secret in ["token", "password", "signingSecret"] {
        assert_eq!(wh[secret], "***", "{secret}: {audited}");
    }

    // An update without them leaves them; an unknown type or algorithm is a
    // 400 and changes nothing.
    let (s, _) = read_json(app.put(&path, &token, json!({"name": "Renamed"})).await).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let kept: (Option<String>,) =
        sqlx::query_as("SELECT wh_username FROM iam_service_accounts WHERE id = $1")
            .bind(&id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(kept.0.as_deref(), Some("svc-user"));
    for (creds, code) in [
        (json!({"authType": "OAUTH_MAGIC"}), "INVALID_AUTH_TYPE"),
        (
            json!({"authType": "HMAC_SIGNATURE", "signingAlgorithm": "MD5"}),
            "INVALID_SIGNING_ALGORITHM",
        ),
    ] {
        let (s, body) = read_json(
            app.put(&path, &token, json!({"webhookCredentials": creds}))
                .await,
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["error"], code);
    }

    // Go's replace: `NONE` clears every member.
    let (s, _) = read_json(
        app.put(
            &path,
            &token,
            json!({"webhookCredentials": {"authType": "NONE"}}),
        )
        .await,
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let cleared: (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT wh_auth_type, wh_auth_token_ref, wh_password_ref \
         FROM iam_service_accounts WHERE id = $1",
    )
    .bind(&id)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(cleared, (Some("NONE".to_string()), None, None));
}

// ── 7. Dispatch-job list rows: priority ──────────────────────────────────

/// Run the dispatch-job projector for a moment.
async fn project_dispatch_jobs(pool: &sqlx::PgPool) {
    let cancel = CancellationToken::new();
    let projector = tokio::spawn(dispatch_job_projection::run(
        pool.clone(),
        200,
        Arc::new(StreamHealth::new("dispatch-job-projection".into())),
        cancel.clone(),
    ));
    time::sleep(Duration::from_millis(1500)).await;
    cancel.cancel();
    time::timeout(Duration::from_secs(10), projector)
        .await
        .expect("projector stops")
        .unwrap();
}

/// Go documents `priority` on the list row and never fills it. It is the
/// job's own priority claim, projected to the read row: 1 for
/// `HIGH_PRIORITY`, 0 for `DEFAULT`, absent when the job claims neither.
#[tokio::test]
#[ignore = "requires Docker"]
async fn dispatch_job_rows_answer_the_jobs_own_priority() {
    let app = setup().await;
    let token = app.anchor_admin_token().await;
    let item = |code: &str, queue: Option<&str>| {
        let mut item = json!({
            "source": "gaps",
            "code": code,
            "targetUrl": "https://receiver.example.test/hook",
            "payload": "{}",
            "serviceAccountId": "sac_nobody",
        });
        if let Some(queue) = queue {
            item["queue"] = json!(queue);
        }
        item
    };
    let (s, body) = read_json(
        app.post(
            "/api/dispatch-jobs/batch",
            &token,
            json!({"items": [
                item("gaps:jobs:job:high", Some("HIGH_PRIORITY")),
                item("gaps:jobs:job:default", Some("DEFAULT")),
                item("gaps:jobs:job:unclaimed", None),
            ]}),
        )
        .await,
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{body}");
    project_dispatch_jobs(&app.pool).await;

    for path in [
        "/api/dispatch-jobs",
        "/api/dispatch-jobs/list-raw",
        "/bff/dispatch-jobs",
    ] {
        let rows = get_json(&app, path, &token).await;
        let row = |code: &str| {
            rows.as_array()
                .unwrap()
                .iter()
                .find(|r| r["code"] == code)
                .unwrap_or_else(|| panic!("{path}: no {code} in {rows}"))
                .clone()
        };
        assert_eq!(row("gaps:jobs:job:high")["priority"], 1, "{path}");
        assert_eq!(row("gaps:jobs:job:default")["priority"], 0, "{path}");
        assert!(
            row("gaps:jobs:job:unclaimed").get("priority").is_none(),
            "{path}"
        );
    }
}

// ── 8. Raw debug reads: RawEventResponse, RawDispatchJobResponse ─────────

/// No member of `row` is `null`: Go's raw shapes leave an absent member out.
fn assert_no_nulls(row: &Value) {
    for (key, value) in row.as_object().expect("an object") {
        assert!(!value.is_null(), "{key} is null in {row}");
    }
}

/// The debug BFF reads answer Go's `RawEventResponse` and
/// `RawDispatchJobResponse`: absent members left out (never `null`), the
/// event's context data, the job's payload length rather than its payload.
#[tokio::test]
#[ignore = "requires Docker"]
async fn debug_raw_reads_answer_gos_raw_shapes() {
    let app = setup().await;
    let token = app.anchor_admin_token().await;
    sqlx::query(
        "INSERT INTO msg_events (id, type, source, time, data, context_data) VALUES \
         ('evtraw0000001', 'gaps:orders:order:placed', 'urn:gaps', NOW(), '{\"n\":1}', \
          '[{\"key\":\"orderId\",\"value\":\"42\"}]'), \
         ('evtraw0000002', 'gaps:orders:order:placed', 'urn:gaps', NOW(), NULL, NULL)",
    )
    .execute(&app.pool)
    .await
    .unwrap();

    let events = get_json(&app, "/bff/debug/events?size=10", &token).await;
    let full = events
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == "evtraw0000001")
        .expect("the event")
        .clone();
    assert_no_nulls(&full);
    assert_eq!(full["eventType"], "gaps:orders:order:placed");
    assert_eq!(full["data"], json!({"n": 1}));
    assert_eq!(
        full["contextData"],
        json!([{"key": "orderId", "value": "42"}])
    );
    let bare = get_json(&app, "/bff/debug/events/evtraw0000002", &token).await;
    assert_no_nulls(&bare);
    for absent in [
        "data",
        "contextData",
        "subject",
        "clientId",
        "deduplicationId",
    ] {
        assert!(bare.get(absent).is_none(), "{absent}: {bare}");
    }
    assert_eq!(bare["specVersion"], "1.0");

    let (s, body) = read_json(
        app.post(
            "/api/dispatch-jobs/batch",
            &token,
            json!({"items": [{
                "source": "gaps",
                "code": "gaps:jobs:job:raw",
                "targetUrl": "https://receiver.example.test/hook",
                "payload": "{\"k\":\"v\"}",
                "serviceAccountId": "sac_nobody",
            }]}),
        )
        .await,
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{body}");
    let jobs = get_json(&app, "/bff/debug/dispatch-jobs?size=10", &token).await;
    let job = &jobs[0];
    assert_no_nulls(job);
    assert_eq!(job["code"], "gaps:jobs:job:raw");
    assert_eq!(job["payloadLength"], 9);
    assert_eq!(job["attemptHistoryCount"], 0);
    assert!(job.get("payload").is_none());
}
