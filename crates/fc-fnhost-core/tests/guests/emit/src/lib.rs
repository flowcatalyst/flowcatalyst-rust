//! `emit`: publishes one event through `events.emit`. `?dedupId=D` (absent:
//! blank), `&type=T` (default `fixture:guest:thing:happened`),
//! `&data=raw` (default `{"from":"wasm"}`), `&correlationId=C`. Answers
//! `{"result":{"ok":true}}` or `{"result":{"ok":false,"error":…,"code":…,"status":…}}`.

use common::{json, Json, Request};
use fc::events::{self, EmitError, OutboundEvent};
use wasip2::http::types::{IncomingRequest, ResponseOutparam};

struct Emit;
wasip2::http::proxy::export!(Emit);

impl wasip2::exports::http::incoming_handler::Guest for Emit {
    fn handle(req: IncomingRequest, out: ResponseOutparam) {
        let req = Request::read(req);
        let event = OutboundEvent {
            type_: req
                .q("type")
                .unwrap_or_else(|| "fixture:guest:thing:happened".into()),
            source: None,
            subject: Some("thing-1".into()),
            data_content_type: None,
            data: Some(req.q("data").unwrap_or_else(|| r#"{"from":"wasm"}"#.into())),
            correlation_id: req.q("correlationId"),
            causation_id: None,
            message_group: Some("group-1".into()),
            dedup_id: req.q("dedupId").unwrap_or_default(),
        };
        let result = match events::emit(&event) {
            Ok(()) => Json::obj([("ok", Json::bool(true))]),
            Err(EmitError::Invalid(code)) => Json::obj([
                ("ok", Json::bool(false)),
                ("error", Json::str("invalid")),
                ("code", Json::str(&code)),
            ]),
            Err(EmitError::Refused(r)) => Json::obj([
                ("ok", Json::bool(false)),
                ("error", Json::str("refused")),
                ("code", Json::str(&r.code)),
                ("status", Json::num(r.status)),
            ]),
            Err(EmitError::Unavailable) => Json::obj([
                ("ok", Json::bool(false)),
                ("error", Json::str("unavailable")),
            ]),
        };
        json(out, 200, &Json::obj([("result", result)]));
    }
}
