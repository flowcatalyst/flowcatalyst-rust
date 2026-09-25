//! The recording webhook receiver.
//!
//! One receiver per side (so recordings never mix), bound to a random
//! loopback port. Every request under `/hook/<scenario>/<target>` is
//! recorded and answered from the scenario's script for that target. A
//! target's script is installed before the scenario's stimuli are sent and
//! removed afterwards, so late deliveries from an earlier scenario are still
//! recorded (under their own scenario) but can never be answered by a later
//! script.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::Router;
use parking_lot::Mutex;
use serde::Serialize;

use crate::scenario::{ReceiverScript, ScriptedResponse};

/// One request the receiver saw.
#[derive(Debug, Clone, Serialize)]
pub struct Delivery {
    pub scenario: String,
    pub target: String,
    /// Harness key of the stimulus (`hk` in the payload), when found.
    pub hk: Option<String>,
    /// Message group the stimulus was sent with (`hg` in the payload).
    pub group: Option<String>,
    /// Sequence number within the group (`hs` in the payload).
    pub seq: Option<i64>,
    /// Milliseconds since the scenario's stimuli started.
    pub at_ms: u64,
    /// 1-based count of deliveries of this `hk` so far, including this one.
    pub attempt: u32,
    /// What the receiver answered.
    pub answered: String,
    /// Whether the target accepted the message: the script's answer is an
    /// acceptance *and* it was handed back to the caller. A caller that
    /// gave up first (its timeout) closed the exchange, so the answer never
    /// went out and the delivery is not an acceptance.
    pub accepted: bool,
    /// The script's answer would have been an acceptance.
    pub scripted_acceptance: bool,
    /// Milliseconds since the scenario start when the answer went out;
    /// `None` when the caller hung up first.
    pub answered_at_ms: Option<u64>,
    /// When the caller hung up without waiting for the answer.
    pub hung_up_at_ms: Option<u64>,
    /// Requests of this scenario in flight at the receiver when this one
    /// arrived, including itself (the pool-concurrency witness).
    pub concurrent: u32,
    pub headers: Vec<(String, String)>,
    pub body: serde_json::Value,
    #[serde(skip)]
    pub raw_body: String,
}

#[derive(Default)]
struct Inner {
    scripts: HashMap<(String, String), ReceiverScript>,
    starts: HashMap<String, Instant>,
    deliveries: Vec<Delivery>,
    per_hk: HashMap<(String, String), u32>,
    inflight: HashMap<String, u32>,
    router_config: Option<serde_json::Value>,
}

#[derive(Clone, Default)]
pub struct Receiver {
    inner: Arc<Mutex<Inner>>,
}

