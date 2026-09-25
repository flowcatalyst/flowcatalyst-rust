//! Outbox Repository Trait
//!
//! The storage contract of the outbox processor, shaped as Go's
//! (`flowcatalyst-go/internal/outbox/repository.go`):
//!
//! - [`OutboxRepository::claim_pending`] atomically claims PENDING rows of
//!   every item type (EVENT, DISPATCH_JOB, AUDIT_LOG) and marks them
//!   IN_PROGRESS in the same statement, so a row is never polled twice while
//!   it is in flight;
//! - a row leaves the table only after the platform accepted it
//!   ([`OutboxRepository::mark_success`] deletes it);
//! - a failure is written to the row ([`OutboxRepository::mark_failed`]): the
//!   retry count is bumped, the error kept, and the row goes back to PENDING
//!   (retryable) or keeps its failure code (terminal);
//! - rows claimed by a processor that died are returned to PENDING by
//!   [`OutboxRepository::recover_stuck`].
//!
//! The rows live in the application's own database, in the table the SDKs
//! create (Rust, TypeScript, Laravel, Go, Java). Their column types differ
//! between SDKs (`payload` TEXT or JSONB, `status` SMALLINT or INTEGER,
//! timestamps with or without a zone), so every backend reads the columns in
//! a type-tolerant way and reads only the processor's columns (`id`, `type`,
//! `message_group`, `payload`, `status`, `retry_count`, `error_message`,
//! `created_at`, `updated_at`), as Go does.

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use fc_common::{OutboxItem, OutboxItemType, OutboxStatus};
use std::collections::HashSet;
use std::time::Duration;

/// Configuration for outbox repository tables.
///
/// By default all types share a single `outbox_messages` table.
/// Each type can optionally be routed to a separate table.
#[derive(Debug, Clone)]
pub struct OutboxTableConfig {
    /// Table name for EVENT items (default: "outbox_messages")
    pub events_table: String,
    /// Table name for DISPATCH_JOB items (default: "outbox_messages")
    pub dispatch_jobs_table: String,
    /// Table name for AUDIT_LOG items (default: "outbox_messages")
    pub audit_logs_table: String,
}

impl Default for OutboxTableConfig {
    fn default() -> Self {
        Self {
            events_table: "outbox_messages".to_string(),
            dispatch_jobs_table: "outbox_messages".to_string(),
            audit_logs_table: "outbox_messages".to_string(),
        }
    }
}

impl OutboxTableConfig {
    /// Get table name for item type
    pub fn table_for_type(&self, item_type: OutboxItemType) -> &str {
        match item_type {
            OutboxItemType::Event => &self.events_table,
            OutboxItemType::DispatchJob => &self.dispatch_jobs_table,
            OutboxItemType::AuditLog => &self.audit_logs_table,
        }
    }

    /// Get the set of unique table names (for schema creation).
    /// Deduplicates when multiple types share the same table.
    pub fn unique_tables(&self) -> Vec<&str> {
        let mut seen = HashSet::new();
        let mut tables = Vec::new();
        for table in [
            &self.events_table,
            &self.dispatch_jobs_table,
            &self.audit_logs_table,
        ] {
            if seen.insert(table.as_str()) {
                tables.push(table.as_str());
            }
        }
        tables
    }

    /// Each unique table with the item types routed to it, in table order.
    /// A claim reads only these types from a table, so a row of an unknown
    /// `type` is never claimed (it would have no endpoint to go to).
    pub fn tables_with_types(&self) -> Vec<(&str, Vec<OutboxItemType>)> {
        let mut out: Vec<(&str, Vec<OutboxItemType>)> = Vec::new();
        for item_type in OutboxItemType::ALL {
            let table = self.table_for_type(item_type);
            match out.iter_mut().find(|(t, _)| *t == table) {
                Some((_, types)) => types.push(item_type),
                None => out.push((table, vec![item_type])),
            }
        }
        out
    }
}

