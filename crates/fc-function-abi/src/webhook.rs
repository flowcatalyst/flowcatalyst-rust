use serde::Deserialize;
use serde_json::value::RawValue;

use crate::Timestamp;

/// Parses a `webhook` endpoint's request body into the envelope the platform
/// sent. Mirrors Java `function-api/src/main/java/io/flowcatalyst/function/Webhook.java`
/// in what it accepts from the platform: the body is a JSON object (UTF-8,
/// RFC 8259), unknown members are ignored, optional members may be absent or
/// `null`, integers must be integral and fit an `i32`, and `data` / `payload`
/// come back as their raw JSON text, untouched.
pub struct Webhook;

/// The body is not valid JSON, or not shaped like the envelope being parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct WebhookFormatError(pub(crate) String);

/// A subscription or direct dispatch job's delivery envelope (Java
/// `function-api/.../Event.java`), from the platform's `DeliveryPayload.build`:
/// `{id, type, attemptNumber, source?, subject?, correlationId?,
/// messageGroup?, clientId?, clientCode?, data?}`.
///
/// Only a **non-`dataOnly`** subscription sends this envelope; a `dataOnly`
/// one sends the payload itself as the whole body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// The dispatch job's own id.
    pub id: String,
    /// The event type / dispatch job code.
    pub event_type: String,
    /// This delivery attempt, 1-based.
    pub attempt_number: i32,
    /// The originating event's `source`.
    pub source: Option<String>,
    /// The originating event's `subject`.
    pub subject: Option<String>,
    /// Links this delivery to the flow that caused it.
    pub correlation_id: Option<String>,
    /// The ordering group this delivery belongs to.
    pub message_group: Option<String>,
    /// The owning client's id; `None` for a platform-scoped job.
    pub client_id: Option<String>,
    /// The owning client's code; `None` under the same conditions.
    pub client_code: Option<String>,
    /// The raw JSON text of `data` exactly as it appeared on the wire
    /// (whitespace and escapes included), `None` when absent or `null`.
    pub data_json: Option<String>,
}

/// A scheduled job firing's envelope (Java `function-api/.../Schedule.java`),
/// from the platform's `JobDispatcher.WebhookEnvelope`:
/// `{jobId, jobCode, instanceId, scheduledFor?, firedAt, triggerKind,
/// correlationId?, payload?, tracksCompletion, timeoutSeconds?, concurrent}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    /// The scheduled job's own id.
    pub job_id: String,
    /// The scheduled job's code.
    pub job_code: String,
    /// This firing's instance id.
    pub instance_id: String,
    /// When this firing was due.
    pub scheduled_for: Option<Timestamp>,
    /// When this firing actually ran.
    pub fired_at: Timestamp,
    /// `CRON`, `MANUAL` or `BACKFILL`, as the raw wire string. Kept a string,
    /// not a strict enum: Java's API carries it raw ("this jar cannot depend
    /// on the server's `TriggerKind` enum") and accepts any value, so a new
    /// trigger kind never breaks a deployed function.
    pub trigger_kind: String,
    /// Links this firing to the flow that caused it.
    pub correlation_id: Option<String>,
    /// The raw JSON text of `payload`, `None` when absent or `null`.
    pub payload_json: Option<String>,
    /// Whether the scheduler expects this firing to report completion.
    pub tracks_completion: bool,
    /// The firing's own timeout, `None` when the job has none.
    pub timeout_seconds: Option<i32>,
    /// Whether this job allows overlapping firings.
    pub concurrent: bool,
}

/// The event envelope on the wire.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EventWire {
    id: String,
    #[serde(rename = "type")]
    event_type: String,
    attempt_number: i32,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    correlation_id: Option<String>,
    #[serde(default)]
    message_group: Option<String>,
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    client_code: Option<String>,
    #[serde(default)]
    data: Option<Box<RawValue>>,
}

/// The schedule envelope on the wire.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScheduleWire {
    job_id: String,
    job_code: String,
    instance_id: String,
    #[serde(default)]
    scheduled_for: Option<String>,
    fired_at: String,
    trigger_kind: String,
    #[serde(default)]
    correlation_id: Option<String>,
    #[serde(default)]
    payload: Option<Box<RawValue>>,
    tracks_completion: bool,
    #[serde(default)]
    timeout_seconds: Option<i32>,
    concurrent: bool,
}