impl Receiver {
    /// Bind on 127.0.0.1 at a random port and serve until the process ends.
    pub async fn start() -> anyhow::Result<(Receiver, SocketAddr)> {
        let receiver = Receiver::default();
        let app = Router::new()
            .route("/hook/{scenario}/{target}", any(handle))
            .route("/health", any(|| async { "ok" }))
            .route("/router-config", any(router_config))
            .with_state(receiver.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Ok((receiver, addr))
    }

    pub fn install(&self, scenario: &str, target: &str, script: ReceiverScript) {
        let mut g = self.inner.lock();
        g.scripts
            .insert((scenario.to_string(), target.to_string()), script);
    }

    pub fn mark_start(&self, scenario: &str) {
        self.inner
            .lock()
            .starts
            .insert(scenario.to_string(), Instant::now());
    }

    pub fn uninstall_scenario(&self, scenario: &str) {
        self.inner.lock().scripts.retain(|(s, _), _| s != scenario);
    }

    pub fn deliveries(&self, scenario: &str) -> Vec<Delivery> {
        self.inner
            .lock()
            .deliveries
            .iter()
            .filter(|d| d.scenario == scenario)
            .cloned()
            .collect()
    }

    /// Serve this document at `/router-config` (the shim used when a
    /// side's platform serves no router-config document of its own).
    pub fn set_router_config(&self, doc: serde_json::Value) {
        self.inner.lock().router_config = Some(doc);
    }

    pub fn count(&self, scenario: &str) -> usize {
        self.inner
            .lock()
            .deliveries
            .iter()
            .filter(|d| d.scenario == scenario)
            .count()
    }
}

/// Find `hk`/`hg`/`hs` anywhere in the body. The envelope differs between
/// implementations (an event delivery wraps the event, a raw dispatch job
/// sends its payload as is, some wrap it in a string), so search
/// recursively, also inside string members that parse as JSON.
fn find_marker(v: &serde_json::Value, key: &str) -> Option<serde_json::Value> {
    match v {
        serde_json::Value::Object(m) => {
            if let Some(x) = m.get(key) {
                return Some(x.clone());
            }
            m.values().find_map(|c| find_marker(c, key))
        }
        serde_json::Value::Array(a) => a.iter().find_map(|c| find_marker(c, key)),
        serde_json::Value::String(s) if s.contains(key) => serde_json::from_str(s)
            .ok()
            .and_then(|inner: serde_json::Value| find_marker(&inner, key)),
        _ => None,
    }
}

async fn handle(
    State(rx): State<Receiver>,
    Path((scenario, target)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let body_json: serde_json::Value = serde_json::from_slice(&body)
        .unwrap_or_else(|_| serde_json::Value::String(String::from_utf8_lossy(&body).into()));
    let hk = find_marker(&body_json, "hk").and_then(|v| v.as_str().map(str::to_string));
    let group = find_marker(&body_json, "hg").and_then(|v| v.as_str().map(str::to_string));
    let seq = find_marker(&body_json, "hs").and_then(|v| v.as_i64());

    let (response, delivery_index) = {
        let mut g = rx.inner.lock();
        let at_ms = g
            .starts
            .get(&scenario)
            .map(|s| s.elapsed().as_millis() as u64)
            .unwrap_or(0);
        let key = (scenario.clone(), hk.clone().unwrap_or_default());
        let attempt = {
            let n = g.per_hk.entry(key).or_insert(0);
            *n += 1;
            *n
        };
        let script = g
            .scripts
            .get(&(scenario.clone(), target.clone()))
            .cloned()
            .unwrap_or_default();
        let response = script.pick(attempt, seq);
        let mut hdrs: Vec<(String, String)> = headers
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_string(),
                    v.to_str().unwrap_or("<binary>").to_string(),
                )
            })
            .collect();
        hdrs.sort();
        let concurrent = {
            let n = g.inflight.entry(scenario.clone()).or_insert(0);
            *n += 1;
            *n
        };
        g.deliveries.push(Delivery {
            scenario: scenario.clone(),
            target: target.clone(),
            hk,
            group,
            seq,
            at_ms,
            attempt,
            answered: response.describe(),
            accepted: false,
            scripted_acceptance: response.is_acceptance(),
            answered_at_ms: None,
            hung_up_at_ms: None,
            concurrent,
            headers: hdrs,
            body: body_json,
            raw_body: String::from_utf8_lossy(&body).into_owned(),
        });
        let idx = g.deliveries.len() - 1;
        (response, idx)
    };
    // Decrements the scenario's in-flight count however the exchange ends
    // (answered, or dropped by a caller that gave up waiting).
    let _inflight = InflightGuard {
        rx: rx.clone(),
        scenario: scenario.clone(),
        index: delivery_index,
    };

    if let Some(ms) = response.delay_ms {
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }
    if response.hang {
        // Never answer; the caller's own timeout ends the exchange.
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
    // Still here: the caller is still waiting (hyper drops this future when
    // the connection closes), so the answer goes out.
    {
        let mut g = rx.inner.lock();
        let at = g
            .starts
            .get(&scenario)
            .map(|s| s.elapsed().as_millis() as u64)
            .unwrap_or(0);
        if let Some(d) = g.deliveries.get_mut(delivery_index) {
            d.answered_at_ms = Some(at);
            d.accepted = d.scripted_acceptance;
        }
    }
    let status = StatusCode::from_u16(response.status).unwrap_or(StatusCode::OK);
    let mut builder = Response::builder().status(status);
    for (k, v) in &response.headers {
        builder = builder.header(k, v);
    }
    match &response.body {
        Some(b) => builder
            .header("content-type", "application/json")
            .body(axum::body::Body::from(b.to_string()))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()),
        None => builder
            .body(axum::body::Body::empty())
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()),
    }
}

struct InflightGuard {
    rx: Receiver,
    scenario: String,
    index: usize,
}

impl Drop for InflightGuard {
    fn drop(&mut self) {
        let mut g = self.rx.inner.lock();
        let at = g
            .starts
            .get(&self.scenario)
            .map(|s| s.elapsed().as_millis() as u64)
            .unwrap_or(0);
        if let Some(n) = g.inflight.get_mut(&self.scenario) {
            *n = n.saturating_sub(1);
        }
        if let Some(d) = g.deliveries.get_mut(self.index) {
            if d.answered_at_ms.is_none() {
                d.hung_up_at_ms = Some(at);
            }
        }
    }
}

async fn router_config(State(rx): State<Receiver>) -> Response {
    match rx.inner.lock().router_config.clone() {
        Some(doc) => axum::Json(doc).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

impl ScriptedResponse {
    pub fn describe(&self) -> String {
        let mut s = self.status.to_string();
        if let Some(b) = &self.body {
            s.push(' ');
            s.push_str(&b.to_string());
        }
        for (k, v) in &self.headers {
            s.push_str(&format!(" {k}:{v}"));
        }
        if let Some(ms) = self.delay_ms {
            s.push_str(&format!(" after {ms}ms"));
        }
        if self.hang {
            s.push_str(" (hang)");
        }
        s
    }

    /// A 2xx without `"ack": false` is the target accepting the message.
    pub fn is_acceptance(&self) -> bool {
        if self.hang || !(200..300).contains(&self.status) {
            return false;
        }
        !matches!(
            self.body.as_ref().and_then(|b| b.get("ack")),
            Some(serde_json::Value::Bool(false))
        )
    }
}