/// A claimed row that cannot be sent: its payload is not JSON. The processor
/// fails it terminally (BAD_REQUEST) instead of letting one bad row fail the
/// whole claim.
#[derive(Debug, Clone, PartialEq)]
pub struct InvalidRow {
    pub id: String,
    pub item_type: OutboxItemType,
    pub message_group: Option<String>,
    pub error: String,
}

/// The rows one claim marked IN_PROGRESS.
#[derive(Debug, Clone, Default)]
pub struct ClaimedBatch {
    /// Rows ready to send.
    pub items: Vec<OutboxItem>,
    /// Rows whose payload could not be read.
    pub invalid: Vec<InvalidRow>,
}

impl ClaimedBatch {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty() && self.invalid.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len() + self.invalid.len()
    }

    /// Adds a raw row: a payload that parses becomes an item, one that does
    /// not becomes an [`InvalidRow`].
    #[allow(clippy::too_many_arguments)]
    pub fn push_row(
        &mut self,
        id: String,
        item_type: OutboxItemType,
        message_group: Option<String>,
        payload: &str,
        retry_count: i32,
        error_message: Option<String>,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) {
        match serde_json::from_str::<serde_json::Value>(payload) {
            Ok(payload) => self.items.push(OutboxItem {
                id,
                item_type,
                message_group: message_group.filter(|g| !g.is_empty()),
                payload,
                status: OutboxStatus::InProgress,
                retry_count,
                created_at,
                updated_at,
                error_message,
                client_id: None,
                payload_size: None,
                headers: None,
            }),
            Err(e) => self.invalid.push(InvalidRow {
                id,
                item_type,
                message_group: message_group.filter(|g| !g.is_empty()),
                error: format!("marshal: payload is not JSON: {e}"),
            }),
        }
    }
}

/// Outbox persistence, implemented once per supported database.
///
/// Every id-based operation names the item type, which picks the table
/// ([`OutboxTableConfig::table_for_type`]); ids are the table's primary key.
#[async_trait]
pub trait OutboxRepository: Send + Sync {
    /// Claims up to `limit` PENDING rows of every known item type, ordered by
    /// `message_group, created_at` (then `id`), and marks them IN_PROGRESS in
    /// the same atomic step (Go `ClaimPending`). A row claimed here is not
    /// returned by another claim until it is resolved, released or recovered.
    async fn claim_pending(&self, limit: u32) -> Result<ClaimedBatch>;

    /// The platform accepted these rows: delete them (Go `MarkSuccess`), which
    /// keeps the application's table bounded.
    async fn mark_success(&self, item_type: OutboxItemType, ids: &[String]) -> Result<()>;

    /// Records a failed attempt (Go `MarkFailed`): bumps `retry_count`, stores
    /// `error_message`, and sets the status to PENDING when `requeue` (the
    /// next poll re-claims it) or to `status` otherwise (not re-claimed).
    async fn mark_failed(
        &self,
        item_type: OutboxItemType,
        ids: &[String],
        status: OutboxStatus,
        error_message: &str,
        requeue: bool,
    ) -> Result<()>;

    /// Returns claimed rows to PENDING with no failure penalty (Go `Release`):
    /// no retry bump, no error. Only rows still IN_PROGRESS are touched.
    async fn release(&self, item_type: OutboxItemType, ids: &[String]) -> Result<()>;

    /// Resets rows to PENDING from any status, clearing `retry_count` and the
    /// error, for a fresh attempt (Go `Requeue`, the unblock of a group).
    async fn requeue(&self, item_type: OutboxItemType, ids: &[String]) -> Result<()>;

    /// Returns rows IN_PROGRESS for longer than `older_than` (claimed by a
    /// processor that died) to PENDING (Go `RecoverStuck`). Returns how many.
    async fn recover_stuck(&self, older_than: Duration) -> Result<u64>;

    /// Initialize schema (create tables if not exists)
    async fn init_schema(&self) -> Result<()>;