impl Webhook {
    /// Parses a subscription / direct dispatch job delivery body: the bytes
    /// themselves, or anything that holds them (the guest PDK's request).
    pub fn event(body: impl AsRef<[u8]>) -> Result<Event, WebhookFormatError> {
        let w: EventWire = parse_object(body.as_ref())?;
        Ok(Event {
            id: w.id,
            event_type: w.event_type,
            attempt_number: w.attempt_number,
            source: w.source,
            subject: w.subject,
            correlation_id: w.correlation_id,
            message_group: w.message_group,
            client_id: w.client_id,
            client_code: w.client_code,
            data_json: w.data.map(|raw| raw.get().to_owned()),
        })
    }

    /// Parses a scheduled job firing body (bytes, or anything holding them).
    pub fn schedule(body: impl AsRef<[u8]>) -> Result<Schedule, WebhookFormatError> {
        let w: ScheduleWire = parse_object(body.as_ref())?;
        Ok(Schedule {
            job_id: w.job_id,
            job_code: w.job_code,
            instance_id: w.instance_id,
            scheduled_for: w
                .scheduled_for
                .map(|raw| parse_instant("scheduledFor", &raw))
                .transpose()?,
            fired_at: parse_instant("firedAt", &w.fired_at)?,
            trigger_kind: w.trigger_kind,
            correlation_id: w.correlation_id,
            payload_json: w.payload.map(|raw| raw.get().to_owned()),
            tracks_completion: w.tracks_completion,
            timeout_seconds: w.timeout_seconds,
            concurrent: w.concurrent,
        })
    }
}

/// The body as `T`, which must come from a JSON object: serde would also
/// read a struct from an array, positionally, and Java does not.
fn parse_object<'de, T: Deserialize<'de>>(body: &'de [u8]) -> Result<T, WebhookFormatError> {
    if body.trim_ascii_start().first() != Some(&b'{') {
        return Err(WebhookFormatError(
            "webhook body must be a JSON object".into(),
        ));
    }
    serde_json::from_slice(body).map_err(|e| WebhookFormatError(e.to_string()))
}

fn parse_instant(key: &str, raw: &str) -> Result<Timestamp, WebhookFormatError> {
    Timestamp::parse(raw)
        .ok_or_else(|| WebhookFormatError(format!("field '{key}' is not a valid timestamp: {raw}")))
}

