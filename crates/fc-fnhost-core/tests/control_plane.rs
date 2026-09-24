//! `HttpControlPlane` and `PlatformSource` against a fake platform: ETag and
//! 304, one token refresh on a 401, the heartbeat's 204, emit error codes,
//! and the artifact download route.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use fc_fnhost_core::artifact::{
    ArtifactCache, ArtifactError, ArtifactStore, ArtifactStores, PlatformSource, DEFAULT_MAX_BYTES,
};
use fc_fnhost_core::clock::SystemClock;
use fc_fnhost_core::control_plane::{
    ControlPlane, ControlPlaneErrorReason, EmitItem, EmitRequest, Fetched, HttpControlPlane,
};
use fc_fnhost_core::digest::Digest;
use fc_fnhost_core::heartbeat::{HeartbeatReport, HostState};
use fc_fnhost_core::token::TokenSource;
use fc_function_abi::FunctionAddress;
use parking_lot::Mutex;
use serde_json::json;
use sha2::{Digest as _, Sha256};

#[derive(Default)]
struct Platform {
    minted: AtomicUsize,
    /// Tokens the platform accepts; others get 401.
    valid_tokens: Mutex<Vec<String>>,
    etag: Mutex<String>,
    body: Mutex<String>,
    heartbeat_status: Mutex<u16>,
    heartbeats: Mutex<Vec<String>>,
    if_none_match: Mutex<Vec<Option<String>>>,
    pools: Mutex<Vec<String>>,
    emit_status: Mutex<(u16, String)>,
    emits: Mutex<Vec<String>>,
    artifact: Mutex<Vec<u8>>,
}

fn authorised(platform: &Platform, headers: &HeaderMap) -> bool {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    platform.valid_tokens.lock().iter().any(|t| t == token)
}

async fn token(State(p): State<Arc<Platform>>) -> Response {
    let n = p.minted.fetch_add(1, Ordering::SeqCst);
    axum::Json(json!({"access_token": format!("tok-{n}"), "expires_in": 3600})).into_response()
}

