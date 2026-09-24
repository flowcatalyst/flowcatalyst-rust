//! Outbox Builder DTOs
//!
//! Lightweight builder types for creating outbox messages without
//! implementing the full `DomainEvent` trait. These match the TS and
//! Laravel SDK patterns.
//!
//! # Examples
//!
//! ```ignore
//! use fc_sdk::outbox::{CreateEventDto, CreateDispatchJobDto, CreateAuditLogDto};
//!
//! // Create an event
//! let event = CreateEventDto::new("user.registered", serde_json::json!({"userId": "123"}))
//!     .source("user-service")
//!     .subject("users.user.123")
//!     .correlation_id("corr-456")
//!     .message_group("users:user:123");
//!
//! // Create a dispatch job
//! let job = CreateDispatchJobDto::new(
//!     "user-service",
//!     "user.registered",
//!     "https://webhook.example.com/users",
//!     r#"{"userId":"123"}"#,
//!     "pool_0HZXEQ5Y8JY5Z",
//! ).correlation_id("corr-456")
//!  .timeout_seconds(60);
//!
//! // Create an audit log
//! let audit = CreateAuditLogDto::new("User", "usr_123", "CREATE")
//!     .operation_data(serde_json::json!({"email": "user@example.com"}))
//!     .principal_id("usr_456")
//!     .source("user-service");
//! ```

use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::HashMap;

// ─── CreateEventDto ─────────────────────────────────────────────────────────

/// Builder for creating an event in the outbox.
#[derive(Debug, Clone)]
pub struct CreateEventDto {
    pub event_type: String,
    pub data: serde_json::Value,
    pub source: Option<String>,
    pub subject: Option<String>,
    pub correlation_id: Option<String>,
    pub causation_id: Option<String>,
    pub deduplication_id: Option<String>,
    pub message_group: Option<String>,
    pub context_data: Vec<ContextDataEntry>,
    pub headers: HashMap<String, String>,
}

/// Key-value context data entry attached to events.
#[derive(Debug, Clone, Serialize)]
pub struct ContextDataEntry {
    pub key: String,
    pub value: String,
}

impl CreateEventDto {
    pub fn new(event_type: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            event_type: event_type.into(),
            data,
            source: None,
            subject: None,
            correlation_id: None,
            causation_id: None,
            deduplication_id: None,
            message_group: None,
            context_data: Vec::new(),
            headers: HashMap::new(),
        }
    }

    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    pub fn subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    pub fn correlation_id(mut self, id: impl Into<String>) -> Self {
        self.correlation_id = Some(id.into());
        self
    }

    pub fn causation_id(mut self, id: impl Into<String>) -> Self {
        self.causation_id = Some(id.into());
        self
    }

    pub fn deduplication_id(mut self, id: impl Into<String>) -> Self {
        self.deduplication_id = Some(id.into());
        self
    }

    pub fn message_group(mut self, group: impl Into<String>) -> Self {
        self.message_group = Some(group.into());
        self
    }

    pub fn context_data(mut self, entries: Vec<ContextDataEntry>) -> Self {
        self.context_data.extend(entries);
        self
    }

    pub fn add_context(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.context_data.push(ContextDataEntry {
            key: key.into(),
            value: value.into(),
        });
        self
    }

    pub fn headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers.extend(headers);
        self
    }

    /// Build the event payload JSON for the outbox.
    pub fn to_payload(&self) -> serde_json::Value {
        let mut payload = serde_json::json!({
            "specVersion": "1.0",
            "type": self.event_type,
            // `Value`'s Display is its compact JSON encoding and cannot fail.
            "data": self.data.to_string(),
        });

        let obj = payload.as_object_mut().unwrap();
        if let Some(ref v) = self.source {
            obj.insert("source".into(), serde_json::json!(v));
        }
        if let Some(ref v) = self.subject {
            obj.insert("subject".into(), serde_json::json!(v));
        }
        if let Some(ref v) = self.correlation_id {
            obj.insert("correlationId".into(), serde_json::json!(v));
        }
        if let Some(ref v) = self.causation_id {
            obj.insert("causationId".into(), serde_json::json!(v));
        }
        if let Some(ref v) = self.deduplication_id {
            obj.insert("deduplicationId".into(), serde_json::json!(v));
        }
        if let Some(ref v) = self.message_group {
            obj.insert("messageGroup".into(), serde_json::json!(v));
        }
        if !self.context_data.is_empty() {
            obj.insert("contextData".into(), serde_json::json!(self.context_data));
        }

        payload
    }
}

