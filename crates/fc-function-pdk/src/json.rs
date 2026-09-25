//! serde helpers (feature `json`).

use fc_function_abi::{OutboundEvent, Response};

use crate::Error;

/// `value` as a JSON response with `status` (Java's `Result.json`, with the
/// serialization done for you).
pub fn json<T: serde::Serialize + ?Sized>(status: u16, value: &T) -> Result<Response, Error> {
    Ok(Response::json(status, serde_json::to_string(value)?)?)
}

/// JSON payloads for [`OutboundEvent`].
pub trait OutboundEventExt: Sized {
    /// `value`, serialized, as the event's payload.
    fn with_json<T: serde::Serialize + ?Sized>(self, value: &T) -> Result<Self, serde_json::Error>;
}

impl OutboundEventExt for OutboundEvent {
    fn with_json<T: serde::Serialize + ?Sized>(self, value: &T) -> Result<Self, serde_json::Error> {
        Ok(self.with_data(serde_json::to_vec(value)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_serializes_with_the_content_type() {
        let r = json(201, &serde_json::json!({"a": [1, 2]})).unwrap();
        assert_eq!(r.status(), 201);
        assert_eq!(r.body(), br#"{"a":[1,2]}"#);
        assert_eq!(r.headers()["Content-Type"], ["application/json"]);
        assert!(json(600, &1).is_err());
    }

    #[test]
    fn an_event_takes_a_json_payload() {
        let e = OutboundEvent::new("a:b:c:d", "d-1")
            .unwrap()
            .with_json(&serde_json::json!({"n": 1}))
            .unwrap();
        assert_eq!(e.data(), br#"{"n":1}"#);
    }
}