async fn desired(
    State(p): State<Arc<Platform>>,
    Query(q): Query<std::collections::HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    if !authorised(&p, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    p.pools
        .lock()
        .push(q.get("pool").cloned().unwrap_or_default());
    let known = headers
        .get("if-none-match")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    p.if_none_match.lock().push(known.clone());
    let etag = p.etag.lock().clone();
    if known.as_deref() == Some(etag.as_str()) {
        return StatusCode::NOT_MODIFIED.into_response();
    }
    let mut response = (StatusCode::OK, p.body.lock().clone()).into_response();
    if !etag.is_empty() {
        response.headers_mut().insert("etag", etag.parse().unwrap());
    }
    response
}

async fn heartbeat(State(p): State<Arc<Platform>>, headers: HeaderMap, body: String) -> Response {
    if !authorised(&p, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    p.heartbeats.lock().push(body);
    StatusCode::from_u16(*p.heartbeat_status.lock())
        .unwrap()
        .into_response()
}

async fn events(State(p): State<Arc<Platform>>, headers: HeaderMap, body: String) -> Response {
    if !authorised(&p, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    p.emits.lock().push(body);
    let (status, body) = p.emit_status.lock().clone();
    (StatusCode::from_u16(status).unwrap(), body).into_response()
}

async fn artifact(
    State(p): State<Arc<Platform>>,
    Path(version_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !authorised(&p, &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if version_id != "fnv_1" {
        return StatusCode::NOT_FOUND.into_response();
    }
    Response::new(Body::from(p.artifact.lock().clone()))
}

struct Rig {
    platform: Arc<Platform>,
    control: HttpControlPlane,
    tokens: Arc<TokenSource>,
    url: String,
}

async fn rig() -> Rig {
    let platform = Arc::new(Platform::default());
    *platform.heartbeat_status.lock() = 204;
    *platform.emit_status.lock() = (201, r#"{"results":[]}"#.into());
    *platform.etag.lock() = "\"e1\"".into();
    *platform.body.lock() = json!({"functions": []}).to_string();
    let app = Router::new()
        .route("/oauth/token", post(token))
        .route("/control/functions/desired-state", get(desired))
        .route("/control/functions/heartbeat", post(heartbeat))
        .route("/control/functions/events", post(events))
        .route("/control/functions/artifacts/{version_id}", get(artifact))
        .with_state(platform.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = HttpControlPlane::default_client();
    let tokens = Arc::new(TokenSource::new(
        client.clone(),
        url.clone(),
        "id",
        "secret",
        Arc::new(SystemClock),
    ));
    Rig {
        control: HttpControlPlane::new(client, url.clone(), tokens.clone()),
        platform,
        tokens,
        url,
    }
}

#[tokio::test]
async fn desired_state_sends_the_etag_and_reads_304_as_not_modified() {
    let r = rig().await;
    r.platform.valid_tokens.lock().push("tok-0".into());
    let first = r.control.desired_state("blue", None).await.unwrap();
    assert!(matches!(first, Fetched::Changed { ref etag, .. } if etag == "\"e1\""));
    let second = r
        .control
        .desired_state("blue", Some("\"e1\""))
        .await
        .unwrap();
    assert_eq!(second, Fetched::NotModified);
    assert_eq!(
        *r.platform.if_none_match.lock(),
        [None, Some("\"e1\"".to_owned())]
    );
    assert_eq!(*r.platform.pools.lock(), ["blue", "blue"]);
    assert_eq!(
        r.platform.minted.load(Ordering::SeqCst),
        1,
        "the token is cached across calls"
    );
}

#[tokio::test]
async fn a_401_refreshes_the_token_once_and_retries_once() {
    let r = rig().await;
    r.platform.valid_tokens.lock().push("tok-1".into());
    let fetched = r.control.desired_state("default", None).await.unwrap();
    assert!(matches!(fetched, Fetched::Changed { .. }));
    assert_eq!(r.platform.minted.load(Ordering::SeqCst), 2);

    // a platform that rejects every token: refreshed once, then UNAUTHORIZED
    r.platform.valid_tokens.lock().clear();
    let err = r.control.desired_state("default", None).await.unwrap_err();
    assert_eq!(err.reason, ControlPlaneErrorReason::Unauthorized);
    assert_eq!(
        r.platform.minted.load(Ordering::SeqCst),
        3,
        "exactly one refresh, never a loop"
    );
}

#[tokio::test]
async fn malformed_responses_are_unavailable() {
    let r = rig().await;
    r.platform.valid_tokens.lock().push("tok-0".into());
    *r.platform.etag.lock() = String::new();
    let err = r.control.desired_state("default", None).await.unwrap_err();
    assert_eq!(err.reason, ControlPlaneErrorReason::Unavailable, "no ETag");
    *r.platform.etag.lock() = "\"e2\"".into();
    *r.platform.body.lock() = "[1,2]".into();
    let err = r.control.desired_state("default", None).await.unwrap_err();
    assert_eq!(
        err.reason,
        ControlPlaneErrorReason::Unavailable,
        "not a document"
    );
    let down = HttpControlPlane::new(
        HttpControlPlane::default_client(),
        "http://127.0.0.1:1",
        r.tokens.clone(),
    );
    assert_eq!(
        down.desired_state("default", None)
            .await
            .unwrap_err()
            .reason,
        ControlPlaneErrorReason::Unavailable
    );
}

#[tokio::test]
async fn heartbeat_posts_the_report_and_expects_204() {
    let r = rig().await;
    r.platform.valid_tokens.lock().push("tok-0".into());
    let report = HeartbeatReport {
        host_id: "h".into(),
        pool: "default".into(),
        state: HostState::Active,
        loaded: vec![],
    };
    r.control.heartbeat(&report).await.unwrap();
    assert_eq!(
        *r.platform.heartbeats.lock(),
        [r#"{"hostId":"h","pool":"default","state":"ACTIVE","loaded":[]}"#]
    );
    *r.platform.heartbeat_status.lock() = 200;
    assert_eq!(
        r.control.heartbeat(&report).await.unwrap_err().reason,
        ControlPlaneErrorReason::Unavailable
    );
}

fn emit_request() -> EmitRequest {
    EmitRequest {
        host_id: "h".into(),
        address: FunctionAddress::parse("app.svc.fn").unwrap(),
        version: 3,
        events: vec![EmitItem {
            event_type: "app:order:shipped".into(),
            subject: Some("order/1".into()),
            dedup_id: "d1".into(),
            data: json!({"id": 1}),
            correlation_id: None,
            causation_id: None,
            message_group: None,
        }],
    }
}

#[tokio::test]
async fn emit_maps_platform_errors_to_their_code_and_status() {
    let r = rig().await;
    r.platform.valid_tokens.lock().push("tok-0".into());
    r.control.emit(&emit_request()).await.unwrap();
    assert!(r.platform.emits.lock()[0].starts_with(r#"{"hostId":"h","address":"app.svc.fn","version":3,"events":[{"type":"app:order:shipped","subject":"order/1","dedupId":"d1""#));

    *r.platform.emit_status.lock() = (
        403,
        r#"{"error":"EVENT_TYPE_NOT_OWNED","message":"no"}"#.into(),
    );
    let err = r.control.emit(&emit_request()).await.unwrap_err();
    assert_eq!((err.code(), err.status()), ("EVENT_TYPE_NOT_OWNED", 403));

    *r.platform.emit_status.lock() = (500, "not json".into());
    let err = r.control.emit(&emit_request()).await.unwrap_err();
    assert_eq!((err.code(), err.status()), ("UNKNOWN", 500));

    let down = HttpControlPlane::new(
        HttpControlPlane::default_client(),
        "http://127.0.0.1:1",
        r.tokens.clone(),
    );
    let err = down.emit(&emit_request()).await.unwrap_err();
    assert_eq!((err.code(), err.status()), ("UNAVAILABLE", 503));
}

#[tokio::test]
async fn platform_artifacts_download_by_version_id_with_one_refresh() {
    let r = rig().await;
    r.platform.valid_tokens.lock().push("tok-1".into());
    let content = b"\0asm platform artifact".to_vec();
    *r.platform.artifact.lock() = content.clone();
    let digest = Digest::from_sha256(&Sha256::digest(&content).into());
    let dir = tempfile::tempdir().unwrap();
    let stores = ArtifactStores::new(ArtifactCache::new(dir.path(), DEFAULT_MAX_BYTES).unwrap())
        .with_source(
            "platform",
            Arc::new(PlatformSource::new(
                reqwest::Client::new(),
                r.url.clone(),
                r.tokens.clone(),
            )),
        );
    let reference = format!("platform://fnc_1/{}", digest.hex());
    let fetched = stores
        .fetch(&reference, &digest, Some("fnv_1"))
        .await
        .unwrap();
    assert_eq!(std::fs::read(fetched.file).unwrap(), content);
    assert_eq!(
        r.platform.minted.load(Ordering::SeqCst),
        2,
        "the first token was refused once"
    );

    let other = Digest::from_sha256(&[5; 32]);
    assert_eq!(
        stores.fetch(&reference, &other, None).await,
        Err(ArtifactError::VersionRequired)
    );
    assert_eq!(
        stores.fetch(&reference, &other, Some("fnv_9")).await,
        Err(ArtifactError::NotFound)
    );
}