    /// Get the table configuration
    fn table_config(&self) -> &OutboxTableConfig;
}

/// Parses a timestamp as the SDKs store it in text columns: RFC 3339
/// (`2026-09-25T10:00:00.123Z`) or SQL (`2026-09-25 10:00:00`, as Laravel
/// writes it, read as UTC). Unreadable values read as now: the processor
/// never decides anything on these, so they must not fail a claim.
pub fn parse_text_timestamp(raw: &str) -> DateTime<Utc> {
    if let Ok(t) = DateTime::parse_from_rfc3339(raw) {
        return t.with_timezone(&Utc);
    }
    for format in ["%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S%.f"] {
        if let Ok(t) = chrono::NaiveDateTime::parse_from_str(raw, format) {
            return t.and_utc();
        }
    }
    Utc::now()
}

/// A validated SQL identifier for a configured table name (letters, digits,
/// `_`, optionally schema-qualified with one `.`). Table names are
/// interpolated into SQL, so anything else is refused.
pub fn checked_table_name(table: &str) -> Result<&str> {
    let valid = !table.is_empty()
        && table.split('.').count() <= 2
        && table.split('.').all(|part| {
            !part.is_empty()
                && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && !part.starts_with(|c: char| c.is_ascii_digit())
        });
    if valid {
        Ok(table)
    } else {
        Err(anyhow::anyhow!("invalid outbox table name {table:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_table_config_default() {
        let config = OutboxTableConfig::default();
        assert_eq!(config.events_table, "outbox_messages");
        assert_eq!(config.unique_tables(), vec!["outbox_messages"]);
        assert_eq!(
            config.tables_with_types(),
            vec![("outbox_messages", OutboxItemType::ALL.to_vec())]
        );
    }

    #[test]
    fn separate_tables_each_carry_their_own_type() {
        let config = OutboxTableConfig {
            events_table: "a".into(),
            dispatch_jobs_table: "b".into(),
            audit_logs_table: "a".into(),
        };
        assert_eq!(config.unique_tables(), vec!["a", "b"]);
        assert_eq!(
            config.tables_with_types(),
            vec![
                ("a", vec![OutboxItemType::Event, OutboxItemType::AuditLog]),
                ("b", vec![OutboxItemType::DispatchJob]),
            ]
        );
    }

    #[test]
    fn a_row_with_a_bad_payload_is_invalid_not_an_error() {
        let now = Utc::now();
        let mut batch = ClaimedBatch::default();
        batch.push_row(
            "a".into(),
            OutboxItemType::Event,
            Some("g".into()),
            r#"{"type":"x"}"#,
            1,
            None,
            now,
            now,
        );
        batch.push_row(
            "b".into(),
            OutboxItemType::DispatchJob,
            Some(String::new()),
            "not json",
            0,
            None,
            now,
            now,
        );
        assert_eq!(batch.items.len(), 1);
        assert_eq!(batch.items[0].retry_count, 1);
        assert_eq!(batch.items[0].status, OutboxStatus::InProgress);
        assert_eq!(batch.invalid.len(), 1);
        assert_eq!(batch.invalid[0].id, "b");
        // An empty group is no group.
        assert_eq!(batch.invalid[0].message_group, None);
    }

    #[test]
    fn text_timestamps_in_either_sdk_format() {
        let rfc = parse_text_timestamp("2026-09-25T10:00:01.500Z");
        let sql = parse_text_timestamp("2026-09-25 10:00:01");
        assert_eq!(rfc.timestamp(), sql.timestamp());
    }

    #[test]
    fn table_names_are_identifiers() {
        assert!(checked_table_name("outbox_messages").is_ok());
        assert!(checked_table_name("app.outbox_messages").is_ok());
        for bad in ["", "a;drop", "a b", "1abc", "a.b.c", "a."] {
            assert!(checked_table_name(bad).is_err(), "{bad}");
        }
    }
}
