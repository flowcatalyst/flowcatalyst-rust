use crate::{java, InvalidArgument};

/// An event a function emits through its host. Mirrors Java
/// `function-api/src/main/java/io/flowcatalyst/function/OutboundEvent.java`.
///
/// `dedup_id` is required and never blank (Java's constructor invariant);
/// everything but `event_type` and `dedup_id` is optional. `data` is the
/// event payload's bytes (JSON on every wire this crate knows), empty when
/// there is none.
#[derive(Clone, PartialEq, Eq)]
pub struct OutboundEvent {
    event_type: String,
    source: Option<String>,
    subject: Option<String>,
    data_content_type: Option<String>,
    data: Vec<u8>,
    correlation_id: Option<String>,
    causation_id: Option<String>,
    message_group: Option<String>,
    dedup_id: String,
}

impl OutboundEvent {
    /// An event of `event_type` deduplicated on `dedup_id`, with no payload and
    /// no optional fields. Errors when `dedup_id` is blank (Java's
    /// `String.isBlank`: "dedupId must not be blank").
    pub fn new(
        event_type: impl Into<String>,
        dedup_id: impl Into<String>,
    ) -> Result<Self, InvalidArgument> {
        let dedup_id = dedup_id.into();
        if java::is_blank(&dedup_id) {
            return Err(InvalidArgument("dedupId must not be blank".into()));
        }
        Ok(Self {
            event_type: event_type.into(),
            source: None,
            subject: None,
            data_content_type: None,
            data: Vec::new(),
            correlation_id: None,
            causation_id: None,
            message_group: None,
            dedup_id,
        })
    }

    /// Sets the CloudEvents `source`.
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// Sets the CloudEvents `subject`.
    pub fn with_subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    /// Sets the media type of `data`.
    pub fn with_data_content_type(mut self, data_content_type: impl Into<String>) -> Self {
        self.data_content_type = Some(data_content_type.into());
        self
    }

    /// Sets the payload bytes.
    pub fn with_data(mut self, data: impl Into<Vec<u8>>) -> Self {
        self.data = data.into();
        self
    }

    /// Links this event to the request or flow that caused it.
    pub fn with_correlation_id(mut self, correlation_id: impl Into<String>) -> Self {
        self.correlation_id = Some(correlation_id.into());
        self
    }

    /// The id of the event that directly caused this one.
    pub fn with_causation_id(mut self, causation_id: impl Into<String>) -> Self {
        self.causation_id = Some(causation_id.into());
        self
    }

    /// The ordering group this event belongs to.
    pub fn with_message_group(mut self, message_group: impl Into<String>) -> Self {
        self.message_group = Some(message_group.into());
        self
    }

    /// The CloudEvents `type`.
    pub fn event_type(&self) -> &str {
        &self.event_type
    }

    /// The CloudEvents `source`.
    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    /// The CloudEvents `subject`.
    pub fn subject(&self) -> Option<&str> {
        self.subject.as_deref()
    }

    /// The media type of `data`.
    pub fn data_content_type(&self) -> Option<&str> {
        self.data_content_type.as_deref()
    }

    /// The payload bytes; empty when there is no payload.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// The correlation id.
    pub fn correlation_id(&self) -> Option<&str> {
        self.correlation_id.as_deref()
    }

    /// The causation id.
    pub fn causation_id(&self) -> Option<&str> {
        self.causation_id.as_deref()
    }

    /// The message group.
    pub fn message_group(&self) -> Option<&str> {
        self.message_group.as_deref()
    }

    /// The id the platform deduplicates this event on; never blank.
    pub fn dedup_id(&self) -> &str {
        &self.dedup_id
    }
}

impl std::fmt::Debug for OutboundEvent {
    /// The payload's length, never its bytes, as Java's `toString`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OutboundEvent")
            .field("event_type", &self.event_type)
            .field("source", &self.source)
            .field("subject", &self.subject)
            .field("data_content_type", &self.data_content_type)
            .field("data.length", &self.data.len())
            .field("correlation_id", &self.correlation_id)
            .field("causation_id", &self.causation_id)
            .field("message_group", &self.message_group)
            .field("dedup_id", &self.dedup_id)
            .finish()
    }
}

/// The host could not emit an event on the function's behalf. Mirrors Java
/// `function-api/src/main/java/io/flowcatalyst/function/EventEmitException.java`:
/// the platform's own error `code` and HTTP `status` verbatim for a refusal
/// from `POST /control/functions/events`, or [`emit_error::UNAVAILABLE`] /
/// `503` when the platform could not be reached. A function can retry on a
/// 5xx and fail loudly on [`emit_error::EVENT_TYPE_NOT_OWNED`].
///
/// `message` (Java 67b04a51) is the reason: the platform's own `message`
/// for a refusal (which check refused the emit), or what failed for a
/// transport failure; empty when there is none.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("emit refused: {code} ({status}){}", if message.is_empty() { String::new() } else { format!(": {message}") })]
pub struct EventEmitError {
    code: String,
    status: u16,
    message: String,
}

