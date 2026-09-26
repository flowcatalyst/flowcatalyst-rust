//! The business apps that run against the platform (inhance integral, hr,
//! rfp through the Laravel SDK; AgentPlanner), replayed request for request
//! against the Rust platform. Each test names the app code it replays.
//! Requires Docker.

#[path = "support/mod.rs"]
mod support;

#[allow(unused_imports)]
use serde_json::{json, Value};

use fc_platform::client::entity::Client;
use fc_platform::domain::{Principal, UserScope};
use support::{read_json, TestApp};

/// An unauthenticated JSON POST (login, logout).
async fn post_unauth(
    app: &TestApp,
    path: &str,
    body: Value,
) -> axum::http::Response<axum::body::Body> {
    use tower::ServiceExt;
    let req = axum::http::Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(axum::body::Body::from(body.to_string()))
        .unwrap();
    app.router.clone().oneshot(req).await.unwrap()
}

async fn insert_user(app: &TestApp, email: &str, active: bool) -> Principal {
    let mut p = Principal::new_user(email, UserScope::Anchor);
    p.active = active;
    app.repos
        .principal_repo
        .insert(&p)
        .await
        .expect("insert principal");
    p
}

/// hr `PrincipalDirectory.php:151` and rfp `PlatformPrincipalDirectory.php:216`
/// call `GET /api/principals?active=true` with no page size and expect every
/// active user back (Go returns all rows, principal/api/api.go:272-283); hr's
/// role import sends `?type=USER&active=true`.
#[tokio::test]
#[ignore = "requires Docker"]
async fn hr_and_rfp_list_every_active_principal() {
    let app = TestApp::setup().await;
    for i in 0..25 {
        insert_user(&app, &format!("user{i:02}@inhance.test"), true).await;
    }
    insert_user(&app, "gone@inhance.test", false).await;
    let token = app.anchor_admin_token().await;

    let (status, body) = read_json(app.get("/api/principals?active=true", &token).await).await;
    assert_eq!(status, 200, "{body}");
    let principals = body["principals"].as_array().unwrap();
    assert_eq!(principals.len(), 25, "{body}");
    assert_eq!(body["total"], 25);
    assert!(principals.iter().all(|p| p["active"] == true));
    // Principal::fromArray requires id, type and name.
    for p in principals {
        assert!(p["id"].is_string() && p["type"] == "USER" && p["name"].is_string());
    }

    let (status, body) = read_json(
        app.get("/api/principals?type=USER&active=true", &token)
            .await,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["principals"].as_array().unwrap().len(), 25);

    let (_, body) = read_json(app.get("/api/principals?active=false", &token).await).await;
    assert_eq!(body["principals"].as_array().unwrap().len(), 1);

    // An explicit page size still pages.
    let (_, body) = read_json(
        app.get("/api/principals?active=true&page=1&pageSize=10", &token)
            .await,
    )
    .await;
    assert_eq!(body["principals"].as_array().unwrap().len(), 10);
    assert_eq!(body["total"], 25);
}

