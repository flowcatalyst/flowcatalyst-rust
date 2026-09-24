//! Outbox Payload Builders
//!
//! Helpers for constructing outbox payloads for direct insertion.
//! Use these when you need to write outbox items outside of the UnitOfWork pattern
//! (e.g., dispatch jobs, standalone audit logs, or custom event emission).

use serde::Serialize;
use sqlx::{PgPool, Postgres, Transaction};

use crate::tsid::TsidGenerator;
use crate::usecase::DomainEvent;

use super::error::OutboxError;

/// Write a dispatch job to the outbox for async processing.
///
/// Dispatch jobs are created when you need the platform to deliver a webhook
/// to a subscription's connection endpoint.
///
/// # Example
///
/// ```ignore
/// use fc_sdk::outbox::payload::write_dispatch_job;
///
/// write_dispatch_job(
///     &mut txn,
///     "outbox_messages",
///     &DispatchJobPayload {
///         code: "orders:fulfillment:shipment:shipped".to_string(),
///         target_url: "https://webhook.example.com/shipments".to_string(),
///         payload: serde_json::json!({"order_id": "123"}),
///         subscription_id: "sub_0HZXEQ5Y8JY5Z".to_string(),
///         event_id: Some("evn_0HZXEQ5Y8JY5Z".to_string()),
///         message_group: Some("fulfillment:shipment:123".to_string()),
///         ..Default::default()
///     },
///     None, // client_id
/// ).await?;
/// ```
pub async fn write_dispatch_job(
    txn: &mut Transaction<'_, Postgres>,
    table: &str,
    job: &DispatchJobPayload,
    client_id: Option<&str>,
) -> Result<(), OutboxError> {
    let id = TsidGenerator::generate_untyped();
    let payload = serde_json::to_value(job)?;
    let payload_size = payload.to_string().len() as i32;

    let query = format!(
        "INSERT INTO {} (id, type, message_group, payload, status, retry_count, created_at, updated_at, client_id, payload_size) \
         VALUES ($1, 'DISPATCH_JOB', $2, $3, 0, 0, NOW(), NOW(), $4, $5)",
        table
    );

    sqlx::query(&query)
        .bind(&id)
        .bind(job.message_group.as_deref())
        .bind(&payload)
        .bind(client_id)
        .bind(payload_size)
        .execute(&mut **txn)
        .await?;

    Ok(())
}

/// Write an event to the outbox outside of the UnitOfWork pattern.
///
/// Use this for standalone event emission when you don't have an entity to persist.
pub async fn write_event<E: DomainEvent + Serialize>(
    txn: &mut Transaction<'_, Postgres>,
    table: &str,
    event: &E,
    client_id: Option<&str>,
) -> Result<(), OutboxError> {
    let id = TsidGenerator::generate_untyped();
    let data_json: serde_json::Value =
        serde_json::from_str(&event.to_data_json()).unwrap_or(serde_json::json!({}));

    let payload = serde_json::json!({
        "event_type": event.event_type(),
        "spec_version": event.spec_version(),
        "source": event.source(),
        "subject": event.subject(),
        "data": data_json,
        "correlation_id": event.correlation_id(),
        "causation_id": event.causation_id(),
        "deduplication_id": format!("{}-{}", event.event_type(), event.event_id()),
        "message_group": event.message_group(),
        "context_data": [
            {"key": "principalId", "value": event.principal_id()},
        ],
    });

    let payload_size = payload.to_string().len() as i32;

    let query = format!(
        "INSERT INTO {} (id, type, message_group, payload, status, retry_count, created_at, updated_at, client_id, payload_size) \
         VALUES ($1, 'EVENT', $2, $3, 0, 0, NOW(), NOW(), $4, $5)",
        table
    );

    sqlx::query(&query)
        .bind(&id)
        .bind(event.message_group())
        .bind(&payload)
        .bind(client_id)
        .bind(payload_size)
        .execute(&mut **txn)
        .await?;

    Ok(())
}

/// Write an audit log entry to the outbox outside of the UnitOfWork pattern.
pub async fn write_audit_log(
    txn: &mut Transaction<'_, Postgres>,
    table: &str,
    audit: &AuditLogPayload,
    client_id: Option<&str>,
) -> Result<(), OutboxError> {
    let id = TsidGenerator::generate_untyped();
    let payload = serde_json::to_value(audit)?;
    let payload_size = payload.to_string().len() as i32;

    let query = format!(
        "INSERT INTO {} (id, type, message_group, payload, status, retry_count, created_at, updated_at, client_id, payload_size) \
         VALUES ($1, 'AUDIT_LOG', $2, $3, 0, 0, NOW(), NOW(), $4, $5)",
        table
    );

    sqlx::query(&query)
        .bind(&id)
        .bind(audit.message_group.as_deref())
        .bind(&payload)
        .bind(client_id)
        .bind(payload_size)
        .execute(&mut **txn)
        .await?;

    Ok(())
}