// ─── CreateDispatchJobDto ───────────────────────────────────────────────────

/// Builder for creating a dispatch job in the outbox.
#[derive(Debug, Clone)]
pub struct CreateDispatchJobDto {
    pub source: String,
    pub code: String,
    pub target_url: String,
    pub payload: String,
    pub dispatch_pool_id: String,
    pub subject: Option<String>,
    pub correlation_id: Option<String>,
    pub event_id: Option<String>,
    pub metadata: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub payload_content_type: String,
    pub data_only: bool,
    pub message_group: Option<String>,
    pub sequence: Option<i64>,
    pub timeout_seconds: u32,
    pub max_retries: u32,
    pub retry_strategy: Option<String>,
    pub scheduled_for: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub idempotency_key: Option<String>,
    pub external_id: Option<String>,
    pub connection_id: Option<String>,
}

impl CreateDispatchJobDto {
    pub fn new(
        source: impl Into<String>,
        code: impl Into<String>,
        target_url: impl Into<String>,
        payload: impl Into<String>,
        dispatch_pool_id: impl Into<String>,
    ) -> Self {
        Self {
            source: source.into(),
            code: code.into(),
            target_url: target_url.into(),
            payload: payload.into(),
            dispatch_pool_id: dispatch_pool_id.into(),
            subject: None,
            correlation_id: None,
            event_id: None,
            metadata: HashMap::new(),
            headers: HashMap::new(),
            payload_content_type: "application/json".to_string(),
            data_only: true,
            message_group: None,
            sequence: None,
            timeout_seconds: 30,
            max_retries: 5,
            retry_strategy: None,
            scheduled_for: None,
            expires_at: None,
            idempotency_key: None,
            external_id: None,
            connection_id: None,
        }
    }

    /// Create from a JSON-serializable payload (auto-stringifies).
    ///
    /// Fails if `payload` cannot be serialized, rather than sending `{}`.
    pub fn from_json(
        source: impl Into<String>,
        code: impl Into<String>,
        target_url: impl Into<String>,
        payload: &impl Serialize,
        dispatch_pool_id: impl Into<String>,
    ) -> Result<Self, serde_json::Error> {
        Ok(Self::new(
            source,
            code,
            target_url,
            serde_json::to_string(payload)?,
            dispatch_pool_id,
        ))
    }

    pub fn subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    pub fn correlation_id(mut self, id: impl Into<String>) -> Self {
        self.correlation_id = Some(id.into());
        self
    }

    pub fn event_id(mut self, id: impl Into<String>) -> Self {
        self.event_id = Some(id.into());
        self
    }

    pub fn metadata(mut self, metadata: HashMap<String, String>) -> Self {
        self.metadata.extend(metadata);
        self
    }

