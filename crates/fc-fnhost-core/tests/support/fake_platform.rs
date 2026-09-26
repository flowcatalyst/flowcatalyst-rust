//! A fake FlowCatalyst platform: `/oauth/token` and the four
//! `/control/functions/*` routes, with Java's wire shapes (the ETag is the
//! sha256 of the body; 304 on a matching `If-None-Match`; heartbeat 204).

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use parking_lot::Mutex;
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};

#[derive(Default)]
pub struct FakePlatform {
    pub document: Mutex<Value>,
    pub artifacts: Mutex<HashMap<String, Vec<u8>>>,
    pub heartbeats: Mutex<Vec<Value>>,
    pub tokens_minted: AtomicUsize,
    pub desired_requests: AtomicUsize,
    pub not_modified: AtomicUsize,
    pub token_forms: Mutex<Vec<String>>,
}

impl FakePlatform {
    pub fn set_document(&self, document: Value) {
        *self.document.lock() = document;
    }

    pub fn last_heartbeat(&self) -> Option<Value> {
        self.heartbeats.lock().last().cloned()
    }

    /// `address@version → state[:error]` of the last heartbeat.
    pub fn last_states(&self) -> Vec<String> {
        let Some(beat) = self.last_heartbeat() else {
            return Vec::new();
        };
        beat["loaded"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                let mut s = format!(
                    "{}@{} {}",
                    e["address"].as_str().unwrap(),
                    e["version"],
                    e["state"].as_str().unwrap()
                );
                if let Some(error) = e.get("error").and_then(Value::as_str) {
                    s.push(':');
                    s.push_str(error);
                }
                s
            })
            .collect()
    }
}

fn authorised(headers: &HeaderMap) -> bool {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("Bearer tok-"))
}

async fn token(State(p): State<Arc<FakePlatform>>, body: String) -> Response {
    p.token_forms.lock().push(body);
    let n = p.tokens_minted.fetch_add(1, Ordering::SeqCst);
    axum::Json(
        json!({"access_token": format!("tok-{n}"), "token_type": "Bearer", "expires_in": 3600}),
    )
    .into_response()
}

async fn desired(State(p): State<Arc<FakePlatform>>, headers: HeaderMap) -> Response {
    if !authorised(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    p.desired_requests.fetch_add(1, Ordering::SeqCst);
    let body = p.document.lock().to_string();
    let etag = format!("\"{}\"", hex::encode(Sha256::digest(body.as_bytes())));
    if headers.get("if-none-match").and_then(|v| v.to_str().ok()) == Some(etag.as_str()) {
        p.not_modified.fetch_add(1, Ordering::SeqCst);
        return StatusCode::NOT_MODIFIED.into_response();
    }
    (
        [("etag", etag), ("content-type", "application/json".into())],
        body,
    )
        .into_response()
}

async fn heartbeat(
    State(p): State<Arc<FakePlatform>>,
    headers: HeaderMap,
    body: String,
) -> Response {
    if !authorised(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    p.heartbeats
        .lock()
        .push(serde_json::from_str(&body).unwrap());
    StatusCode::NO_CONTENT.into_response()
}

async fn artifact(
    State(p): State<Arc<FakePlatform>>,
    Path(version_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !authorised(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match p.artifacts.lock().get(&version_id) {
        Some(bytes) => Response::new(Body::from(bytes.clone())),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Starts the fake on an ephemeral port; returns it and its base URL.
pub async fn start() -> (Arc<FakePlatform>, String) {
    let platform = Arc::new(FakePlatform::default());
    platform.set_document(json!({"functions": []}));
    let app = Router::new()
        .route("/oauth/token", post(token))
        .route("/control/functions/desired-state", get(desired))
        .route("/control/functions/heartbeat", post(heartbeat))
        .route("/control/functions/artifacts/{version_id}", get(artifact))
        .with_state(platform.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (platform, url)
}

pub fn sha256_digest(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

/// A desired-state entry in the platform's wire shape.
pub fn entry(
    address: &str,
    version: i32,
    runtime: &str,
    mode: &str,
    artifact_ref: &str,
    content: &[u8],
) -> Value {
    json!({
        "address": address,
        "functionId": "fnc_1",
        "versionId": version_id(address, version),
        "version": version,
        "role": "live",
        "mode": mode,
        "digest": sha256_digest(content),
        "artifactRef": artifact_ref,
        "manifest": {"runtime": runtime, "entrypoint": "handle", "limits": {"maxDurationMs": 1000}},
        "applicationId": "app_1",
        "config": {},
        "secrets": {},
        "missingSettings": [],
        "aliases": []
    })
}

pub fn version_id(address: &str, version: i32) -> String {
    format!("fnv_{}_{version}", address.replace('.', "_"))
}

pub async fn wait_for(what: &str, mut condition: impl FnMut() -> bool) {
    for _ in 0..1000 {
        if condition() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for {what}");
}