/// Convenience: write an event directly to the outbox (auto-manages transaction).
pub async fn emit_event<E: DomainEvent + Serialize>(
    pool: &PgPool,
    table: &str,
    event: &E,
    client_id: Option<&str>,
) -> Result<(), OutboxError> {
    let mut txn = pool.begin().await?;
    write_event(&mut txn, table, event, client_id).await?;
    txn.commit().await?;
    Ok(())
}

/// Convenience: write a dispatch job directly to the outbox (auto-manages transaction).
pub async fn emit_dispatch_job(
    pool: &PgPool,
    table: &str,
    job: &DispatchJobPayload,
    client_id: Option<&str>,
) -> Result<(), OutboxError> {
    let mut txn = pool.begin().await?;
    write_dispatch_job(&mut txn, table, job, client_id).await?;
    txn.commit().await?;
    Ok(())
}

/// Convenience: write an audit log directly to the outbox (auto-manages transaction).
pub async fn emit_audit_log(
    pool: &PgPool,
    table: &str,
    audit: &AuditLogPayload,
    client_id: Option<&str>,
) -> Result<(), OutboxError> {
    let mut txn = pool.begin().await?;
    write_audit_log(&mut txn, table, audit, client_id).await?;
    txn.commit().await?;
    Ok(())
}

// ─── Payload Types ───────────────────────────────────────────────────────────

