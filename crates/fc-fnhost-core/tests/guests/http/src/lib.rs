//! `http`: one outbound call through `wasi:http/outgoing-handler`.
//! `?url=U` (scheme://authority/path), `&method=M` (default GET),
//! `&timeoutMs=N` (the guest's own first-byte timeout). Answers
//! `{"status":…,"xUpstream":…,"body":…,"ms":…}` or
//! `{"error":"<the wasi:http error code>","ms":…}`, always with 200.

use common::{json, read_body, Json, Request};
use wasip2::clocks::monotonic_clock;
use wasip2::http::outgoing_handler;
use wasip2::http::types::{
    Fields, IncomingRequest, Method, OutgoingRequest, RequestOptions, ResponseOutparam, Scheme,
};

struct Http;
wasip2::http::proxy::export!(Http);

fn call(url: &str, method: &str, timeout_ms: Option<u64>) -> Result<Json, String> {
    let (scheme, rest) = url.split_once("://").ok_or("not a URL")?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let headers = Fields::from_list(&[("x-from-guest".to_string(), b"yes".to_vec())])
        .map_err(|e| format!("{e:?}"))?;
    let request = OutgoingRequest::new(headers);
    let scheme = match scheme {
        "https" => Scheme::Https,
        "http" => Scheme::Http,
        other => Scheme::Other(other.to_string()),
    };
    let method = match method {
        "POST" => Method::Post,
        "PUT" => Method::Put,
        _ => Method::Get,
    };
    request.set_method(&method).map_err(|_| "method")?;
    request.set_scheme(Some(&scheme)).map_err(|_| "scheme")?;
    request
        .set_authority(Some(authority))
        .map_err(|_| "authority")?;
    request
        .set_path_with_query(Some(path))
        .map_err(|_| "path")?;
    let options = timeout_ms.map(|ms| {
        let o = RequestOptions::new();
        let _ = o.set_first_byte_timeout(Some(ms * 1_000_000));
        o
    });
    let pending = outgoing_handler::handle(request, options).map_err(|e| format!("{e:?}"))?;
    pending.subscribe().block();
    let response = pending
        .get()
        .ok_or("no response")?
        .map_err(|_| "taken twice")?
        .map_err(|e| format!("{e:?}"))?;
    let status = response.status();
    let upstream: Vec<String> = response
        .headers()
        .get("x-upstream")
        .into_iter()
        .map(|v| String::from_utf8_lossy(&v).to_string())
        .collect();
    let location = response
        .headers()
        .get("location")
        .first()
        .map(|v| String::from_utf8_lossy(v).to_string());
    let body = read_body(response.consume().map_err(|_| "body")?);
    Ok(Json::obj([
        ("status", Json::num(status)),
        (
            "xUpstream",
            Json::opt(
                (!upstream.is_empty())
                    .then(|| upstream.join(", "))
                    .as_deref(),
            ),
        ),
        ("location", Json::opt(location.as_deref())),
        ("body", Json::str(&String::from_utf8_lossy(&body))),
    ]))
}

impl wasip2::exports::http::incoming_handler::Guest for Http {
    fn handle(req: IncomingRequest, out: ResponseOutparam) {
        let req = Request::read(req);
        let url = req.q("url").unwrap_or_default();
        let method = req.q("method").unwrap_or_else(|| "GET".into());
        let timeout_ms = req.q("timeoutMs").and_then(|v| v.parse().ok());
        let start = monotonic_clock::now();
        let result = call(&url, &method, timeout_ms);
        let ms = Json::num((monotonic_clock::now() - start) / 1_000_000);
        let body = match result {
            Ok(Json(ok)) => Json(format!("{},\"ms\":{}}}", &ok[..ok.len() - 1], ms.0)),
            Err(e) => Json::obj([("error", Json::str(&e)), ("ms", ms)]),
        };
        json(out, 200, &body);
    }
}
