//! Go's event-type bug fixes (docs/platform-bug-fixes.md in flowcatalyst-go):
//! the `/bff/event-types` reads check the read permission and match every
//! value of a repeated facet (bugs 1, 2, 308589b); add schema stores the
//! document and honours the MIME and schema type (bug 3, 7b5453a); archive
//! and delete enforce the version rules (bug 5, 62b72e3); the sync leaves
//! unchanged types alone (bug 6, b7aaaa4); Sync Platform Events needs the
//! event-type sync permission (a3414cc). Requires Docker (or
//! `FC_TEST_PG_BIN`).

use crate::support;

use axum::http::StatusCode;
use serde_json::{json, Value};

use fc_common::tsid::{self, EntityType};
use fc_platform::domain::{Principal, UserScope};
use fc_platform::role::entity::permissions::admin;
use support::{assert_status, read_json, TestApp};

/// `EventTypeUpdated::EVENT_TYPE`.
const UPDATED: &str = "platform:admin:eventtype:updated";

/// An anchor-tier token whose `scope` grants exactly `perms`.
fn anchor_holding(app: &TestApp, perms: &[&str]) -> String {
    let p = Principal::new_user("et-bugs@flowcatalyst.test", UserScope::Anchor);
    let granted: Vec<String> = perms.iter().map(|s| s.to_string()).collect();
    app.auth_service
        .generate_access_token_with_scope(&p, &granted, None)
        .expect("token")
}

async fn insert_event_type(app: &TestApp, code: &str) {
    let parts: Vec<&str> = code.split(':').collect();
    sqlx::query(
        "INSERT INTO msg_event_types
           (id, code, name, status, source, client_scoped, application, subdomain, aggregate,
            created_at, updated_at)
         VALUES ($1, $2, $2, 'CURRENT', 'UI', false, $3, $4, $5, NOW(), NOW())",
    )
    .bind(tsid::generate(EntityType::EventType))
    .bind(code)
    .bind(parts[0])
    .bind(parts[1])
    .bind(parts[2])
    .execute(&app.pool)
    .await
    .expect("insert event type");
}