    pub fn add_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    pub fn headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers.extend(headers);
        self
    }

    pub fn payload_content_type(mut self, ct: impl Into<String>) -> Self {
        self.payload_content_type = ct.into();
        self
    }

    pub fn data_only(mut self, data_only: bool) -> Self {
        self.data_only = data_only;
        self
    }

    pub fn message_group(mut self, group: impl Into<String>) -> Self {
        self.message_group = Some(group.into());
        self
    }

    pub fn sequence(mut self, seq: i64) -> Self {
        self.sequence = Some(seq);
        self
    }

    pub fn timeout_seconds(mut self, secs: u32) -> Self {
        self.timeout_seconds = secs;
        self
    }

    pub fn max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }

    pub fn retry_strategy(mut self, strategy: impl Into<String>) -> Self {
        self.retry_strategy = Some(strategy.into());
        self
    }

    pub fn scheduled_for(mut self, at: DateTime<Utc>) -> Self {
        self.scheduled_for = Some(at);
        self
    }

    pub fn expires_at(mut self, at: DateTime<Utc>) -> Self {
        self.expires_at = Some(at);
        self
    }

    pub fn idempotency_key(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = Some(key.into());
        self
    }

    pub fn external_id(mut self, id: impl Into<String>) -> Self {
        self.external_id = Some(id.into());
        self
    }

    pub fn connection_id(mut self, id: impl Into<String>) -> Self {
        self.connection_id = Some(id.into());
        self
    }

    /// Build the dispatch job payload JSON for the outbox.
    pub fn to_payload(&self) -> serde_json::Value {
        let mut payload = serde_json::json!({
            "source": self.source,
            "code": self.code,
            "targetUrl": self.target_url,
            "payload": self.payload,
            "payloadContentType": self.payload_content_type,
            "dispatchPoolId": self.dispatch_pool_id,
            "dataOnly": self.data_only,
            "timeoutSeconds": self.timeout_seconds,
            "maxRetries": self.max_retries,
        });

        let obj = payload.as_object_mut().unwrap();
        if let Some(ref v) = self.subject {
            obj.insert("subject".into(), serde_json::json!(v));
        }
        if let Some(ref v) = self.correlation_id {
            obj.insert("correlationId".into(), serde_json::json!(v));
        }
        if let Some(ref v) = self.event_id {
            obj.insert("eventId".into(), serde_json::json!(v));
        }
        if !self.metadata.is_empty() {
            obj.insert("metadata".into(), serde_json::json!(self.metadata));
        }
        if !self.headers.is_empty() {
            obj.insert("headers".into(), serde_json::json!(self.headers));
        }
        if let Some(ref v) = self.message_group {
            obj.insert("messageGroup".into(), serde_json::json!(v));
        }
        if let Some(v) = self.sequence {
            obj.insert("sequence".into(), serde_json::json!(v));
        }
        if let Some(ref v) = self.retry_strategy {
            obj.insert("retryStrategy".into(), serde_json::json!(v));
        }
        if let Some(ref v) = self.scheduled_for {
            obj.insert("scheduledFor".into(), serde_json::json!(v.to_rfc3339()));
        }
        if let Some(ref v) = self.expires_at {
            obj.insert("expiresAt".into(), serde_json::json!(v.to_rfc3339()));
        }
        if let Some(ref v) = self.idempotency_key {
            obj.insert("idempotencyKey".into(), serde_json::json!(v));
        }
        if let Some(ref v) = self.external_id {
            obj.insert("externalId".into(), serde_json::json!(v));
        }
        if let Some(ref v) = self.connection_id {
            obj.insert("connectionId".into(), serde_json::json!(v));
        }

        payload
    }
}

// ─── CreateAuditLogDto ──────────────────────────────────────────────────────

/// Builder for creating an audit log entry in the outbox.
#[derive(Debug, Clone)]
pub struct CreateAuditLogDto {
    pub entity_type: String,
    pub entity_id: String,
    pub operation: String,
    pub operation_data: Option<serde_json::Value>,
    pub principal_id: Option<String>,
    pub performed_at: Option<DateTime<Utc>>,
    pub source: Option<String>,
    pub correlation_id: Option<String>,
    pub metadata: HashMap<String, String>,
    pub headers: HashMap<String, String>,
}