/// Java `function-api/src/test/java/io/flowcatalyst/function/WebhookTest.java`
/// and `EventTest.java`. The golden tables (`tests/java_golden.rs`) hold the
/// rest of Java's answers.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_full_event_envelope() {
        let body = br#"{"id":"djb_123","type":"billing:invoices:invoice:created","attemptNumber":2,
                 "source":"billing-service","subject":"inv_1","correlationId":"corr-1",
                 "messageGroup":"group-1","clientId":"cli_1","clientCode":"acme",
                 "data":{"invoiceId":"inv_1","amount":100}}"#;
        let e = Webhook::event(body).unwrap();
        assert_eq!(e.id, "djb_123");
        assert_eq!(e.event_type, "billing:invoices:invoice:created");
        assert_eq!(e.attempt_number, 2);
        assert_eq!(e.source.as_deref(), Some("billing-service"));
        assert_eq!(e.subject.as_deref(), Some("inv_1"));
        assert_eq!(e.correlation_id.as_deref(), Some("corr-1"));
        assert_eq!(e.message_group.as_deref(), Some("group-1"));
        assert_eq!(e.client_id.as_deref(), Some("cli_1"));
        assert_eq!(e.client_code.as_deref(), Some("acme"));
        assert_eq!(
            e.data_json.as_deref(),
            Some(r#"{"invoiceId":"inv_1","amount":100}"#)
        );
    }

    #[test]
    fn every_optional_event_field_may_be_absent_entirely() {
        let e = Webhook::event(br#"{"id":"djb_1","type":"t","attemptNumber":1}"#).unwrap();
        assert_eq!(
            e,
            Event {
                id: "djb_1".into(),
                event_type: "t".into(),
                attempt_number: 1,
                source: None,
                subject: None,
                correlation_id: None,
                message_group: None,
                client_id: None,
                client_code: None,
                data_json: None,
            }
        );
    }

    #[test]
    fn non_json_payload_data_passes_through_as_a_literal_json_string() {
        let e =
            Webhook::event(br#"{"id":"djb_1","type":"t","attemptNumber":1,"data":"not json {"}"#)
                .unwrap();
        assert_eq!(e.data_json.as_deref(), Some(r#""not json {""#));
    }

    #[test]
    fn event_requires_id_and_type() {
        assert!(Webhook::event(br#"{"type":"t","attemptNumber":1}"#).is_err());
        assert!(Webhook::event(br#"{"id":"i","attemptNumber":1}"#).is_err());
        assert!(Webhook::event(br#"{"id":"i","type":"t"}"#).is_err());
    }

    #[test]
    fn event_body_that_is_not_a_json_object_is_rejected() {
        assert!(Webhook::event(b"[1,2,3]").is_err());
        assert!(Webhook::event(b"not json").is_err());
        assert!(Webhook::event(b"").is_err());
    }

    #[test]
    fn parses_a_full_schedule_envelope() {
        let body = br#"{"jobId":"sjb_1","jobCode":"nightly-report","instanceId":"sji_1",
                 "scheduledFor":"2026-09-19T00:00:00.000000Z","firedAt":"2026-09-19T00:00:01.500000Z",
                 "triggerKind":"CRON","correlationId":"corr-9","payload":{"n":1},
                 "tracksCompletion":true,"timeoutSeconds":30,"concurrent":false}"#;
        let s = Webhook::schedule(body).unwrap();
        assert_eq!(s.job_id, "sjb_1");
        assert_eq!(s.job_code, "nightly-report");
        assert_eq!(s.instance_id, "sji_1");
        assert_eq!(
            s.scheduled_for,
            Timestamp::parse("2026-09-19T00:00:00.000000Z")
        );
        assert_eq!(
            Some(s.fired_at),
            Timestamp::parse("2026-09-19T00:00:01.500000Z")
        );
        assert_eq!(s.trigger_kind, "CRON");
        assert_eq!(s.correlation_id.as_deref(), Some("corr-9"));
        assert_eq!(s.payload_json.as_deref(), Some(r#"{"n":1}"#));
        assert!(s.tracks_completion);
        assert_eq!(s.timeout_seconds, Some(30));
        assert!(!s.concurrent);
    }

    #[test]
    fn every_optional_schedule_field_may_be_absent_entirely() {
        let s = Webhook::schedule(
            br#"{"jobId":"sjb_1","jobCode":"nightly-report","instanceId":"sji_1",
                 "firedAt":"2026-09-19T00:00:01Z","triggerKind":"MANUAL",
                 "tracksCompletion":false,"concurrent":true}"#,
        )
        .unwrap();
        assert_eq!(s.scheduled_for, None);
        assert_eq!(s.correlation_id, None);
        assert_eq!(s.payload_json, None);
        assert_eq!(s.timeout_seconds, None);
    }

    #[test]
    fn schedule_requires_its_mandatory_fields() {
        assert!(Webhook::schedule(
            br#"{"jobCode":"c","instanceId":"i","firedAt":"2026-09-19T00:00:00Z","triggerKind":"CRON","tracksCompletion":true,"concurrent":false}"#
        )
        .is_err());
        assert!(Webhook::schedule(
            br#"{"jobId":"j","jobCode":"c","instanceId":"i","triggerKind":"CRON","tracksCompletion":true,"concurrent":false}"#
        )
        .is_err());
    }

    #[test]
    fn schedule_rejects_a_malformed_timestamp() {
        let e = Webhook::schedule(
            br#"{"jobId":"j","jobCode":"c","instanceId":"i","firedAt":"not-a-time","triggerKind":"CRON","tracksCompletion":true,"concurrent":false}"#,
        )
        .unwrap_err();
        assert_eq!(
            e.to_string(),
            "field 'firedAt' is not a valid timestamp: not-a-time"
        );
    }
}