fn strings(v: &Value) -> Vec<String> {
    let mut out: Vec<String> = v
        .as_array()
        .unwrap_or_else(|| panic!("not an array: {v}"))
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect();
    out.sort();
    out
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn bff_event_type_reads_check_the_permission_and_match_every_facet_value() {
    let app = TestApp::setup().await;
    insert_event_type(&app, "alpha:sales:order:created").await;
    insert_event_type(&app, "beta:billing:invoice:sent").await;
    insert_event_type(&app, "gamma:ops:job:ran").await;

    // Bug 1: no event-type view, no reads (the filters answered anyone).
    let no_view = anchor_holding(&app, &[admin::EVENT_TYPE_CREATE]);
    for path in [
        "/bff/event-types",
        "/bff/event-types/filters/applications",
        "/bff/event-types/filters/subdomains",
        "/bff/event-types/filters/aggregates",
    ] {
        let (status, body) = read_json(app.get(path, &no_view).await).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {body}");
    }

    // Bug 2: a repeated facet matches any of its values.
    let viewer = anchor_holding(&app, &[admin::EVENT_TYPE_READ]);
    let list = assert_status(
        app.get(
            "/bff/event-types?application=alpha&application=beta",
            &viewer,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let mut codes: Vec<&str> = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|et| et["code"].as_str().unwrap())
        .collect();
    codes.sort();
    assert_eq!(
        codes,
        ["alpha:sales:order:created", "beta:billing:invoice:sent"],
        "{list}"
    );

    let subdomains = assert_status(
        app.get(
            "/bff/event-types/filters/subdomains?application=alpha&application=beta",
            &viewer,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(strings(&subdomains["options"]), ["billing", "sales"]);

    let aggregates = assert_status(
        app.get(
            "/bff/event-types/filters/aggregates?application=alpha&application=gamma&subdomain=ops",
            &viewer,
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(strings(&aggregates["options"]), ["job"]);
}

/// The stored schema of `version`: (JSON type of the content, schema type,
/// MIME type).
async fn stored_version(
    app: &TestApp,
    event_type_id: &str,
    version: &str,
) -> (String, String, String) {
    sqlx::query_as(
        "SELECT jsonb_typeof(schema_content), schema_type, mime_type
           FROM msg_event_type_spec_versions WHERE event_type_id = $1 AND version = $2",
    )
    .bind(event_type_id)
    .bind(version)
    .fetch_one(&app.pool)
    .await
    .expect("spec version")
}

async fn create_bff_event_type(app: &TestApp, token: &str, code: &str) -> String {
    let created = assert_status(
        app.post(
            "/bff/event-types",
            token,
            json!({ "code": code, "name": code, "clientScoped": false }),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    created["id"].as_str().expect("id").to_string()
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn add_schema_stores_the_document_and_honours_mime_and_schema_type() {
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token().await;
    let id = create_bff_event_type(&app, &admin, "schemas:orders:order:created").await;
    let path = format!("/bff/event-types/{id}/schemas");

    // The SPA sends its textarea's text: a JSON string holding the document.
    assert_status(
        app.post(
            &path,
            &admin,
            json!({ "version": "1.0", "schema": "{\"type\": \"object\"}", "mimeType": "" }),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        stored_version(&app, &id, "1.0").await,
        (
            "object".to_string(),
            "JSON_SCHEMA".to_string(),
            "application/schema+json".to_string()
        )
    );

    // An XSD is text, with its type's default MIME type.
    assert_status(
        app.post(
            &path,
            &admin,
            json!({ "version": "1.1", "schema": "<xs:schema/>", "schemaType": "xsd" }),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        stored_version(&app, &id, "1.1").await,
        (
            "string".to_string(),
            "XSD".to_string(),
            "application/xml".to_string()
        )
    );

    for (body, code) in [
        (
            json!({ "version": "1.2", "schema": "not a document" }),
            "INVALID_SCHEMA",
        ),
        (
            json!({ "version": "1.2", "schema": { "a": 1 }, "schemaType": "PROTO" }),
            "INVALID_SCHEMA",
        ),
        (
            json!({ "version": "1.2", "schema": {}, "schemaType": "YAML" }),
            "INVALID_SCHEMA_TYPE",
        ),
        (
            json!({ "version": "1.2", "schema": {}, "mimeType": "x".repeat(101) }),
            "INVALID_MIME_TYPE",
        ),
    ] {
        let (status, resp) = read_json(app.post(&path, &admin, body.clone()).await).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}: {resp}");
        assert_eq!(resp["error"], code, "{body}: {resp}");
    }

    // The API path gets the same normalisation (no new body fields).
    assert_status(
        app.post(
            &format!("/api/event-types/{id}/schemas"),
            &admin,
            json!({ "version": "1.3", "schema": "[{\"type\": \"string\"}]" }),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    assert_eq!(stored_version(&app, &id, "1.3").await.0, "array");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn archive_and_delete_enforce_the_version_rules() {
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token().await;
    let id = create_bff_event_type(&app, &admin, "rules:orders:order:created").await;
    assert_status(
        app.post(
            &format!("/bff/event-types/{id}/schemas"),
            &admin,
            json!({ "version": "1.0", "schema": { "type": "object" } }),
        )
        .await,
        StatusCode::OK,
    )
    .await;

    // A FINALISING version: not every version is deprecated.
    let (status, body) = read_json(
        app.post(&format!("/bff/event-types/{id}/archive"), &admin, json!({}))
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "SCHEMAS_NOT_DEPRECATED", "{body}");
    assert_eq!(body["message"], "Deprecate every schema version first.");

    // A CURRENT version: neither archived nor all finalising.
    assert_status(
        app.post(
            &format!("/bff/event-types/{id}/schemas/1.0/finalise"),
            &admin,
            json!({}),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    for path in [
        format!("/bff/event-types/{id}"),
        format!("/api/event-types/{id}"),
    ] {
        let (status, body) = read_json(app.delete(&path, &admin).await).await;
        assert_eq!(status, StatusCode::CONFLICT, "{path}: {body}");
        assert_eq!(body["error"], "DELETE_NOT_ALLOWED", "{path}: {body}");
        assert_eq!(
            body["message"],
            "Only an archived event type, or one whose schema versions are all still finalising, can be deleted."
        );
    }

    // Deprecated, it archives; archived, it deletes.
    assert_status(
        app.post(
            &format!("/bff/event-types/{id}/schemas/1.0/deprecate"),
            &admin,
            json!({}),
        )
        .await,
        StatusCode::OK,
    )
    .await;
    let (status, body) = read_json(
        app.post(&format!("/bff/event-types/{id}/archive"), &admin, json!({}))
            .await,
    )
    .await;
    assert!(status.is_success(), "{status}: {body}");
    let (status, body) =
        read_json(app.delete(&format!("/bff/event-types/{id}"), &admin).await).await;
    assert!(status.is_success(), "{status}: {body}");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn sync_platform_needs_the_sync_permission_and_counts_unchanged_types_as_unchanged() {
    let app = TestApp::setup().await;
    let path = "/bff/event-types/sync-platform";

    // Anchor reach with an event-type write permission is not enough (a3414cc).
    let writer = anchor_holding(
        &app,
        &[
            admin::EVENT_TYPE_CREATE,
            admin::EVENT_TYPE_UPDATE,
            admin::EVENT_TYPE_DELETE,
        ],
    );
    let (status, body) = read_json(app.post(path, &writer, json!({})).await).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"], "PERMISSION_REQUIRED", "{body}");
    // The sync permission without anchor reach is not enough either.
    let p = Principal::new_user("et-sync@flowcatalyst.test", UserScope::Partner);
    let partner = app
        .auth_service
        .generate_access_token_with_scope(&p, &[admin::EVENT_TYPE_SYNC.to_string()], None)
        .expect("token");
    let (status, body) = read_json(app.post(path, &partner, json!({})).await).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    // Bug 6: a second run changes nothing and says so.
    let syncer = anchor_holding(&app, &[admin::EVENT_TYPE_SYNC]);
    let first = assert_status(app.post(path, &syncer, json!({})).await, StatusCode::OK).await;
    assert!(first["created"].as_u64().unwrap() > 0, "{first}");
    let events_after_first = app.event_count_by_type(UPDATED).await;
    let second = assert_status(app.post(path, &syncer, json!({})).await, StatusCode::OK).await;
    assert_eq!(second["created"], 0, "{second}");
    assert_eq!(second["updated"], 0, "{second}");
    assert_eq!(second["total"], first["total"], "{second}");
    let events_after_second = app.event_count_by_type(UPDATED).await;
    assert_eq!(
        events_after_first, events_after_second,
        "no update events for unchanged types"
    );
}