impl CreateAuditLogDto {
    pub fn new(
        entity_type: impl Into<String>,
        entity_id: impl Into<String>,
        operation: impl Into<String>,
    ) -> Self {
        Self {
            entity_type: entity_type.into(),
            entity_id: entity_id.into(),
            operation: operation.into(),
            operation_data: None,
            principal_id: None,
            performed_at: None,
            source: None,
            correlation_id: None,
            metadata: HashMap::new(),
            headers: HashMap::new(),
        }
    }

    pub fn operation_data(mut self, data: serde_json::Value) -> Self {
        self.operation_data = Some(data);
        self
    }

    /// Operation data with `masked` top-level fields masked in addition to
    /// the name rule every audit payload gets (see
    /// [`crate::usecase::audit`]) — e.g. a config value whose secrecy depends
    /// on a sibling field. The masks are applied here, so `operation_data`
    /// holds the masked document.
    pub fn operation_data_masked(mut self, data: serde_json::Value, masked: &[&str]) -> Self {
        self.operation_data = Some(crate::usecase::audit::redact(&data, masked));
        self
    }

    pub fn principal_id(mut self, id: impl Into<String>) -> Self {
        self.principal_id = Some(id.into());
        self
    }

    pub fn performed_at(mut self, at: DateTime<Utc>) -> Self {
        self.performed_at = Some(at);
        self
    }

    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    pub fn correlation_id(mut self, id: impl Into<String>) -> Self {
        self.correlation_id = Some(id.into());
        self
    }

    pub fn metadata(mut self, metadata: HashMap<String, String>) -> Self {
        self.metadata.extend(metadata);
        self
    }