impl EventEmitError {
    /// A refusal with the platform's code and status.
    pub fn new(code: impl Into<String>, status: u16) -> Self {
        Self {
            code: code.into(),
            status,
            message: String::new(),
        }
    }

    /// This refusal, with its reason.
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }

    /// Why: the platform's own message, or what failed to reach it; empty
    /// when there is none.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// A transport failure: [`emit_error::UNAVAILABLE`], `503`.
    pub fn unavailable() -> Self {
        Self::new(emit_error::UNAVAILABLE, 503)
    }

    /// The platform's error code, e.g. `EVENT_TYPE_NOT_OWNED`.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// The HTTP status the platform answered, or `503` for a transport failure.
    pub fn status(&self) -> u16 {
        self.status
    }
}

/// The error strings a guest can see from an emit, as Java produces them.
///
/// The first group is what Java's host (`fnhost/wasm/HostFunctions.java`
/// `emit`) answers itself before or instead of calling the platform, verbatim
/// (the `INVALID_EVENT` ones carry a reason after the code). The second group
/// is the platform's own codes, which the host relays unchanged
/// ([`EventEmitError::code`]); the list is the one Java's
/// `EventEmitException` documents, not a closed set.
pub mod emit_error {
    /// The emit input was not JSON.
    pub const INVALID_EVENT_NOT_JSON: &str = "INVALID_EVENT: not JSON";
    /// The emit input was JSON but not an object.
    pub const INVALID_EVENT_NOT_AN_OBJECT: &str = "INVALID_EVENT: not a JSON object";
    /// `type` was missing, not a string, or blank.
    pub const INVALID_EVENT_TYPE_REQUIRED: &str = "INVALID_EVENT: type is required";
    /// `dedupId` was missing, not a string, or blank.
    pub const DEDUP_ID_REQUIRED: &str = "DEDUP_ID_REQUIRED";
    /// The event's `data` was not JSON. The component host's own code (its
    /// `events.emit` takes `data` as JSON text); Java's host reads the whole
    /// event as JSON, so its nearest code is [`INVALID_EVENT_NOT_JSON`].
    pub const INVALID_EVENT_DATA_NOT_JSON: &str = "INVALID_EVENT: data is not JSON";
    /// The host's emit failed for a reason that is not a platform refusal.
    pub const EMIT_FAILED: &str = "EMIT_FAILED";
    /// The platform could not be reached (status 503).
    pub const UNAVAILABLE: &str = "UNAVAILABLE";

    /// The calling host is not registered with the platform.
    pub const HOST_UNKNOWN: &str = "HOST_UNKNOWN";
    /// The host does not serve the emitting function.
    pub const FUNCTION_NOT_SERVED_BY_HOST: &str = "FUNCTION_NOT_SERVED_BY_HOST";
    /// The dedup id was already used.
    pub const DEDUP_ID_DUPLICATE: &str = "DEDUP_ID_DUPLICATE";
    /// The function's application does not own the event type.
    pub const EVENT_TYPE_NOT_OWNED: &str = "EVENT_TYPE_NOT_OWNED";
}

#[cfg(test)]
mod tests {
    use super::*;

    // Java OutboundEventTest.dedupIdIsRequiredAndMayNotBeBlank (the null case
    // is a compile-time fact in Rust).
    #[test]
    fn dedup_id_is_required_and_may_not_be_blank() {
        assert_eq!(
            OutboundEvent::new("app:sub:agg:evt", "")
                .unwrap_err()
                .message(),
            "dedupId must not be blank"
        );
        assert!(OutboundEvent::new("app:sub:agg:evt", "   ").is_err());
        assert!(OutboundEvent::new("app:sub:agg:evt", "\u{000B}").is_err());
    }

    // Java OutboundEventTest.aNonBlankDedupIdIsAccepted
    #[test]
    fn a_non_blank_dedup_id_is_accepted() {
        let e = OutboundEvent::new("app:sub:agg:evt", "dedup-1")
            .unwrap()
            .with_source("src")
            .with_subject("subj")
            .with_data_content_type("application/json")
            .with_data(b"{}".to_vec());
        assert_eq!(e.dedup_id(), "dedup-1");
        assert_eq!(e.source(), Some("src"));
        assert_eq!(e.data(), b"{}");
        assert_eq!(e.correlation_id(), None);
        assert!(format!("{e:?}").contains("data.length: 2"));
    }

    #[test]
    fn emit_error_message_matches_java() {
        assert_eq!(
            EventEmitError::new("EVENT_TYPE_NOT_OWNED", 403).to_string(),
            "emit refused: EVENT_TYPE_NOT_OWNED (403)"
        );
        assert_eq!(EventEmitError::unavailable().status(), 503);
        // Java 67b04a51: the platform's reason joins the text.
        let refused = EventEmitError::new("EVENT_TYPE_NOT_OWNED", 403).with_message("nope");
        assert_eq!(refused.message(), "nope");
        assert_eq!(
            refused.to_string(),
            "emit refused: EVENT_TYPE_NOT_OWNED (403): nope"
        );
    }
}
