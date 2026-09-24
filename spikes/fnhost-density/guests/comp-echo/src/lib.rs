//! (c) The F0 component guest: a `wasi:http/proxy` incoming handler with the same behaviours as
//! Java's fc_test_guest, chosen by path: `/echo`, `/spin`, `/alloc?mb=`, `/http?url=`,
//! `/config?key=`, `/log?msg=`. Plus one typed FlowCatalyst import (`flowcatalyst:function/host`).

wit_bindgen::generate!({ path: "../../wit", world: "guest-imports" });

use flowcatalyst::function::host;
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use wasi::http::types::{
    Fields, IncomingBody, IncomingRequest, Method, OutgoingBody, OutgoingRequest, OutgoingResponse, ResponseOutparam,
    Scheme,
};

static CALLS: AtomicU64 = AtomicU64::new(0);

struct Component;
wasi::http::proxy::export!(Component);

fn read_body(b: IncomingBody) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let s = b.stream().unwrap();
        while let Ok(chunk) = s.blocking_read(64 * 1024) {
            out.extend(chunk);
        }
    }
    IncomingBody::finish(b);
    out
}

fn respond(out: ResponseOutparam, status: u16, body: &[u8]) {
    let h = Fields::from_list(&[("content-type".to_string(), b"application/json".to_vec())]).unwrap();
    let r = OutgoingResponse::new(h);
    r.set_status_code(status).unwrap();
    let ob = r.body().unwrap();
    ResponseOutparam::set(out, Ok(r));
    {
        let w = ob.write().unwrap();
        for chunk in body.chunks(4096) {
            w.blocking_write_and_flush(chunk).unwrap();
        }
    }
    OutgoingBody::finish(ob, None).unwrap();
}

fn method(m: Method) -> String {
    match m {
        Method::Get => "GET".into(),
        Method::Post => "POST".into(),
        Method::Put => "PUT".into(),
        Method::Delete => "DELETE".into(),
        Method::Patch => "PATCH".into(),
        Method::Head => "HEAD".into(),
        Method::Options => "OPTIONS".into(),
        Method::Connect => "CONNECT".into(),
        Method::Trace => "TRACE".into(),
        Method::Other(s) => s,
    }
}

/// GET `url` through wasi:http/outgoing-handler. A policy denial arrives as a typed `ErrorCode`.
fn outbound(url: &str) -> Result<serde_json::Value, String> {
    let (scheme, rest) = url.split_once("://").ok_or("not a URL")?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let headers = Fields::from_list(&[("x-from-guest".to_string(), b"yes".to_vec())]).map_err(|e| format!("{e:?}"))?;
    let req = OutgoingRequest::new(headers);
    let scheme = match scheme {
        "https" => Scheme::Https,
        "http" => Scheme::Http,
        s => Scheme::Other(s.to_string()),
    };
    req.set_scheme(Some(&scheme)).map_err(|_| "scheme")?;
    req.set_authority(Some(authority)).map_err(|_| "authority")?;
    req.set_path_with_query(Some(path)).map_err(|_| "path")?;
    let fut = wasi::http::outgoing_handler::handle(req, None).map_err(|e| format!("{e:?}"))?;
    fut.subscribe().block();
    let resp = fut.get().ok_or("no response")?.map_err(|_| "taken")?.map_err(|e| format!("{e:?}"))?;
    let status = resp.status();
    let upstream: Vec<String> =
        resp.headers().get(&"x-upstream".to_string()).into_iter().map(|v| String::from_utf8_lossy(&v).to_string()).collect();
    let body = read_body(resp.consume().map_err(|_| "body")?);
    Ok(json!({"status": status, "xUpstream": if upstream.is_empty() { None } else { Some(upstream.join(", ")) },
              "body": String::from_utf8_lossy(&body)}))
}

impl wasi::exports::http::incoming_handler::Guest for Component {
    fn handle(req: IncomingRequest, out: ResponseOutparam) {
        let calls = CALLS.fetch_add(1, Ordering::SeqCst) + 1;
        let pq = req.path_with_query().unwrap_or_default();
        let (path, query) = pq.split_once('?').unwrap_or((pq.as_str(), ""));
        let q = |name: &str| {
            query.split('&').find_map(|kv| {
                let (k, v) = kv.split_once('=')?;
                (k == name).then(|| v.to_string())
            })
        };
        let (status, body) = match path {
            "/echo" => {
                let headers: Vec<(String, String)> = req
                    .headers()
                    .entries()
                    .into_iter()
                    .map(|(k, v)| (k, String::from_utf8_lossy(&v).to_string()))
                    .collect();
                let m = method(req.method());
                let body = read_body(req.consume().unwrap());
                (200, json!({"method": m, "path": pq, "headers": headers,
                             "body": String::from_utf8_lossy(&body), "calls": calls}))
            }
            "/spin" => {
                if q("spin").as_deref() == Some("false") {
                    (200, json!({"spun": false}))
                } else {
                    let mut n: u64 = 0;
                    loop {
                        n = std::hint::black_box(n.wrapping_add(1));
                    }
                }
            }
            "/alloc" => {
                let mb: usize = q("mb").and_then(|v| v.parse().ok()).unwrap_or(1);
                let v = vec![7u8; mb * 1024 * 1024];
                let sum: u64 = std::hint::black_box(&v).iter().step_by(4096).map(|b| *b as u64).sum();
                (200, json!({"allocatedMb": mb, "sum": sum}))
            }
            "/http" => match outbound(&q("url").unwrap_or_default()) {
                Ok(v) => (200, v),
                Err(e) => (200, json!({"error": e})),
            },
            "/config" => {
                let key = q("key").unwrap_or_default();
                (200, json!({"key": key, "value": host::config_get(&key)}))
            }
            "/log" => {
                let msg = q("msg").unwrap_or_else(|| "hello".into());
                host::log(host::Level::Info, &format!("guest info: {msg}"));
                host::log(host::Level::Warn, &format!("guest warn: {msg}"));
                println!("guest stdout: {msg}");
                eprintln!("guest stderr: {msg}");
                (200, json!({"logged": msg}))
            }
            _ => (404, json!({"error": "no such path"})),
        };
        respond(out, status, body.to_string().as_bytes());
    }
}