/// Go lists every application and every OAuth client when unfiltered
/// (application/api/api.go:63-81, auth/api/api.go:170-187); `?active=true`
/// parses.
#[tokio::test]
#[ignore = "requires Docker"]
async fn applications_and_oauth_clients_list_every_row_by_default() {
    let app = TestApp::setup().await;
    let token = app.anchor_admin_token().await;
    let mut retired =
        fc_platform::application::entity::Application::new("retired-app", "Retired App");
    retired.active = false;
    app.repos
        .application_repo
        .insert(&retired)
        .await
        .expect("insert application");

    let (status, body) = read_json(app.get("/api/applications", &token).await).await;
    assert_eq!(status, 200, "{body}");
    let codes: Vec<&str> = body["applications"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["code"].as_str().unwrap())
        .collect();
    assert!(codes.contains(&"retired-app"), "{codes:?}");
    assert!(codes.contains(&"platform"), "{codes:?}");

    let (status, body) = read_json(app.get("/api/applications?active=true", &token).await).await;
    assert_eq!(status, 200, "{body}");
    assert!(body["applications"]
        .as_array()
        .unwrap()
        .iter()
        .all(|a| a["active"] == true));

    let (status, body) = read_json(app.get("/api/oauth-clients?active=true", &token).await).await;
    assert_eq!(status, 200, "{body}");
    let (status, _) = read_json(app.get("/api/oauth-clients", &token).await).await;
    assert_eq!(status, 200);
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

/// integral `SyncUsersToFlowCatalystCommand.php:589-599` through the Laravel
/// SDK (`Principals::syncUsers`, `SyncPrincipalEntry::toArray`): one entry
/// per call, `roles: []`, `active` omitted, the local bcrypt hash verbatim.
/// Go: principal/api/sync.go:43-76, operations/sync_principals.go:68-246.
#[tokio::test]
#[ignore = "requires Docker"]
async fn integral_syncs_a_user_with_its_password_hash() {
    let app = TestApp::setup().await;
    let token = app.anchor_admin_token().await;
    let hash = "$2y$10$eImiTXuWVxfM37uY4JANjQ==eImiTXuWVxfM37uY4JANjQ.abcdefg";

    let (status, body) = read_json(
        app.post(
            "/api/principals/sync",
            &token,
            json!({"principals": [{
                "email": "Jo.Bloggs@Inhance.test",
                "name": "Jo Bloggs",
                "roles": [],
                "passwordHash": hash
            }]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body,
        json!({"created": 1, "updated": 0, "deleted": 0, "syncedEmails": ["jo.bloggs@inhance.test"]})
    );

    let p = app
        .repos
        .principal_repo
        .find_by_email("jo.bloggs@inhance.test")
        .await
        .unwrap()
        .expect("synced user");
    assert_eq!(p.name, "Jo Bloggs");
    assert!(p.active);
    assert_eq!(p.scope, UserScope::Client);
    assert_eq!(p.client_id, None);
    assert_eq!(
        p.user_identity.as_ref().unwrap().password_hash.as_deref(),
        Some(hash)
    );
    assert_eq!(
        app.event_count_by_type("platform:iam:user:created").await,
        1
    );
    assert_eq!(
        app.event_count_by_type("platform:iam:principals:synced")
            .await,
        1
    );
    // The hash never reaches the audit log.
    let (audit,): (Value,) = sqlx::query_as(
        "SELECT operation_json FROM aud_logs WHERE operation = 'SyncPrincipalsCommand' LIMIT 1",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(audit["principals"][0]["passwordHash"], "***", "{audit}");

    // A second sync updates: a new name and roles; an admin-assigned role
    // survives; an omitted hash keeps the stored one.
    let mut with_admin_role = p.clone();
    with_admin_role.assign_role("platform:viewer");
    app.repos
        .principal_repo
        .update(&with_admin_role)
        .await
        .unwrap();
    let (status, body) = read_json(
        app.post(
            "/api/principals/sync",
            &token,
            json!({"principals": [{
                "email": "jo.bloggs@inhance.test",
                "name": "Jo B",
                "roles": ["HR:Manager"]
            }]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["created"], 0);
    assert_eq!(body["updated"], 1);
    let p = app
        .repos
        .principal_repo
        .find_by_id(&p.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(p.name, "Jo B");
    let mut roles: Vec<&str> = p.roles.iter().map(|r| r.role.as_str()).collect();
    roles.sort();
    assert_eq!(roles, vec!["hr:manager", "platform:viewer"]);
    assert_eq!(
        p.user_identity.as_ref().unwrap().password_hash.as_deref(),
        Some(hash)
    );
    assert_eq!(
        app.event_count_by_type("platform:iam:user:updated").await,
        1
    );

    // An empty sync is a 400 (Go PRINCIPALS_REQUIRED).
    let (status, body) = read_json(
        app.post("/api/principals/sync", &token, json!({"principals": []}))
            .await,
    )
    .await;
    assert_eq!(status, 400, "{body}");
}

/// A user integral synced with its Laravel bcrypt hash (`$2y$`) signs in
/// with the password it always had, and the hash is re-encoded to Argon2id
/// on that login (Go auth/passwordhash/passwordhash.go:115-185,
/// auth/login/endpoint.go:510-525).
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_synced_laravel_user_signs_in_and_is_rehashed() {
    let app = TestApp::setup().await;
    let token = app.anchor_admin_token().await;
    let laravel_hash = bcrypt::hash("Tr0ub4dor&3", 10)
        .unwrap()
        .replacen("$2b$", "$2y$", 1);

    let (status, body) = read_json(
        app.post(
            "/api/principals/sync",
            &token,
            json!({"principals": [{
                "email": "sam@inhance.test",
                "name": "Sam",
                "roles": [],
                "passwordHash": laravel_hash
            }]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{body}");

    let (status, body) = read_json(
        post_unauth(
            &app,
            "/auth/login",
            json!({"email": "sam@inhance.test", "password": "wrong"}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 401, "{body}");

    let (status, body) = read_json(
        post_unauth(
            &app,
            "/auth/login",
            json!({"email": "sam@inhance.test", "password": "Tr0ub4dor&3"}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{body}");

    let stored = app
        .repos
        .principal_repo
        .find_by_email("sam@inhance.test")
        .await
        .unwrap()
        .unwrap()
        .user_identity
        .unwrap()
        .password_hash
        .unwrap();
    assert!(stored.starts_with("$argon2id$"), "{stored}");

    // And the upgraded hash still signs in.
    let (status, _) = read_json(
        post_unauth(
            &app,
            "/auth/login",
            json!({"email": "sam@inhance.test", "password": "Tr0ub4dor&3"}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200);
}

/// integral `CreateUserUseCase.php:110-122` (packages_root/.../
/// integral-service-v2): a tenant user is created with the tenant's **code**
/// as `clientId`; an `@inhanceapps.com` user with `scope: "ANCHOR"`. Go
/// resolves the code (resolveClientRef, principal/api/api.go:825-845) and
/// honours the requested tier (deriveUserScope, api.go:780-819).
#[tokio::test]
#[ignore = "requires Docker"]
async fn integral_creates_users_by_client_code_and_anchor_scope() {
    let app = TestApp::setup().await;
    let token = app.anchor_admin_token().await;
    let client_id = create_client(&app, "inhance").await;
    app.repos
        .anchor_domain_repo
        .insert(&fc_platform::auth::config_entity::AnchorDomain::new(
            "inhanceapps.com",
        ))
        .await
        .expect("insert anchor domain");

    let (status, body) = read_json(
        app.post(
            "/api/principals/users",
            &token,
            json!({
                "email": "tenant.user@example.test",
                "name": "Tenant User",
                "password": "abcdefghijklmnopqrstuvwxyzABCDEF",
                "clientId": "inhance",
                "enforcePasswordComplexity": false
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["clientId"], client_id.as_str(), "{body}");
    assert_eq!(body["scope"], "CLIENT");

    // The clt_ id works as well.
    let (status, body) = read_json(
        app.post(
            "/api/principals/users",
            &token,
            json!({"email": "second@example.test", "name": "Second",
                   "password": "abcdefghijklmnopqrstuvwxyzABCDEF",
                   "clientId": client_id, "enforcePasswordComplexity": false}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["clientId"], client_id.as_str());

    let (status, body) = read_json(
        app.post(
            "/api/principals/users",
            &token,
            json!({
                "email": "staff@inhanceapps.com",
                "name": "Staff",
                "password": "abcdefghijklmnopqrstuvwxyzABCDEF",
                "enforcePasswordComplexity": false,
                "scope": "ANCHOR"
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["scope"], "ANCHOR");
    assert!(body["clientId"].is_null(), "{body}");

    // An unknown code fails closed with Go's 404.
    let (status, body) = read_json(
        app.post(
            "/api/principals/users",
            &token,
            json!({"email": "x@example.test", "name": "X", "password": "abcdefghijklmnop",
                   "clientId": "no-such-tenant", "enforcePasswordComplexity": false}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 404, "{body}");
    assert_eq!(body["error"], "Client_NOT_FOUND");
    assert_eq!(body["message"], "Client not found: no-such-tenant");

    // ANCHOR on a domain that isn't an anchor domain is refused.
    let (status, body) = read_json(
        app.post(
            "/api/principals/users",
            &token,
            json!({"email": "y@example.test", "name": "Y", "password": "abcdefghijklmnop",
                   "enforcePasswordComplexity": false, "scope": "ANCHOR"}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"], "ANCHOR_DOMAIN_REQUIRED");
}

/// One item as the Laravel SDK's outbox writes it (`CreateEventDto::
/// toPayload`, src/Outbox/DTOs/CreateEventDto.php:230-245) and fc-outbox
/// forwards it: hr's grading events carry `clientCode` from
/// `FLOWCATALYST_CLIENT_CODE` (OutboxUnitOfWork.php:123-150) and `data` as a
/// JSON string.
fn hr_outbox_item(event_id: &str, client_code: &str) -> Value {
    json!({
        "specVersion": "1.0",
        "type": "hr:grading:grading-record:submitted",
        "source": "hr",
        "subject": "grading.grading-record.123",
        "data": "{\"gradingRecordId\":\"123\",\"status\":\"SUBMITTED\"}",
        "correlationId": "corr-1",
        "deduplicationId": format!("hr:grading:grading-record:submitted-{event_id}"),
        "messageGroup": "grading:grading-record:123",
        "clientCode": client_code,
        "contextData": [
            {"key": "principalId", "value": "prn_1"},
            {"key": "executionId", "value": null},
            {"key": "aggregateType", "value": "GradingRecord"}
        ]
    })
}

/// hr's `clientCode` resolves to the client's id, as Go does
/// (event/api/api.go:151-185): an explicit `clientId` wins and an unknown
/// code leaves the event unlinked but stored.
#[tokio::test]
#[ignore = "requires Docker"]
async fn hr_outbox_events_are_linked_to_their_client_by_code() {
    let app = TestApp::setup().await;
    let token = app.anchor_admin_token().await;
    let inhance = create_client(&app, "inhance").await;
    let other = create_client(&app, "other").await;

    let mut explicit = hr_outbox_item("e3", "inhance");
    explicit["clientId"] = json!(other);
    let (status, body) = read_json(
        app.post(
            "/api/events/batch",
            &token,
            json!({"items": [
                hr_outbox_item("e1", "inhance"),
                hr_outbox_item("e2", "no-such-tenant"),
                explicit
            ]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 201, "{body}");
    let results = body["results"].as_array().unwrap();
    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|r| r["status"] == "SUCCESS"), "{body}");

    let rows: Vec<(String, Option<String>, Value)> = sqlx::query_as(
        "SELECT deduplication_id, client_id, context_data FROM msg_events
         WHERE type = 'hr:grading:grading-record:submitted' ORDER BY deduplication_id",
    )
    .fetch_all(&app.pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].1.as_deref(), Some(inhance.as_str()));
    assert_eq!(rows[1].1, None, "an unknown code leaves the event unlinked");
    assert_eq!(rows[2].1.as_deref(), Some(other.as_str()), "clientId wins");
    assert_eq!(rows[0].2[0]["key"], "principalId");
    assert_eq!(rows[0].2[1]["value"], "");
}

/// AgentPlanner (`oidc.py:248-258`) and the Laravel SDK
/// (`OidcAuthController.php:215-247`) end the session with `client_id`
/// instead of `id_token_hint` when they hold no id_token. Go accepts it
/// (auth/bridge/login_endpoint.go:174-230).
#[tokio::test]
#[ignore = "requires Docker"]
async fn logout_with_client_id_and_no_id_token_hint() {
    let app = TestApp::setup().await;
    let mut client =
        fc_platform::auth::oauth_entity::OAuthClient::new("agent-planner", "Agent Planner");
    client.post_logout_redirect_uris = vec!["https://planner.example.test/auth/logged-out".into()];
    app.repos
        .oauth_client_repo
        .insert(&client)
        .await
        .expect("insert oauth client");

    let redirect = "https%3A%2F%2Fplanner.example.test%2Fauth%2Flogged-out";
    let resp = app
        .get_unauth(&format!(
            "/auth/oidc/session/end?post_logout_redirect_uri={redirect}&client_id=agent-planner"
        ))
        .await;
    assert_eq!(resp.status(), 303);
    assert_eq!(
        resp.headers()["location"],
        "https://planner.example.test/auth/logged-out"
    );

    // A URI the client didn't register is still refused.
    let (status, body) = read_json(
        app.get_unauth(
            "/auth/oidc/session/end?post_logout_redirect_uri=https%3A%2F%2Fevil.test%2F&client_id=agent-planner",
        )
        .await,
    )
    .await;
    assert_eq!(status, 400, "{body}");

    // Neither a hint nor a client_id: the redirect can't be verified.
    let (status, body) = read_json(
        app.get_unauth(&format!(
            "/auth/oidc/session/end?post_logout_redirect_uri={redirect}"
        ))
        .await,
    )
    .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(
        body["error_description"],
        "Invalid post_logout_redirect_uri: id_token_hint or client_id is required to verify post_logout_redirect_uri"
    );
}

/// `/auth/me` carries the effective permissions the SPA gates pages on:
/// the principal's roles flattened in Go's order, plus `"*"` for the super-admin
/// wildcard (Go handleMe / buildPermissionList,
/// auth/login/endpoint.go:437-450, 656-694), and `ssoManaged`.
#[tokio::test]
#[ignore = "requires Docker"]
async fn auth_me_lists_the_effective_permissions() {
    use fc_platform::role::entity::AuthRole;

    let app = TestApp::setup().await;
    let viewer = AuthRole::new("hr", "viewer", "HR Viewer")
        .with_permission("hr:staff:record:view")
        .with_permission("hr:grading:record:view");
    let editor = AuthRole::new("hr", "editor", "HR Editor")
        .with_permission("hr:staff:record:view")
        .with_permission("hr:staff:record:update");
    for role in [&viewer, &editor] {
        app.repos.role_repo.insert(role).await.expect("insert role");
    }
    let client_id = create_client(&app, "inhance").await;
    let mut user =
        Principal::new_user("me@inhance.test", UserScope::Client).with_client_id(&client_id);
    user.assign_role("hr:viewer");
    user.assign_role("hr:editor");
    app.repos
        .principal_repo
        .insert(&user)
        .await
        .expect("insert user");
    let token = app.auth_service.generate_session_token(&user).unwrap();

    let (status, body) = read_json(app.get_with_session("/auth/me", &token).await).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["principalId"], user.id.as_str());
    assert_eq!(body["clientId"], client_id.as_str());
    assert_eq!(body["status"], "");
    assert_eq!(body["email"], "me@inhance.test");
    // Roles in assignment order (Go: `ORDER BY assigned_at`); the two were
    // assigned within the same instant, so either may come first.
    let roles: Vec<&str> = body["roles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.as_str().unwrap())
        .collect();
    let mut sorted_roles = roles.clone();
    sorted_roles.sort();
    assert_eq!(sorted_roles, ["hr:editor", "hr:viewer"]);
    assert_eq!(body["scope"], "CLIENT");
    // Go's order: role by role, each role's permissions sorted, a
    // permission kept where it first appears.
    let expected = if roles[0] == "hr:editor" {
        json!([
            "hr:staff:record:update",
            "hr:staff:record:view",
            "hr:grading:record:view"
        ])
    } else {
        json!([
            "hr:grading:record:view",
            "hr:staff:record:view",
            "hr:staff:record:update"
        ])
    };
    assert_eq!(body["permissions"], expected);
    assert_eq!(body["ssoManaged"], false);

    // The super-admin wildcard adds the "*" sentinel.
    let super_admin = AuthRole::new("platform", "test-super", "Super")
        .with_permission(fc_platform::role::entity::permissions::ADMIN_ALL);
    app.repos.role_repo.insert(&super_admin).await.unwrap();
    let mut admin = Principal::new_user("root@flowcatalyst.test", UserScope::Anchor);
    admin.assign_role("platform:test-super");
    app.repos.principal_repo.insert(&admin).await.unwrap();
    let token = app.auth_service.generate_session_token(&admin).unwrap();
    let (status, body) = read_json(app.get_with_session("/auth/me", &token).await).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["permissions"], json!(["platform:*:*:*", "*"]));
    assert!(body["clientId"].is_null());
    assert_eq!(body["scope"], "ANCHOR");

    // No permission at all: Go's empty list serialises as null.
    let bare = Principal::new_user("bare@flowcatalyst.test", UserScope::Anchor);
    app.repos.principal_repo.insert(&bare).await.unwrap();
    let bare_token = app.auth_service.generate_session_token(&bare).unwrap();
    let (status, body) = read_json(app.get_with_session("/auth/me", &bare_token).await).await;
    assert_eq!(status, 200, "{body}");
    assert!(body["permissions"].is_null(), "{body}");
    assert_eq!(body["roles"], json!([]));

    // A deactivated principal is no longer authenticated here.
    app.repos
        .principal_repo
        .update(&Principal {
            active: false,
            ..admin.clone()
        })
        .await
        .unwrap();
    let (status, _) = read_json(app.get_with_session("/auth/me", &token).await).await;
    assert_eq!(status, 401);
}

/// fc-outbox retries a batch it didn't see acknowledged, so the same hr
/// events (same `deduplicationId`) arrive twice. Go stores them once and
/// reports every item SUCCESS (event/repository.go:31-140,
/// event/api/api.go:197-205); a repeat inside one batch is dropped too.
#[tokio::test]
#[ignore = "requires Docker"]
async fn replayed_events_are_stored_once_and_acknowledged() {
    let app = TestApp::setup().await;
    let token = app.anchor_admin_token().await;
    create_client(&app, "inhance").await;

    let batch = json!({"items": [
        hr_outbox_item("r1", "inhance"),
        hr_outbox_item("r2", "inhance"),
        hr_outbox_item("r1", "inhance")
    ]});
    for attempt in 0..2 {
        let (status, body) = read_json(app.post("/api/events/batch", &token, &batch).await).await;
        assert_eq!(status, 201, "attempt {attempt}: {body}");
        let results = body["results"].as_array().unwrap();
        assert_eq!(results.len(), 3, "positional results: {body}");
        assert!(results.iter().all(|r| r["status"] == "SUCCESS"), "{body}");
    }
    let (count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM msg_events WHERE type = 'hr:grading:grading-record:submitted'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(count, 2);

    // An item with no deduplication id gets Go's `<type>-<tsid>` one.
    let mut no_dedup = hr_outbox_item("x", "inhance");
    no_dedup.as_object_mut().unwrap().remove("deduplicationId");
    let (status, _) = read_json(
        app.post("/api/events/batch", &token, json!({"items": [no_dedup]}))
            .await,
    )
    .await;
    assert_eq!(status, 201);
    let (dedup,): (Option<String>,) = sqlx::query_as(
        "SELECT deduplication_id FROM msg_events WHERE deduplication_id NOT LIKE '%-r_' AND type = 'hr:grading:grading-record:submitted'",
    )
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert!(dedup
        .unwrap()
        .starts_with("hr:grading:grading-record:submitted-"));
}

/// Decision #22 (owner ruling 4, Java f4cd1bc2): a sync's `passwordHash` is
/// used only to create the user. Re-running integral's sync with a new hash
/// leaves the stored one, applies the rest, and names the email.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_sync_never_replaces_an_existing_users_password_hash() {
    let app = TestApp::setup().await;
    let token = app.anchor_admin_token().await;
    let original = "$2y$10$eImiTXuWVxfM37uY4JANjQ==eImiTXuWVxfM37uY4JANjQ.original";
    let rotated = "$2y$10$eImiTXuWVxfM37uY4JANjQ==eImiTXuWVxfM37uY4JANjQ.rotated";

    let (status, body) = read_json(
        app.post(
            "/api/principals/sync",
            &token,
            json!({"principals": [{"email": "keep@inhance.test", "name": "Keep", "passwordHash": original}]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert!(body.get("passwordHashIgnored").is_none(), "{body}");

    let (status, body) = read_json(
        app.post(
            "/api/principals/sync",
            &token,
            json!({"principals": [
                {"email": "Keep@Inhance.test", "name": "Kept", "passwordHash": rotated},
                {"email": "fresh@inhance.test", "name": "Fresh", "passwordHash": rotated}
            ]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["passwordHashIgnored"], json!(["keep@inhance.test"]));
    let hash_of = |email: &'static str| {
        let app = &app;
        async move {
            let p = app
                .repos
                .principal_repo
                .find_by_email(email)
                .await
                .unwrap()
                .unwrap();
            (
                p.name.clone(),
                p.user_identity.unwrap().password_hash.unwrap(),
            )
        }
    };
    assert_eq!(
        hash_of("keep@inhance.test").await,
        ("Kept".to_string(), original.to_string())
    );
    assert_eq!(hash_of("fresh@inhance.test").await.1, rotated);
}

/// hr and rfp's `sync-test-principals` runs through the application-scoped
/// sync, which must carry the hash for the users it creates (Go
/// sdksync/api.go:485-527; the cutover blocker in the 2026-09-25 triage):
/// the Laravel `$2y$` hash signs in, and a re-run leaves it alone.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_application_sync_carries_the_hash_for_new_users() {
    use fc_platform::role::entity::permissions;
    let app = TestApp::setup().await;
    let hr = fc_platform::application::entity::Application::new("hr", "HR");
    app.repos.application_repo.insert(&hr).await.unwrap();
    // The app's sync principal: reaches every application, may sync users.
    let syncer = Principal::new_user("hr-sync@inhance.test", UserScope::Anchor);
    app.repos.principal_repo.insert(&syncer).await.unwrap();
    let token = app
        .auth_service
        .generate_access_token_with_scope(
            &syncer,
            &[permissions::iam::USER_CREATE.to_string()],
            None,
        )
        .unwrap();
    let laravel_hash = bcrypt::hash("Hr-Test-Pa55", 10)
        .unwrap()
        .replacen("$2b$", "$2y$", 1);

    let sync = |hash: String| {
        let (app, token) = (&app, token.clone());
        async move {
            read_json(
                app.post(
                    "/api/applications/hr/principals/sync",
                    &token,
                    json!({"principals": [{
                        "email": "tester@inhance.test", "name": "Tester",
                        "roles": ["hr:employee"], "passwordHash": hash
                    }]}),
                )
                .await,
            )
            .await
        }
    };
    let (status, body) = sync(laravel_hash.clone()).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["created"], 1);
    assert!(body.get("passwordHashIgnored").is_none(), "{body}");

    let (status, body) = read_json(
        post_unauth(
            &app,
            "/auth/login",
            json!({"email": "tester@inhance.test", "password": "Hr-Test-Pa55"}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{body}");

    // A re-run with another hash changes nothing about the password.
    let other = bcrypt::hash("Other-Pa55", 10).unwrap();
    let (status, body) = sync(other).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["updated"], 1);
    assert_eq!(body["passwordHashIgnored"], json!(["tester@inhance.test"]));
    let (status, _) = read_json(
        post_unauth(
            &app,
            "/auth/login",
            json!({"email": "tester@inhance.test", "password": "Hr-Test-Pa55"}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200);
}
