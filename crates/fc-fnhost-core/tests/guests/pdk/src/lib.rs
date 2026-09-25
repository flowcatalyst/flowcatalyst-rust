//! `pdk`: a guest written with `fc-function-pdk` (G1), for
//! `tests/wasm_pdk.rs`. Every PDK surface, one route each:
//!
//! - `/echo/{id}`: the request and the invocation as JSON;
//! - `/config?key=K`, `/secret?key=K`: `get` and `require`;
//! - `/emit?dedupId=D`: emits the JSON body as `fixture:pdk:thing:happened`;
//! - `/http?url=U[&body=B]`: one outbound `GET` (or `POST` of `B`);
//! - `/webhook`: the body parsed with `Webhook::event(&req)`;
//! - `/log?msg=M`: through `ctx.logger()` and the `log` crate;
//! - `/fail?msg=M`: an `Err` (generic body; the message is only logged);
//! - `/fail-explicit?msg=M`: `Response::fail(M)` (the message is the body);
//! - `/retry`: `Response::retry(1.5 s)`;
//! - `/clock`: `ctx.now()` as epoch milliseconds.

use std::time::{Duration, UNIX_EPOCH};

use fc_function_pdk::prelude::*;
use serde_json::{json as j, Value};

#[handler]
async fn handle(req: Request, ctx: Context) -> Result<Response, Error> {
    let first = req.path().split('/').nth(1).unwrap_or("");
    match first {
        "echo" => echo(&req, &ctx),
        "config" => {
            let key = req.query_param("key").unwrap_or("");
            json(
                200,
                &j!({
                    "value": ctx.config().get(key),
                    "require": ctx.config().require(key).map_err(|e| e.to_string()),
                }),
            )
        }
        "secret" => {
            let key = req.query_param("key").unwrap_or("");
            json(200, &j!({"value": ctx.secrets().get(key)}))
        }
        "emit" => {
            let data: Value = req.json()?;
            let event = OutboundEvent::new(
                "fixture:pdk:thing:happened",
                req.query_param("dedupId").unwrap_or("x"),
            )?
            .with_subject("thing-1")
            .with_json(&data)?;
            let result = match ctx.events().emit(&event) {
                Ok(()) => j!({"ok": true}),
                Err(e) => j!({
                    "ok": false,
                    "code": e.code(),
                    "status": e.status(),
                    "retryable": e.is_retryable(),
                    "message": e.to_string(),
                }),
            };
            json(200, &result)
        }
        "http" => {
            let url = req.query_param("url").unwrap_or("");
            let call = match req.query_param("body") {
                Some(body) => HttpCall::post(url).with_body(body),
                None => HttpCall::get(url),
            };
            let result = match ctx.http().send(call).await {
                Ok(reply) => j!({
                    "status": reply.status(),
                    "upstream": reply.header("X-Upstream"),
                    "body": reply.text()?,
                }),
                Err(HttpError::Denied(denied)) => j!({
                    "denied": denied.host(),
                    "message": denied.to_string(),
                }),
                Err(other) => j!({"error": other.to_string()}),
            };
            json(200, &result)
        }
        "webhook" => {
            let event = Webhook::event(&req)?;
            json(
                200,
                &j!({
                    "id": event.id,
                    "type": event.event_type,
                    "attempt": event.attempt_number,
                    "data": event.data_json,
                    "caller": caller(req.caller()),
                }),
            )
        }
        "log" => {
            let msg = req.query_param("msg").unwrap_or("hello");
            ctx.logger().info(format_args!("pdk logger: {msg}"));
            log::warn!("pdk log crate: {msg}");
            Ok(Response::ack())
        }
        "fail" => Err(Error::msg(req.query_param("msg").unwrap_or("boom"))),
        "fail-explicit" => Ok(Response::fail(req.query_param("msg").unwrap_or("boom"))?),
        "retry" => Ok(Response::retry(Duration::from_millis(1500))),
        "clock" => {
            let ms = ctx.now().duration_since(UNIX_EPOCH)?.as_millis() as u64;
            json(200, &j!({"epochMs": ms}))
        }
        _ => Ok(Response::json(404, r#"{"error":"no such route"}"#)?),
    }
}

fn echo(req: &Request, ctx: &Context) -> Result<Response, Error> {
    let invocation = ctx.invocation();
    let body: Value = if req.body().is_empty() {
        Value::Null
    } else {
        req.json()?
    };
    json(
        200,
        &j!({
            "method": req.method(),
            "path": req.path(),
            "rawQuery": req.raw_query(),
            "query": req.query(),
            "header": req.header("X-TEST-CUSTOM"),
            "headerAll": req.header_all("x-multi"),
            "authority": req.authority(),
            "body": body,
            "id": req.path_param("id"),
            "invocationId": invocation.invocation_id,
            "address": ctx.address().render(),
            "version": ctx.version(),
            "caller": caller(&invocation.caller),
            "correlationId": invocation.correlation_id,
            "causationId": invocation.causation_id,
            "originalPath": invocation.original_path,
            "remoteAddress": invocation.remote_address,
        }),
    )
}

fn caller(caller: &Caller) -> Value {
    match caller {
        Caller::Platform => j!({"kind": "platform"}),
        Caller::Anonymous => j!({"kind": "anonymous"}),
        Caller::Principal(p) => j!({"kind": "principal", "id": p.id}),
    }
}
