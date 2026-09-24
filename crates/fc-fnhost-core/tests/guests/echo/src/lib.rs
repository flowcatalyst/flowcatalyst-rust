//! `echo`: every request field and the invocation context, as JSON.
//! `?sleepMs=N` waits N ms first (a host-side wait, not a spin);
//! `?status=N` answers with that status; `?bodyBytes=N` answers N bytes of
//! `a` instead, streamed 4 KiB at a time (never held in guest memory).
//! `calls` counts the calls this instance has served (always 1: instance
//! per request).

use common::{headers_json, Json, Request};
use fc::invocation::{self, Caller};
use std::sync::atomic::{AtomicU64, Ordering};
use wasip2::clocks::monotonic_clock;
use wasip2::http::types::{
    Fields, IncomingRequest, OutgoingBody, OutgoingResponse, ResponseOutparam,
};

static CALLS: AtomicU64 = AtomicU64::new(0);

struct Echo;
wasip2::http::proxy::export!(Echo);

impl wasip2::exports::http::incoming_handler::Guest for Echo {
    fn handle(req: IncomingRequest, out: ResponseOutparam) {
        let calls = CALLS.fetch_add(1, Ordering::SeqCst) + 1;
        let req = Request::read(req);
        if let Some(n) = req.q("bodyBytes").and_then(|v| v.parse::<usize>().ok()) {
            return stream(out, n);
        }
        let start = monotonic_clock::now();
        if let Some(ms) = req.q("sleepMs").and_then(|v| v.parse::<u64>().ok()) {
            monotonic_clock::subscribe_duration(ms * 1_000_000).block();
        }
        let end = monotonic_clock::now();
        let status = req.q("status").and_then(|v| v.parse().ok()).unwrap_or(200);
        let ctx = invocation::context();
        let caller = match &ctx.caller {
            Caller::Platform => Json::obj([("kind", Json::str("platform"))]),
            Caller::Anonymous => Json::obj([("kind", Json::str("anonymous"))]),
            Caller::Principal(p) => Json::obj([
                ("kind", Json::str("principal")),
                ("id", Json::str(&p.id)),
                ("type", Json::str(&p.principal_type)),
                ("tier", Json::opt(p.tier.as_deref())),
                ("clients", Json::arr(p.clients.iter().map(|c| Json::str(c)))),
                ("roles", Json::arr(p.roles.iter().map(|c| Json::str(c)))),
                (
                    "applications",
                    Json::arr(p.applications.iter().map(|c| Json::str(c))),
                ),
                ("allApplications", Json::bool(p.all_applications)),
                (
                    "permissions",
                    Json::arr(p.permissions.iter().map(|c| Json::str(c))),
                ),
            ]),
        };
        let body = Json::obj([
            ("method", Json::str(&req.method)),
            ("pathWithQuery", Json::str(&req.path_with_query)),
            ("authority", Json::opt(req.authority.as_deref())),
            ("scheme", Json::opt(req.scheme.as_deref())),
            ("headers", headers_json(&req.headers)),
            ("body", Json::str(&String::from_utf8_lossy(&req.body))),
            ("bodyLength", Json::num(req.body.len())),
            ("calls", Json::num(calls)),
            ("startNanos", Json::num(start)),
            ("endNanos", Json::num(end)),
            ("invocationId", Json::str(&ctx.invocation_id)),
            ("address", Json::str(&ctx.address)),
            ("version", Json::num(ctx.version)),
            ("caller", caller),
            ("correlationId", Json::str(&ctx.correlation_id)),
            ("causationId", Json::opt(ctx.causation_id.as_deref())),
            ("originalHost", Json::opt(ctx.original_host.as_deref())),
            ("originalPath", Json::opt(ctx.original_path.as_deref())),
            ("remoteAddress", Json::opt(ctx.remote_address.as_deref())),
            (
                "pathParams",
                Json::map(
                    ctx.path_params
                        .iter()
                        .map(|(k, v)| (k.clone(), Json::str(v))),
                ),
            ),
        ]);
        common::respond(
            out,
            status,
            &[
                ("content-type", "application/json"),
                ("x-guest", "echo"),
                ("x-guest", "twice"),
            ],
            body.0.as_bytes(),
        );
    }
}

fn stream(out: ResponseOutparam, n: usize) {
    let response = OutgoingResponse::new(Fields::new());
    let body = response.body().expect("the body is taken once");
    ResponseOutparam::set(out, Ok(response));
    {
        let stream = body.write().expect("the stream is taken once");
        let chunk = [b'a'; 4096];
        let mut left = n;
        while left > 0 {
            let take = left.min(chunk.len());
            if stream.blocking_write_and_flush(&chunk[..take]).is_err() {
                return; // the host stopped reading
            }
            left -= take;
        }
    }
    let _ = OutgoingBody::finish(body, None);
}
