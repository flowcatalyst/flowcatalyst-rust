//! A minimal in-process HTTP stub for the client tests.
//!
//! Each test registers canned responses by method and path, points a
//! [`FlowCatalystClient`] at the stub, and afterwards asserts on what the
//! client sent (method, path, query and JSON body). No mock-server crate is
//! needed: the stub speaks just enough HTTP/1.1 for reqwest.

use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::FlowCatalystClient;
use tokio::net::TcpListener;

/// One request the stub received.
#[derive(Debug, Clone)]
pub(crate) struct Recorded {
    pub method: String,
    /// Path without the query string.
    pub path: String,
    /// Raw query string (after `?`), empty when absent.
    pub query: String,
    pub body: String,
}

impl Recorded {
    /// The request body parsed as JSON (`Null` when empty).
    pub fn json(&self) -> serde_json::Value {
        if self.body.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_str(&self.body).expect("request body is JSON")
        }
    }

    /// The decoded query pairs, in order.
    pub fn query_pairs(&self) -> Vec<(String, String)> {
        reqwest::Url::parse(&format!("http://stub/?{}", self.query))
            .unwrap()
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect()
    }
}

#[derive(Clone)]
struct Route {
    method: String,
    path: String,
    status: u16,
    body: String,
}

/// The stub platform: canned routes and a log of received requests.
pub(crate) struct MockPlatform {
    pub base_url: String,
    requests: Arc<Mutex<Vec<Recorded>>>,
}

impl MockPlatform {
    /// Start a stub answering each `(method, path, status, body)` route.
    /// Unmatched requests get a 404 with an empty body.
    pub async fn start(routes: &[(&str, &str, u16, &str)]) -> Self {
        let routes: Vec<Route> = routes
            .iter()
            .map(|(m, p, s, b)| Route {
                method: m.to_string(),
                path: p.to_string(),
                status: *s,
                body: b.to_string(),
            })
            .collect();
        let routes = Arc::new(routes);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = requests.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let routes = routes.clone();
                let log = log.clone();
                tokio::spawn(async move {
                    let mut raw = Vec::new();
                    let mut buf = [0u8; 8192];
                    let (head_end, length) = loop {
                        let n = socket.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        raw.extend_from_slice(&buf[..n]);
                        let text = String::from_utf8_lossy(&raw).to_string();
                        let Some(head_end) = text.find("\r\n\r\n") else {
                            continue;
                        };
                        let length: usize = text[..head_end]
                            .lines()
                            .find_map(|l| {
                                let (k, v) = l.split_once(':')?;
                                k.trim()
                                    .eq_ignore_ascii_case("content-length")
                                    .then(|| v.trim().parse().ok())?
                            })
                            .unwrap_or(0);
                        if raw.len() >= head_end + 4 + length {
                            break (head_end, length);
                        }
                    };
                    let text = String::from_utf8_lossy(&raw).to_string();
                    let request_line = text.lines().next().unwrap_or_default().to_string();
                    let mut parts = request_line.split_whitespace();
                    let method = parts.next().unwrap_or_default().to_string();
                    let target = parts.next().unwrap_or_default().to_string();
                    let (path, query) = match target.split_once('?') {
                        Some((p, q)) => (p.to_string(), q.to_string()),
                        None => (target.clone(), String::new()),
                    };
                    let body = String::from_utf8_lossy(&raw[head_end + 4..head_end + 4 + length])
                        .to_string();
                    log.lock().unwrap().push(Recorded {
                        method: method.clone(),
                        path: path.clone(),
                        query,
                        body,
                    });
                    let route = routes
                        .iter()
                        .find(|r| r.method == method && r.path == path)
                        .cloned()
                        .unwrap_or(Route {
                            method,
                            path,
                            status: 404,
                            body: String::new(),
                        });
                    let content_type = if route.body.is_empty() {
                        String::new()
                    } else {
                        "Content-Type: application/json\r\n".to_string()
                    };
                    let response = format!(
                        "HTTP/1.1 {} Stub\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                        route.status,
                        content_type,
                        route.body.len(),
                        route.body
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        Self {
            base_url: format!("http://{addr}"),
            requests,
        }
    }

    /// A client pointed at this stub.
    pub fn client(&self) -> FlowCatalystClient {
        FlowCatalystClient::new(self.base_url.clone()).with_token("tok")
    }

    /// Every request received so far, in order.
    pub fn requests(&self) -> Vec<Recorded> {
        self.requests.lock().unwrap().clone()
    }

    /// The only request received; panics unless exactly one arrived.
    pub fn single(&self) -> Recorded {
        let all = self.requests();
        assert_eq!(all.len(), 1, "expected one request, got {all:?}");
        all.into_iter().next().unwrap()
    }
}
