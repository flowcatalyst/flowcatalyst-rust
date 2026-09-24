//! What every test guest shares: reading the request, writing the response,
//! a query lookup and a small JSON writer (no serde: keeps each guest small).

use wasip2::http::types::{
    Fields, IncomingBody, IncomingRequest, Method, OutgoingBody, OutgoingResponse, ResponseOutparam,
};

pub use wasip2;

/// The request as a guest sees it.
pub struct Request {
    pub method: String,
    pub path_with_query: String,
    pub path: String,
    pub query: String,
    pub authority: Option<String>,
    pub scheme: Option<String>,
    pub headers: Vec<(String, Vec<u8>)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn read(req: IncomingRequest) -> Self {
        let path_with_query = req.path_with_query().unwrap_or_default();
        let (path, query) = match path_with_query.split_once('?') {
            Some((p, q)) => (p.to_string(), q.to_string()),
            None => (path_with_query.clone(), String::new()),
        };
        let scheme = req.scheme().map(|s| match s {
            wasip2::http::types::Scheme::Http => "http".to_string(),
            wasip2::http::types::Scheme::Https => "https".to_string(),
            wasip2::http::types::Scheme::Other(o) => o,
        });
        Self {
            method: method(req.method()),
            path,
            query,
            authority: req.authority(),
            scheme,
            headers: req.headers().entries(),
            body: read_body(req.consume().expect("the body is consumed once")),
            path_with_query,
        }
    }

    /// The first `name=value` in the query (no percent-decoding beyond
    /// `%3A`, `%2F`, `%3F`, `%3D`, `%26`, which is all the tests send).
    pub fn q(&self, name: &str) -> Option<String> {
        self.query.split('&').find_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            (k == name).then(|| decode(v))
        })
    }
}

fn decode(v: &str) -> String {
    v.replace("%3A", ":")
        .replace("%2F", "/")
        .replace("%3F", "?")
        .replace("%3D", "=")
        .replace("%26", "&")
        .replace('+', " ")
}

pub fn method(m: Method) -> String {
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

pub fn read_body(body: IncomingBody) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let stream = body.stream().expect("the body stream is taken once");
        while let Ok(chunk) = stream.blocking_read(64 * 1024) {
            out.extend(chunk);
        }
    }
    IncomingBody::finish(body);
    out
}

/// Sends `status`, `headers` and `body` (written in 4 KiB chunks).
pub fn respond(out: ResponseOutparam, status: u16, headers: &[(&str, &str)], body: &[u8]) {
    let fields: Vec<(String, Vec<u8>)> = headers
        .iter()
        .map(|(k, v)| (k.to_string(), v.as_bytes().to_vec()))
        .collect();
    let response = OutgoingResponse::new(Fields::from_list(&fields).expect("legal headers"));
    response.set_status_code(status).expect("a legal status");
    let outgoing = response.body().expect("the body is taken once");
    ResponseOutparam::set(out, Ok(response));
    {
        let stream = outgoing.write().expect("the stream is taken once");
        for chunk in body.chunks(4096) {
            stream
                .blocking_write_and_flush(chunk)
                .expect("the host accepts the body");
        }
    }
    OutgoingBody::finish(outgoing, None).expect("the body finishes");
}

/// `respond` with `Content-Type: application/json`.
pub fn json(out: ResponseOutparam, status: u16, body: &Json) {
    respond(
        out,
        status,
        &[("content-type", "application/json")],
        body.0.as_bytes(),
    );
}

/// A JSON value, written by hand.
pub struct Json(pub String);

impl Json {
    pub fn str(s: &str) -> Self {
        let mut out = String::with_capacity(s.len() + 2);
        out.push('"');
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                c => out.push(c),
            }
        }
        out.push('"');
        Json(out)
    }

    pub fn opt(s: Option<&str>) -> Self {
        s.map_or(Json("null".into()), Json::str)
    }

    pub fn num(n: impl std::fmt::Display) -> Self {
        Json(n.to_string())
    }

    pub fn bool(b: bool) -> Self {
        Json(b.to_string())
    }

    pub fn arr(items: impl IntoIterator<Item = Json>) -> Self {
        let parts: Vec<String> = items.into_iter().map(|j| j.0).collect();
        Json(format!("[{}]", parts.join(",")))
    }

    pub fn obj(members: impl IntoIterator<Item = (&'static str, Json)>) -> Self {
        let parts: Vec<String> = members
            .into_iter()
            .map(|(k, v)| format!("{}:{}", Json::str(k).0, v.0))
            .collect();
        Json(format!("{{{}}}", parts.join(",")))
    }

    /// An object with owned keys.
    pub fn map(members: impl IntoIterator<Item = (String, Json)>) -> Self {
        let parts: Vec<String> = members
            .into_iter()
            .map(|(k, v)| format!("{}:{}", Json::str(&k).0, v.0))
            .collect();
        Json(format!("{{{}}}", parts.join(",")))
    }
}

/// Headers as `[[name, value], …]`, values read as UTF-8 (lossy).
pub fn headers_json(headers: &[(String, Vec<u8>)]) -> Json {
    Json::arr(
        headers
            .iter()
            .map(|(k, v)| Json::arr([Json::str(k), Json::str(&String::from_utf8_lossy(v))])),
    )
}
