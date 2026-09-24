//! The `fc_emit_event` host function's JSON (Java
//! `fnhost/wasm/HostFunctions.java`, `extism:host/user`):
//!
//! - in: `{"type", "source"?, "subject"?, "dataContentType"?, "data",
//!   "correlationId"?, "causationId"?, "messageGroup"?, "dedupId"}`
//!   ([`OutboundEvent`]'s shape; `data` any JSON value);
//! - out: `{"ok":true}` or `{"ok":false,"error":"<code or why>"}`.

use super::jackson::{self, Node};
use crate::emit_error;
use crate::java::{is_blank, utf16_to_string};
use crate::OutboundEvent;

/// Reads a guest's emit input as Java's host does (`HostFunctions.emit`).
/// On failure, the error is the exact string the host answers with
/// ([`emit_error::INVALID_EVENT_NOT_JSON`],
/// [`emit_error::INVALID_EVENT_NOT_AN_OBJECT`],
/// [`emit_error::INVALID_EVENT_TYPE_REQUIRED`] or
/// [`emit_error::DEDUP_ID_REQUIRED`]).
///
/// Java's leniency is kept: a field of the wrong JSON type reads as absent
/// (`"source": 5` is no source; `"type": 5` is "type is required"), and
/// unknown keys are ignored. `data` is re-serialised as Jackson writes it
/// (compact, numbers normalised: `1e2` becomes `100.0`); absent or `null`
/// `data` is an empty payload.
pub fn decode_emit_input(json: &[u8]) -> Result<OutboundEvent, &'static str> {
    let node = match jackson::read_tree(json) {
        Err(_) => return Err(emit_error::INVALID_EVENT_NOT_JSON),
        Ok(Some(node @ Node::Obj(_))) => node,
        Ok(_) => return Err(emit_error::INVALID_EVENT_NOT_AN_OBJECT),
    };
    let text = |field: &str| match node.member(field) {
        Some(Node::Str(s)) => Some(utf16_to_string(s)),
        _ => None,
    };
    let event_type = text("type")
        .filter(|t| !is_blank(t))
        .ok_or(emit_error::INVALID_EVENT_TYPE_REQUIRED)?;
    let dedup_id = text("dedupId")
        .filter(|d| !is_blank(d))
        .ok_or(emit_error::DEDUP_ID_REQUIRED)?;
    let mut data = Vec::new();
    if let Some(value) = node.member("data").filter(|v| **v != Node::Null) {
        jackson::write_node(value, &mut data);
    }

    let mut event = OutboundEvent::new(event_type, dedup_id)
        .map_err(|_| emit_error::DEDUP_ID_REQUIRED)?
        .with_data(data);
    if let Some(v) = text("source") {
        event = event.with_source(v);
    }
    if let Some(v) = text("subject") {
        event = event.with_subject(v);
    }
    if let Some(v) = text("dataContentType") {
        event = event.with_data_content_type(v);
    }
    if let Some(v) = text("correlationId") {
        event = event.with_correlation_id(v);
    }
    if let Some(v) = text("causationId") {
        event = event.with_causation_id(v);
    }
    if let Some(v) = text("messageGroup") {
        event = event.with_message_group(v);
    }
    Ok(event)
}

/// Writes an event as a guest passes it to `fc_emit_event`, for the Rust
/// PDK. Optional fields are omitted when absent; `data` is written verbatim
/// (it must be JSON, or the host answers
/// [`emit_error::INVALID_EVENT_NOT_JSON`]) and as `null` when empty.
pub fn encode_emit_input(event: &OutboundEvent) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"{\"type\":");
    jackson::write_str(event.event_type(), &mut out);
    let optional = |out: &mut Vec<u8>, key: &str, value: Option<&str>| {
        if let Some(value) = value {
            out.extend_from_slice(format!(",\"{key}\":").as_bytes());
            jackson::write_str(value, out);
        }
    };
    optional(&mut out, "source", event.source());
    optional(&mut out, "subject", event.subject());
    optional(&mut out, "dataContentType", event.data_content_type());
    out.extend_from_slice(b",\"data\":");
    if event.data().is_empty() {
        out.extend_from_slice(b"null");
    } else {
        out.extend_from_slice(event.data());
    }
    optional(&mut out, "correlationId", event.correlation_id());
    optional(&mut out, "causationId", event.causation_id());
    optional(&mut out, "messageGroup", event.message_group());
    out.extend_from_slice(b",\"dedupId\":");
    jackson::write_str(event.dedup_id(), &mut out);
    out.push(b'}');
    out
}

/// The host's answer to an emit, as Java writes it: `{"ok":true}`, or
/// `{"ok":false,"error":…}` carrying the code (a platform refusal's
/// [`crate::EventEmitError::code`], [`emit_error::EMIT_FAILED`] for any other
/// failure, or a [`decode_emit_input`] error).
pub fn encode_emit_answer(outcome: Result<(), &str>) -> Vec<u8> {
    match outcome {
        Ok(()) => b"{\"ok\":true}".to_vec(),
        Err(error) => {
            let mut out = b"{\"ok\":false,\"error\":".to_vec();
            jackson::write_str(error, &mut out);
            out.push(b'}');
            out
        }
    }
}

/// The host's answer, read back by a guest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmitAnswer {
    /// `{"ok":true}`.
    Emitted,
    /// `{"ok":false,"error":…}`: the code or reason the host gave.
    Refused(String),
}

/// Reads the host's answer to an emit, for the Rust PDK. `None` when it is
/// not the answer shape.
pub fn decode_emit_answer(answer: &[u8]) -> Option<EmitAnswer> {
    let root = jackson::read_tree(answer).ok()??;
    match root.member("ok")? {
        Node::Bool(true) => Some(EmitAnswer::Emitted),
        Node::Bool(false) => match root.member("error") {
            Some(Node::Str(s)) => Some(EmitAnswer::Refused(utf16_to_string(s))),
            _ => Some(EmitAnswer::Refused(emit_error::EMIT_FAILED.to_string())),
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_the_guest_encoding() {
        let event = OutboundEvent::new("app:sub:agg:evt", "d-1")
            .unwrap()
            .with_source("src")
            .with_data(br#"{"b":1,"a":[true,null,"x"]}"#.to_vec())
            .with_message_group("grp");
        let json = encode_emit_input(&event);
        assert_eq!(
            std::str::from_utf8(&json).unwrap(),
            r#"{"type":"app:sub:agg:evt","source":"src","data":{"b":1,"a":[true,null,"x"]},"messageGroup":"grp","dedupId":"d-1"}"#
        );
        assert_eq!(decode_emit_input(&json).unwrap(), event);
    }

    #[test]
    fn answers() {
        assert_eq!(encode_emit_answer(Ok(())), br#"{"ok":true}"#);
        assert_eq!(
            encode_emit_answer(Err("EVENT_TYPE_NOT_OWNED")),
            br#"{"ok":false,"error":"EVENT_TYPE_NOT_OWNED"}"#
        );
        assert_eq!(
            decode_emit_answer(br#"{"ok":false,"error":"DEDUP_ID_REQUIRED"}"#),
            Some(EmitAnswer::Refused("DEDUP_ID_REQUIRED".into()))
        );
        assert_eq!(
            decode_emit_answer(br#"{"ok":true}"#),
            Some(EmitAnswer::Emitted)
        );
        assert_eq!(decode_emit_answer(b"[]"), None);
    }
}