    pub fn headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers.extend(headers);
        self
    }

    /// Build the audit log payload JSON for the outbox.
    pub fn to_payload(&self) -> serde_json::Value {
        let performed = self.performed_at.unwrap_or_else(Utc::now).to_rfc3339();

        let mut payload = serde_json::json!({
            "entityType": self.entity_type,
            "entityId": self.entity_id,
            "operation": self.operation,
            "performedAt": performed,
        });

        let obj = payload.as_object_mut().unwrap();
        if let Some(ref v) = self.operation_data {
            // Redacted before it is serialised (owner spec
            // docs/spec/audit-redaction.md, Java repo).
            let redacted = crate::usecase::audit::redact(v, &[]);
            obj.insert(
                "operationData".into(),
                serde_json::json!(redacted.to_string()),
            );
        }
        if let Some(ref v) = self.principal_id {
            obj.insert("principalId".into(), serde_json::json!(v));
        }
        if let Some(ref v) = self.source {
            obj.insert("source".into(), serde_json::json!(v));
        }
        if let Some(ref v) = self.correlation_id {
            obj.insert("correlationId".into(), serde_json::json!(v));
        }
        if !self.metadata.is_empty() {
            obj.insert("metadata".into(), serde_json::json!(self.metadata));
        }

        payload
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    // ─── CreateEventDto ─────────────────────────────────────────────────

    #[test]
    fn event_dto_new_sets_required_fields() {
        let data = serde_json::json!({"userId": "123"});
        let dto = CreateEventDto::new("user.registered", data.clone());

        assert_eq!(dto.event_type, "user.registered");
        assert_eq!(dto.data, data);
        assert!(dto.source.is_none());
        assert!(dto.subject.is_none());
        assert!(dto.correlation_id.is_none());
        assert!(dto.causation_id.is_none());
        assert!(dto.deduplication_id.is_none());
        assert!(dto.message_group.is_none());
        assert!(dto.context_data.is_empty());
        assert!(dto.headers.is_empty());
    }

    #[test]
    fn event_dto_builder_chain() {
        let dto = CreateEventDto::new("order.created", serde_json::json!({}))
            .source("order-service")
            .subject("orders.order.123")
            .correlation_id("corr-456")
            .causation_id("cause-789")
            .deduplication_id("dedup-001")
            .message_group("orders:order:123");

        assert_eq!(dto.source.as_deref(), Some("order-service"));
        assert_eq!(dto.subject.as_deref(), Some("orders.order.123"));
        assert_eq!(dto.correlation_id.as_deref(), Some("corr-456"));
        assert_eq!(dto.causation_id.as_deref(), Some("cause-789"));
        assert_eq!(dto.deduplication_id.as_deref(), Some("dedup-001"));
        assert_eq!(dto.message_group.as_deref(), Some("orders:order:123"));
    }

    #[test]
    fn event_dto_add_context() {
        let dto = CreateEventDto::new("test", serde_json::json!({}))
            .add_context("tenant", "acme")
            .add_context("region", "us-east-1");

        assert_eq!(dto.context_data.len(), 2);
        assert_eq!(dto.context_data[0].key, "tenant");
        assert_eq!(dto.context_data[0].value, "acme");
        assert_eq!(dto.context_data[1].key, "region");
        assert_eq!(dto.context_data[1].value, "us-east-1");
    }

    #[test]
    fn event_dto_context_data_extends() {
        let entries = vec![
            ContextDataEntry {
                key: "a".into(),
                value: "1".into(),
            },
            ContextDataEntry {
                key: "b".into(),
                value: "2".into(),
            },
        ];
        let dto = CreateEventDto::new("test", serde_json::json!({}))
            .add_context("existing", "val")
            .context_data(entries);

        assert_eq!(dto.context_data.len(), 3);
    }

    #[test]
    fn event_dto_headers() {
        let mut headers = HashMap::new();
        headers.insert("X-Custom".to_string(), "value1".to_string());

        let dto = CreateEventDto::new("test", serde_json::json!({})).headers(headers);

        assert_eq!(dto.headers.get("X-Custom").unwrap(), "value1");
    }

    #[test]
    fn event_dto_to_payload_required_fields_only() {
        let data = serde_json::json!({"key": "value"});
        let dto = CreateEventDto::new("user.created", data.clone());
        let payload = dto.to_payload();

        assert_eq!(payload["specVersion"], "1.0");
        assert_eq!(payload["type"], "user.created");
        // data is stringified in the payload
        let data_str: String = serde_json::from_value(payload["data"].clone()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&data_str).unwrap();
        assert_eq!(parsed, data);

        // Optional fields should be absent
        assert!(payload.get("source").is_none());
        assert!(payload.get("subject").is_none());
        assert!(payload.get("correlationId").is_none());
        assert!(payload.get("causationId").is_none());
        assert!(payload.get("deduplicationId").is_none());
        assert!(payload.get("messageGroup").is_none());
        assert!(payload.get("contextData").is_none());
    }

    #[test]
    fn event_dto_to_payload_all_optional_fields() {
        let dto = CreateEventDto::new("order.shipped", serde_json::json!({"tracking": "T1"}))
            .source("fulfillment")
            .subject("orders.order.42")
            .correlation_id("corr-1")
            .causation_id("cause-2")
            .deduplication_id("dedup-3")
            .message_group("orders:order:42")
            .add_context("env", "prod");

        let payload = dto.to_payload();

        assert_eq!(payload["source"], "fulfillment");
        assert_eq!(payload["subject"], "orders.order.42");
        assert_eq!(payload["correlationId"], "corr-1");
        assert_eq!(payload["causationId"], "cause-2");
        assert_eq!(payload["deduplicationId"], "dedup-3");
        assert_eq!(payload["messageGroup"], "orders:order:42");
        assert!(payload["contextData"].is_array());
        assert_eq!(payload["contextData"][0]["key"], "env");
        assert_eq!(payload["contextData"][0]["value"], "prod");
    }

    // ─── CreateDispatchJobDto ───────────────────────────────────────────

    #[test]
    fn dispatch_job_dto_new_sets_defaults() {
        let dto = CreateDispatchJobDto::new(
            "my-service",
            "user.registered",
            "https://hook.example.com",
            r#"{"id":"1"}"#,
            "pool_abc",
        );

        assert_eq!(dto.source, "my-service");
        assert_eq!(dto.code, "user.registered");
        assert_eq!(dto.target_url, "https://hook.example.com");
        assert_eq!(dto.payload, r#"{"id":"1"}"#);
        assert_eq!(dto.dispatch_pool_id, "pool_abc");
        assert_eq!(dto.payload_content_type, "application/json");
        assert!(dto.data_only);
        assert_eq!(dto.timeout_seconds, 30);
        assert_eq!(dto.max_retries, 5);
        assert!(dto.subject.is_none());
        assert!(dto.correlation_id.is_none());
        assert!(dto.event_id.is_none());
        assert!(dto.metadata.is_empty());
        assert!(dto.headers.is_empty());
        assert!(dto.message_group.is_none());
        assert!(dto.sequence.is_none());
        assert!(dto.retry_strategy.is_none());
        assert!(dto.scheduled_for.is_none());
        assert!(dto.expires_at.is_none());
        assert!(dto.idempotency_key.is_none());
        assert!(dto.external_id.is_none());
        assert!(dto.connection_id.is_none());
    }

    #[test]
    fn dispatch_job_dto_from_json_auto_stringifies() {
        let payload_obj = serde_json::json!({"orderId": "ord_123", "amount": 99.99});
        let dto =
            CreateDispatchJobDto::from_json("svc", "evt", "https://x.com", &payload_obj, "pool_1")
                .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&dto.payload).unwrap();
        assert_eq!(parsed["orderId"], "ord_123");
        assert_eq!(parsed["amount"], 99.99);
    }

    #[test]
    fn dispatch_job_dto_from_json_propagates_serialization_errors() {
        struct Unserializable;
        impl Serialize for Unserializable {
            fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("nope"))
            }
        }
        let result =
            CreateDispatchJobDto::from_json("svc", "evt", "https://x.com", &Unserializable, "p");
        assert!(result.is_err(), "must not silently fall back to {{}}");
    }

    #[test]
    fn dispatch_job_dto_builder_chain() {
        let scheduled = Utc.with_ymd_and_hms(2026, 6, 15, 12, 0, 0).unwrap();
        let expires = Utc.with_ymd_and_hms(2026, 6, 16, 12, 0, 0).unwrap();

        let dto = CreateDispatchJobDto::new("svc", "code", "url", "{}", "pool")
            .subject("sub.123")
            .correlation_id("corr-1")
            .event_id("evt-1")
            .add_metadata("key1", "val1")
            .payload_content_type("text/plain")
            .data_only(false)
            .message_group("grp:1")
            .sequence(42)
            .timeout_seconds(120)
            .max_retries(10)
            .retry_strategy("exponential")
            .scheduled_for(scheduled)
            .expires_at(expires)
            .idempotency_key("idem-1")
            .external_id("ext-1")
            .connection_id("conn-1");

        assert_eq!(dto.subject.as_deref(), Some("sub.123"));
        assert_eq!(dto.correlation_id.as_deref(), Some("corr-1"));
        assert_eq!(dto.event_id.as_deref(), Some("evt-1"));
        assert_eq!(dto.metadata.get("key1").unwrap(), "val1");
        assert_eq!(dto.payload_content_type, "text/plain");
        assert!(!dto.data_only);
        assert_eq!(dto.message_group.as_deref(), Some("grp:1"));
        assert_eq!(dto.sequence, Some(42));
        assert_eq!(dto.timeout_seconds, 120);
        assert_eq!(dto.max_retries, 10);
        assert_eq!(dto.retry_strategy.as_deref(), Some("exponential"));
        assert_eq!(dto.scheduled_for, Some(scheduled));
        assert_eq!(dto.expires_at, Some(expires));
        assert_eq!(dto.idempotency_key.as_deref(), Some("idem-1"));
        assert_eq!(dto.external_id.as_deref(), Some("ext-1"));
        assert_eq!(dto.connection_id.as_deref(), Some("conn-1"));
    }

    #[test]
    fn dispatch_job_dto_metadata_extends() {
        let mut extra = HashMap::new();
        extra.insert("k2".to_string(), "v2".to_string());

        let dto = CreateDispatchJobDto::new("s", "c", "u", "{}", "p")
            .add_metadata("k1", "v1")
            .metadata(extra);

        assert_eq!(dto.metadata.len(), 2);
        assert_eq!(dto.metadata["k1"], "v1");
        assert_eq!(dto.metadata["k2"], "v2");
    }

    #[test]
    fn dispatch_job_dto_to_payload_required_fields() {
        let dto = CreateDispatchJobDto::new(
            "svc",
            "order.created",
            "https://hook.example.com",
            r#"{"a":1}"#,
            "pool_x",
        );
        let payload = dto.to_payload();

        assert_eq!(payload["source"], "svc");
        assert_eq!(payload["code"], "order.created");
        assert_eq!(payload["targetUrl"], "https://hook.example.com");
        assert_eq!(payload["payload"], r#"{"a":1}"#);
        assert_eq!(payload["payloadContentType"], "application/json");
        assert_eq!(payload["dispatchPoolId"], "pool_x");
        assert_eq!(payload["dataOnly"], true);
        assert_eq!(payload["timeoutSeconds"], 30);
        assert_eq!(payload["maxRetries"], 5);

        // Optional fields absent
        assert!(payload.get("subject").is_none());
        assert!(payload.get("correlationId").is_none());
        assert!(payload.get("eventId").is_none());
        assert!(payload.get("metadata").is_none());
        assert!(payload.get("headers").is_none());
        assert!(payload.get("messageGroup").is_none());
        assert!(payload.get("sequence").is_none());
        assert!(payload.get("retryStrategy").is_none());
        assert!(payload.get("scheduledFor").is_none());
        assert!(payload.get("expiresAt").is_none());
        assert!(payload.get("idempotencyKey").is_none());
        assert!(payload.get("externalId").is_none());
        assert!(payload.get("connectionId").is_none());
    }

    #[test]
    fn dispatch_job_dto_to_payload_all_optional_fields() {
        let scheduled = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let dto = CreateDispatchJobDto::new("s", "c", "u", "{}", "p")
            .subject("sub")
            .correlation_id("corr")
            .event_id("evt")
            .add_metadata("mk", "mv")
            .message_group("grp")
            .sequence(7)
            .retry_strategy("linear")
            .scheduled_for(scheduled)
            .expires_at(scheduled)
            .idempotency_key("ik")
            .external_id("eid")
            .connection_id("cid");

        let payload = dto.to_payload();

        assert_eq!(payload["subject"], "sub");
        assert_eq!(payload["correlationId"], "corr");
        assert_eq!(payload["eventId"], "evt");
        assert_eq!(payload["metadata"]["mk"], "mv");
        assert_eq!(payload["messageGroup"], "grp");
        assert_eq!(payload["sequence"], 7);
        assert_eq!(payload["retryStrategy"], "linear");
        assert!(payload["scheduledFor"].as_str().unwrap().contains("2026"));
        assert!(payload["expiresAt"].as_str().unwrap().contains("2026"));
        assert_eq!(payload["idempotencyKey"], "ik");
        assert_eq!(payload["externalId"], "eid");
        assert_eq!(payload["connectionId"], "cid");
    }

    // ─── CreateAuditLogDto ──────────────────────────────────────────────

    #[test]
    fn audit_log_dto_new_sets_required_fields() {
        let dto = CreateAuditLogDto::new("User", "usr_123", "CREATE");

        assert_eq!(dto.entity_type, "User");
        assert_eq!(dto.entity_id, "usr_123");
        assert_eq!(dto.operation, "CREATE");
        assert!(dto.operation_data.is_none());
        assert!(dto.principal_id.is_none());
        assert!(dto.performed_at.is_none());
        assert!(dto.source.is_none());
        assert!(dto.correlation_id.is_none());
        assert!(dto.metadata.is_empty());
        assert!(dto.headers.is_empty());
    }

    #[test]
    fn audit_log_dto_builder_chain() {
        let now = Utc::now();
        let dto = CreateAuditLogDto::new("Order", "ord_1", "UPDATE")
            .operation_data(serde_json::json!({"status": "shipped"}))
            .principal_id("usr_admin")
            .performed_at(now)
            .source("admin-panel")
            .correlation_id("corr-x");

        assert_eq!(dto.operation_data.unwrap()["status"], "shipped");
        assert_eq!(dto.principal_id.as_deref(), Some("usr_admin"));
        assert_eq!(dto.performed_at, Some(now));
        assert_eq!(dto.source.as_deref(), Some("admin-panel"));
        assert_eq!(dto.correlation_id.as_deref(), Some("corr-x"));
    }

    #[test]
    fn audit_log_dto_to_payload_required_fields() {
        let dto = CreateAuditLogDto::new("Role", "rol_1", "DELETE");
        let payload = dto.to_payload();

        assert_eq!(payload["entityType"], "Role");
        assert_eq!(payload["entityId"], "rol_1");
        assert_eq!(payload["operation"], "DELETE");
        // performedAt is always present (defaults to now)
        assert!(payload["performedAt"].as_str().is_some());

        // Optional fields absent
        assert!(payload.get("operationData").is_none());
        assert!(payload.get("principalId").is_none());
        assert!(payload.get("source").is_none());
        assert!(payload.get("correlationId").is_none());
        assert!(payload.get("metadata").is_none());
    }

    #[test]
    fn audit_log_dto_to_payload_all_optional_fields() {
        let fixed_time = Utc.with_ymd_and_hms(2026, 3, 15, 10, 30, 0).unwrap();
        let mut meta = HashMap::new();
        meta.insert("ip".to_string(), "10.0.0.1".to_string());

        let dto = CreateAuditLogDto::new("Client", "clt_1", "UPDATE")
            .operation_data(serde_json::json!({"name": "New Name"}))
            .principal_id("prn_admin")
            .performed_at(fixed_time)
            .source("admin-api")
            .correlation_id("corr-99")
            .metadata(meta);

        let payload = dto.to_payload();

        assert!(payload["performedAt"].as_str().unwrap().contains("2026"));
        // operationData is stringified JSON
        let op_data_str = payload["operationData"].as_str().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(op_data_str).unwrap();
        assert_eq!(parsed["name"], "New Name");
        assert_eq!(payload["principalId"], "prn_admin");
        assert_eq!(payload["source"], "admin-api");
        assert_eq!(payload["correlationId"], "corr-99");
        assert_eq!(payload["metadata"]["ip"], "10.0.0.1");
    }

    #[test]
    fn audit_log_dto_performed_at_defaults_to_now() {
        let before = Utc::now();
        let dto = CreateAuditLogDto::new("X", "x_1", "CREATE");
        let payload = dto.to_payload();
        let after = Utc::now();

        let performed_str = payload["performedAt"].as_str().unwrap();
        let performed: DateTime<Utc> = performed_str.parse().unwrap();

        assert!(performed >= before && performed <= after);
    }

    // ─── Clone + Debug ──────────────────────────────────────────────────

    #[test]
    fn dtos_are_clone_and_debug() {
        let event_dto = CreateEventDto::new("t", serde_json::json!(null));
        let _ = event_dto.clone();
        let _ = format!("{:?}", event_dto);

        let job_dto = CreateDispatchJobDto::new("s", "c", "u", "{}", "p");
        let _ = job_dto.clone();
        let _ = format!("{:?}", job_dto);

        let audit_dto = CreateAuditLogDto::new("E", "e_1", "OP");
        let _ = audit_dto.clone();
        let _ = format!("{:?}", audit_dto);
    }
}
