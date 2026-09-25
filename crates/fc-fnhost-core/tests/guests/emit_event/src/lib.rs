//! `emit_event`: publishes one event through `events.emit-event`
//! (`flowcatalyst:function@0.1.1`). `?dedupId=D` (absent: blank). Answers
//! `{"result":{"ok":true,"eventId":…}}` or
//! `{"result":{"ok":false,"error":…,"code":…,"status":…,"message":…}}`.

use common::{json, Json, Request};
use fc::events::{self, EmitEventError, OutboundEvent};
use wasip2::http::types::{IncomingRequest, ResponseOutparam};

struct EmitEvent;
wasip2::http::proxy::export!(EmitEvent);

impl wasip2::exports::http::incoming_handler::Guest for EmitEvent {
    fn handle(req: IncomingRequest, out: ResponseOutparam) {
        let req = Request::read(req);
        let event = OutboundEvent {
            type_: "fixture:guest:thing:happened".into(),
            source: None,
            subject: Some("thing-1".into()),
            data_content_type: None,
            data: Some(r#"{"from":"wasm"}"#.into()),
            correlation_id: None,
            causation_id: None,
            message_group: None,
            dedup_id: req.q("dedupId").unwrap_or_default(),
        };
        let result = match events::emit_event(&event) {
            Ok(id) => Json::obj([("ok", Json::bool(true)), ("eventId", Json::str(&id))]),
            Err(EmitEventError::Invalid(code)) => Json::obj([
                ("ok", Json::bool(false)),
                ("error", Json::str("invalid")),
                ("code", Json::str(&code)),
            ]),
            Err(EmitEventError::Refused(r)) => Json::obj([
                ("ok", Json::bool(false)),
                ("error", Json::str("refused")),
                ("code", Json::str(&r.code)),
                ("status", Json::num(r.status)),
                ("message", Json::str(&r.message)),
            ]),
            Err(EmitEventError::Unavailable(message)) => Json::obj([
                ("ok", Json::bool(false)),
                ("error", Json::str("unavailable")),
                ("message", Json::str(&message)),
            ]),
        };
        json(out, 200, &Json::obj([("result", result)]));
    }
}