/// Payload for a dispatch job outbox item.
#[derive(Debug, Clone, Serialize)]
pub struct DispatchJobPayload {
    /// Event type code or task identifier
    pub code: String,
    /// Webhook endpoint URL
    pub target_url: String,
    /// JSON payload to deliver
    pub payload: serde_json::Value,
    /// Subscription that created this job
    pub subscription_id: String,
    /// Triggering event ID (for event-triggered jobs)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    /// Service account ID for authentication
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account_id: Option<String>,
    /// Dispatch pool for rate limiting
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatch_pool_id: Option<String>,
    /// Message group for FIFO ordering
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_group: Option<String>,
    /// Dispatch mode: "immediate", "next_on_error", "block_on_error"
    #[serde(default = "default_mode")]
    pub mode: String,
    /// Send raw data only (no CloudEvents envelope)
    #[serde(default)]
    pub data_only: bool,
    /// Timeout in seconds for the webhook call
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u32,
    /// Maximum retry attempts
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    /// Retry strategy: "immediate", "fixed_delay", "exponential_backoff"
    #[serde(default = "default_retry_strategy")]
    pub retry_strategy: String,
    /// Idempotency key for deduplication
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

fn default_mode() -> String {
    "immediate".to_string()
}

fn default_timeout() -> u32 {
    30
}

fn default_max_retries() -> u32 {
    3
}

fn default_retry_strategy() -> String {
    "exponential_backoff".to_string()
}

impl Default for DispatchJobPayload {
    fn default() -> Self {
        Self {
            code: String::new(),
            target_url: String::new(),
            payload: serde_json::json!({}),
            subscription_id: String::new(),
            event_id: None,
            service_account_id: None,
            dispatch_pool_id: None,
            message_group: None,
            mode: default_mode(),
            data_only: false,
            timeout_seconds: default_timeout(),
            max_retries: default_max_retries(),
            retry_strategy: default_retry_strategy(),
            idempotency_key: None,
        }
    }
}

/// Payload for an audit log outbox item.
#[derive(Debug, Clone, Serialize)]
pub struct AuditLogPayload {
    /// Entity type that was changed (e.g., "Order", "Shipment")
    pub entity_type: String,
    /// ID of the changed entity
    pub entity_id: String,
    /// Operation name (e.g., "CreateOrder", "ShipOrder")
    pub operation: String,
    /// Serialized command/operation details
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_json: Option<serde_json::Value>,
    /// Principal who performed the action
    pub principal_id: String,
    /// Application ID context
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_id: Option<String>,
    /// Client ID context
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// When the operation was performed
    pub performed_at: String,
    /// Message group for ordering (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_group: Option<String>,
}

impl AuditLogPayload {
    /// Create from a domain event and command.
    pub fn from_event<E: DomainEvent, C: Serialize>(event: &E, command: &C) -> Self {
        let command_name = std::any::type_name::<C>()
            .rsplit("::")
            .next()
            .unwrap_or("Unknown")
            .to_string();

        let subject = event.subject();
        let entity_type = subject
            .split('.')
            .nth(1)
            .map(|s| {
                let mut chars = s.chars();
                match chars.next() {
                    Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            })
            .unwrap_or_else(|| "Unknown".to_string());
        let entity_id = subject.split('.').nth(2).unwrap_or("").to_string();

        Self {
            entity_type,
            entity_id,
            operation: command_name,
            operation_json: serde_json::to_value(command).ok(),
            principal_id: event.principal_id().to_string(),
            application_id: None,
            client_id: None,
            performed_at: event.time().to_rfc3339(),
            message_group: Some(event.message_group().to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usecase::EventMetadata;

    // ─── DispatchJobPayload ─────────────────────────────────────────────

    #[test]
    fn dispatch_job_payload_default_values() {
        let djp = DispatchJobPayload::default();

        assert!(djp.code.is_empty());
        assert!(djp.target_url.is_empty());
        assert_eq!(djp.payload, serde_json::json!({}));
        assert!(djp.subscription_id.is_empty());
        assert!(djp.event_id.is_none());
        assert!(djp.service_account_id.is_none());
        assert!(djp.dispatch_pool_id.is_none());
        assert!(djp.message_group.is_none());
        assert_eq!(djp.mode, "immediate");
        assert!(!djp.data_only);
        assert_eq!(djp.timeout_seconds, 30);
        assert_eq!(djp.max_retries, 3);
        assert_eq!(djp.retry_strategy, "exponential_backoff");
        assert!(djp.idempotency_key.is_none());
    }

    #[test]
    fn dispatch_job_payload_serialization_required_fields() {
        let djp = DispatchJobPayload {
            code: "order.created".to_string(),
            target_url: "https://hook.example.com".to_string(),
            payload: serde_json::json!({"orderId": "123"}),
            subscription_id: "sub_abc".to_string(),
            ..Default::default()
        };

        let json = serde_json::to_value(&djp).unwrap();

        assert_eq!(json["code"], "order.created");
        assert_eq!(json["target_url"], "https://hook.example.com");
        assert_eq!(json["payload"]["orderId"], "123");
        assert_eq!(json["subscription_id"], "sub_abc");
        assert_eq!(json["mode"], "immediate");
        assert_eq!(json["timeout_seconds"], 30);
        assert_eq!(json["max_retries"], 3);
        assert_eq!(json["retry_strategy"], "exponential_backoff");
    }

    #[test]
    fn dispatch_job_payload_skip_serializing_none_fields() {
        let djp = DispatchJobPayload::default();
        let json = serde_json::to_value(&djp).unwrap();

        // Optional fields with skip_serializing_if should be absent
        assert!(json.get("event_id").is_none());
        assert!(json.get("service_account_id").is_none());
        assert!(json.get("dispatch_pool_id").is_none());
        assert!(json.get("message_group").is_none());
        assert!(json.get("idempotency_key").is_none());
    }

    #[test]
    fn dispatch_job_payload_with_optional_fields() {
        let djp = DispatchJobPayload {
            code: "test".to_string(),
            target_url: "https://x.com".to_string(),
            payload: serde_json::json!({}),
            subscription_id: "sub_1".to_string(),
            event_id: Some("evt_123".to_string()),
            service_account_id: Some("svc_456".to_string()),
            dispatch_pool_id: Some("pool_789".to_string()),
            message_group: Some("grp:1".to_string()),
            idempotency_key: Some("idem_abc".to_string()),
            ..Default::default()
        };

        let json = serde_json::to_value(&djp).unwrap();

        assert_eq!(json["event_id"], "evt_123");
        assert_eq!(json["service_account_id"], "svc_456");
        assert_eq!(json["dispatch_pool_id"], "pool_789");
        assert_eq!(json["message_group"], "grp:1");
        assert_eq!(json["idempotency_key"], "idem_abc");
    }

    #[test]
    fn dispatch_job_payload_clone() {
        let djp = DispatchJobPayload {
            code: "test".into(),
            ..Default::default()
        };
        let cloned = djp.clone();
        assert_eq!(cloned.code, "test");
    }

    // ─── AuditLogPayload ────────────────────────────────────────────────

    #[test]
    fn audit_log_payload_serialization() {
        let alp = AuditLogPayload {
            entity_type: "Order".to_string(),
            entity_id: "ord_123".to_string(),
            operation: "CreateOrder".to_string(),
            operation_json: Some(serde_json::json!({"customer_id": "cust_1"})),
            principal_id: "prn_admin".to_string(),
            application_id: Some("app_1".to_string()),
            client_id: Some("clt_1".to_string()),
            performed_at: "2026-01-01T00:00:00+00:00".to_string(),
            message_group: Some("orders:order:ord_123".to_string()),
        };

        let json = serde_json::to_value(&alp).unwrap();

        assert_eq!(json["entity_type"], "Order");
        assert_eq!(json["entity_id"], "ord_123");
        assert_eq!(json["operation"], "CreateOrder");
        assert_eq!(json["operation_json"]["customer_id"], "cust_1");
        assert_eq!(json["principal_id"], "prn_admin");
        assert_eq!(json["application_id"], "app_1");
        assert_eq!(json["client_id"], "clt_1");
        assert_eq!(json["message_group"], "orders:order:ord_123");
    }

    #[test]
    fn audit_log_payload_skip_serializing_none() {
        let alp = AuditLogPayload {
            entity_type: "X".into(),
            entity_id: "x_1".into(),
            operation: "Op".into(),
            operation_json: None,
            principal_id: "prn".into(),
            application_id: None,
            client_id: None,
            performed_at: "t".into(),
            message_group: None,
        };
        let json = serde_json::to_value(&alp).unwrap();

        assert!(json.get("operation_json").is_none());
        assert!(json.get("application_id").is_none());
        assert!(json.get("client_id").is_none());
        assert!(json.get("message_group").is_none());
    }

    #[test]
    fn audit_log_payload_clone() {
        let alp = AuditLogPayload {
            entity_type: "Order".into(),
            entity_id: "ord_1".into(),
            operation: "Create".into(),
            operation_json: None,
            principal_id: "prn".into(),
            application_id: None,
            client_id: None,
            performed_at: "t".into(),
            message_group: None,
        };
        let cloned = alp.clone();
        assert_eq!(cloned.entity_type, "Order");
        assert_eq!(cloned.entity_id, "ord_1");
    }

    // ─── AuditLogPayload::from_event ────────────────────────────────────

    #[derive(Debug, Clone, serde::Serialize)]
    struct TestEvent {
        pub metadata: EventMetadata,
        pub order_id: String,
    }

    crate::impl_domain_event!(TestEvent);

    #[derive(serde::Serialize)]
    struct CreateOrderCommand {
        pub customer_id: String,
        pub items: Vec<String>,
    }

    #[test]
    fn audit_log_from_event_extracts_entity_type() {
        let meta = EventMetadata::new(
            "evt_1".into(),
            "shop:orders:order:created",
            "1.0",
            "shop:orders",
            "orders.order.ord_123".into(),
            "orders:order:ord_123".into(),
            "exec-1".into(),
            "corr-1".into(),
            None,
            "prn_user".into(),
        );
        let event = TestEvent {
            metadata: meta,
            order_id: "ord_123".into(),
        };
        let cmd = CreateOrderCommand {
            customer_id: "cust_1".into(),
            items: vec!["item_a".into()],
        };

        let audit = AuditLogPayload::from_event(&event, &cmd);

        // "orders.order.ord_123" -> entity_type = "Order" (capitalized 2nd segment)
        assert_eq!(audit.entity_type, "Order");
        // entity_id = 3rd segment
        assert_eq!(audit.entity_id, "ord_123");
        // operation = type name of command (last segment)
        assert_eq!(audit.operation, "CreateOrderCommand");
        // principal from event
        assert_eq!(audit.principal_id, "prn_user");
        // message_group from event
        assert_eq!(audit.message_group.as_deref(), Some("orders:order:ord_123"));
        // operation_json should serialize the command
        let op_json = audit.operation_json.unwrap();
        assert_eq!(op_json["customer_id"], "cust_1");
        assert_eq!(op_json["items"][0], "item_a");
        // application_id and client_id are None
        assert!(audit.application_id.is_none());
        assert!(audit.client_id.is_none());
    }

    #[test]
    fn audit_log_from_event_short_subject() {
        let meta = EventMetadata::new(
            "e".into(),
            "t",
            "1",
            "s",
            "single".into(), // only one segment
            "grp".into(),
            "exec".into(),
            "corr".into(),
            None,
            "prn".into(),
        );
        let event = TestEvent {
            metadata: meta,
            order_id: "x".into(),
        };

        #[derive(serde::Serialize)]
        struct Cmd;

        let audit = AuditLogPayload::from_event(&event, &Cmd);

        // No 2nd segment -> "Unknown"
        assert_eq!(audit.entity_type, "Unknown");
        // No 3rd segment -> ""
        assert_eq!(audit.entity_id, "");
    }

    #[test]
    fn audit_log_from_event_performed_at_is_rfc3339() {
        let meta = EventMetadata::new(
            "e".into(),
            "t",
            "1",
            "s",
            "a.b.c".into(),
            "grp".into(),
            "exec".into(),
            "corr".into(),
            None,
            "prn".into(),
        );
        let event = TestEvent {
            metadata: meta,
            order_id: "x".into(),
        };

        #[derive(serde::Serialize)]
        struct Cmd;

        let audit = AuditLogPayload::from_event(&event, &Cmd);

        // Should be parseable as RFC3339
        let _: chrono::DateTime<chrono::Utc> = audit.performed_at.parse().unwrap();
    }
}
