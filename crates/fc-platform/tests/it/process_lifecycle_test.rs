//! Go's process fixes (docs/platform-bug-fixes.md in flowcatalyst-go, bugs
//! 23 and 25, ccf2dd5): edit and delete follow the status with Go's codes and
//! messages, and a blank description is stored as none. Requires Docker (or
//! `FC_TEST_PG_BIN`).

use crate::support;

use axum::http::StatusCode;
use serde_json::json;

use support::{assert_status, read_json, TestApp};

#[tokio::test]
#[ignore = "requires Docker"]
async fn process_edit_and_delete_follow_the_status() {
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token().await;

    let created = assert_status(
        app.post(
            "/api/processes",
            &admin,
            json!({
                "code": "orders:fulfilment:shipping",
                "name": "Shipping",
                "description": "  ",
                "body": "graph TD; A-->B",
            }),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    let id = created["id"].as_str().expect("id").to_string();
    let path = format!("/api/processes/{id}");
    let got = assert_status(app.get(&path, &admin).await, StatusCode::OK).await;
    assert!(
        got.get("description").is_none(),
        "a blank description is none: {got}"
    );

    // A current process is not deleted.
    let (status, body) = read_json(app.delete(&path, &admin).await).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "PROCESS_NOT_ARCHIVED", "{body}");
    assert_eq!(body["message"], "Archive the process before deleting it.");

    // A description set, then blanked, is none again.
    assert_status(
        app.put(&path, &admin, json!({ "description": "Ships orders" }))
            .await,
        StatusCode::NO_CONTENT,
    )
    .await;
    assert_status(
        app.put(&path, &admin, json!({ "description": "" })).await,
        StatusCode::NO_CONTENT,
    )
    .await;
    let got = assert_status(app.get(&path, &admin).await, StatusCode::OK).await;
    assert!(got.get("description").is_none(), "{got}");

    // An archived process is not edited, and is deleted.
    let (status, body) = read_json(
        app.post(&format!("{path}/archive"), &admin, json!({}))
            .await,
    )
    .await;
    assert!(status.is_success(), "{status}: {body}");
    let (status, body) =
        read_json(app.put(&path, &admin, json!({ "name": "Renamed" })).await).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "PROCESS_ARCHIVED", "{body}");
    assert_eq!(body["message"], "Archived processes cannot be edited.");
    let (status, body) = read_json(app.delete(&path, &admin).await).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
}
