//! Outbox-backed Unit of Work
//!
//! Instead of writing directly to `msg_events` and `aud_logs` (platform-owned tables),
//! this UnitOfWork writes outbox items to the `outbox_messages` table in the
//! consumer's database. The outbox poller (fc-outbox-processor) then forwards
//! these to the FlowCatalyst platform API.
//!
//! Entity persistence and outbox writes happen in the **same transaction**,
//! ensuring exactly-once semantics.

use async_trait::async_trait;
use serde::Serialize;
use sqlx::{PgPool, Postgres, Transaction};
use tracing::{debug, error};

use crate::tsid::{EntityType, TsidGenerator};
use crate::usecase::domain_event::DomainEvent;
use crate::usecase::error::UseCaseError;
use crate::usecase::result::UseCaseResult;

// ─── Traits ──────────────────────────────────────────────────────────────────

/// Trait for entities that have a unique string ID.
pub trait HasId {
    fn id(&self) -> &str;
}

/// Trait for domain entities that can be persisted within a PostgreSQL transaction.
///
/// Implement this for every aggregate passed to `UnitOfWork::commit`.
///
/// # Example
///
/// ```ignore
/// use fc_sdk::outbox::{Persist, HasId};
/// use sqlx::{Postgres, Transaction};
///
/// struct Order { id: String, customer_id: String, total: f64 }
///
/// impl HasId for Order {
///     fn id(&self) -> &str { &self.id }
/// }
///
/// #[async_trait::async_trait]
/// impl Persist for Order {
///     async fn upsert(&self, txn: &mut Transaction<'_, Postgres>) -> anyhow::Result<()> {
///         sqlx::query("INSERT INTO orders (id, customer_id, total) VALUES ($1, $2, $3)
///                      ON CONFLICT (id) DO UPDATE SET customer_id = $2, total = $3")
///             .bind(&self.id)
///             .bind(&self.customer_id)
///             .bind(self.total)
///             .execute(&mut **txn)
///             .await?;
///         Ok(())
///     }
///
///     async fn delete(&self, txn: &mut Transaction<'_, Postgres>) -> anyhow::Result<()> {
///         sqlx::query("DELETE FROM orders WHERE id = $1")
///             .bind(&self.id)
///             .execute(&mut **txn)
///             .await?;
///         Ok(())
///     }
/// }
/// ```
#[async_trait]
pub trait Persist: HasId + Send + Sync {
    /// Upsert the entity within the given transaction.
    async fn upsert(&self, txn: &mut Transaction<'_, Postgres>) -> anyhow::Result<()>;

    /// Delete the entity within the given transaction.
    async fn delete(&self, txn: &mut Transaction<'_, Postgres>) -> anyhow::Result<()>;
}

/// Trait for aggregates passed by value to `commit_all`.
#[async_trait]
pub trait Aggregate: Send + Sync {
    fn id(&self) -> &str;
    async fn upsert(&self, txn: &mut Transaction<'_, Postgres>) -> anyhow::Result<()>;
}

// ─── UnitOfWork trait ────────────────────────────────────────────────────────

/// Unit of Work for atomic domain operations.
///
/// Ensures entity state changes, domain events, and audit logs are committed
/// atomically. Events and audit logs are written as outbox items for
/// asynchronous delivery to the FlowCatalyst platform.
#[async_trait]
pub trait UnitOfWork: Send + Sync {
    /// Commit an entity upsert with its domain event and audit log.
    async fn commit<E, T, C>(
        &self,
        aggregate: &T,
        event: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        E: DomainEvent + Serialize + Send + 'static,
        T: Serialize + Persist + Send + Sync,
        C: Serialize + Send + Sync;

    /// Commit an entity delete with its domain event and audit log.
    async fn commit_delete<E, T, C>(
        &self,
        aggregate: &T,
        event: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        E: DomainEvent + Serialize + Send + 'static,
        T: Serialize + Persist + Send + Sync,
        C: Serialize + Send + Sync;

    /// Emit a domain event and audit log without an entity change.
    async fn emit_event<E, C>(
        &self,
        event: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        E: DomainEvent + Serialize + Send + 'static,
        C: Serialize + Send + Sync;

    /// Commit multiple entity upserts with a single domain event and audit log.
    async fn commit_all<E, C>(
        &self,
        aggregates: Vec<Box<dyn Aggregate>>,
        event: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        E: DomainEvent + Serialize + Send + 'static,
        C: Serialize + Send + Sync;
}

// ─── OutboxUnitOfWork ────────────────────────────────────────────────────────

/// Configuration for the outbox unit of work.
#[derive(Debug, Clone)]
pub struct OutboxConfig {
    /// Table name for outbox messages (default: "outbox_messages")
    pub table_name: String,
    /// Optional client_id for multi-tenant scoping
    pub client_id: Option<String>,
}

impl Default for OutboxConfig {
    fn default() -> Self {
        Self {
            table_name: "outbox_messages".to_string(),
            client_id: None,
        }
    }
}

/// Outbox-backed implementation of [`UnitOfWork`].
///
/// Writes domain events and audit logs as outbox items to the `outbox_messages`
/// table, which the fc-outbox-processor polls and forwards to the FlowCatalyst
/// platform API.
///
/// # Example
///
/// ```ignore
/// use fc_sdk::outbox::OutboxUnitOfWork;
///
/// let pool = sqlx::PgPool::connect("postgresql://localhost/myapp").await?;
/// let uow = OutboxUnitOfWork::new(pool);
///
/// // Use in a use case
/// let result = uow.commit(&order, order_created_event, &create_command).await;
/// ```
#[derive(Clone)]
pub struct OutboxUnitOfWork {
    pool: PgPool,
    config: OutboxConfig,
}

impl OutboxUnitOfWork {
    /// Create a new OutboxUnitOfWork with default configuration.
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            config: OutboxConfig::default(),
        }
    }

    /// Create a new OutboxUnitOfWork with custom configuration.
    pub fn with_config(pool: PgPool, config: OutboxConfig) -> Self {
        Self { pool, config }
    }

    /// "domain.aggregate.123" → "Aggregate"
    fn extract_aggregate_type(subject: &str) -> String {
        subject
            .split('.')
            .nth(1)
            .map(|s| {
                let mut chars = s.chars();
                match chars.next() {
                    Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            })
            .unwrap_or_else(|| "Unknown".to_string())
    }

    /// "domain.aggregate.123" → "123"
    fn extract_entity_id(subject: &str) -> String {
        subject.split('.').nth(2).unwrap_or("").to_string()
    }

    /// Write the event outbox item into the transaction.
    async fn write_event_outbox<E: DomainEvent + Serialize>(
        txn: &mut Transaction<'_, Postgres>,
        table: &str,
        event: &E,
        client_id: &Option<String>,
    ) -> Result<(), UseCaseError> {
        let id = TsidGenerator::generate(EntityType::Event);
        let data_json: serde_json::Value = serde_json::from_str(&event.to_data_json())
            .unwrap_or(serde_json::json!({}));

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
                {"key": "aggregateType", "value": Self::extract_aggregate_type(event.subject())},
            ],
        });

        let payload_str = payload.to_string();
        let payload_size = payload_str.len() as i32;

        let query = format!(
            "INSERT INTO {} (id, type, message_group, payload, status, retry_count, created_at, updated_at, client_id, payload_size) \
             VALUES ($1, 'EVENT', $2, $3, 0, 0, NOW(), NOW(), $4, $5)",
            table
        );

        if let Err(e) = sqlx::query(&query)
            .bind(&id)
            .bind(event.message_group())
            .bind(&payload)
            .bind(client_id.as_deref())
            .bind(payload_size)
            .execute(&mut **txn)
            .await
        {
            error!("Failed to write event outbox item: {}", e);
            return Err(UseCaseError::commit(format!(
                "Failed to write event outbox item: {}",
                e
            )));
        }

        Ok(())
    }

    /// Write the audit log outbox item into the transaction.
    async fn write_audit_outbox<E: DomainEvent, C: Serialize>(
        txn: &mut Transaction<'_, Postgres>,
        table: &str,
        event: &E,
        command: &C,
        client_id: &Option<String>,
    ) -> Result<(), UseCaseError> {
        let id = TsidGenerator::generate(EntityType::AuditLog);

        let command_name = std::any::type_name::<C>()
            .rsplit("::")
            .next()
            .unwrap_or("Unknown")
            .to_string();

        let operation_json = serde_json::to_value(command).ok();

        let payload = serde_json::json!({
            "entity_type": Self::extract_aggregate_type(event.subject()),
            "entity_id": Self::extract_entity_id(event.subject()),
            "operation": command_name,
            "operation_json": operation_json,
            "principal_id": event.principal_id(),
            "performed_at": event.time().to_rfc3339(),
        });

        let payload_size = payload.to_string().len() as i32;

        let query = format!(
            "INSERT INTO {} (id, type, message_group, payload, status, retry_count, created_at, updated_at, client_id, payload_size) \
             VALUES ($1, 'AUDIT_LOG', $2, $3, 0, 0, NOW(), NOW(), $4, $5)",
            table
        );

        if let Err(e) = sqlx::query(&query)
            .bind(&id)
            .bind(event.message_group())
            .bind(&payload)
            .bind(client_id.as_deref())
            .bind(payload_size)
            .execute(&mut **txn)
            .await
        {
            error!("Failed to write audit outbox item: {}", e);
            return Err(UseCaseError::commit(format!(
                "Failed to write audit outbox item: {}",
                e
            )));
        }

        Ok(())
    }

    async fn persist_outbox_items<E: DomainEvent + Serialize, C: Serialize>(
        txn: &mut Transaction<'_, Postgres>,
        table: &str,
        event: &E,
        command: &C,
        client_id: &Option<String>,
    ) -> Result<(), UseCaseError> {
        Self::write_event_outbox(txn, table, event, client_id).await?;
        Self::write_audit_outbox(txn, table, event, command, client_id).await?;
        Ok(())
    }
}

#[async_trait]
impl UnitOfWork for OutboxUnitOfWork {
    async fn commit<E, T, C>(
        &self,
        aggregate: &T,
        event: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        E: DomainEvent + Serialize + Send + 'static,
        T: Serialize + Persist + Send + Sync,
        C: Serialize + Send + Sync,
    {
        let mut txn = match self.pool.begin().await {
            Ok(t) => t,
            Err(e) => {
                error!("Failed to start transaction: {}", e);
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to start transaction: {}",
                    e
                )));
            }
        };

        if let Err(e) = aggregate.upsert(&mut txn).await {
            error!("Failed to persist aggregate: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(format!(
                "Failed to persist aggregate: {}",
                e
            )));
        }

        if let Err(e) = Self::persist_outbox_items(
            &mut txn,
            &self.config.table_name,
            &event,
            command,
            &self.config.client_id,
        )
        .await
        {
            return UseCaseResult::failure(e);
        }

        if let Err(e) = txn.commit().await {
            error!("Failed to commit transaction: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(format!(
                "Failed to commit transaction: {}",
                e
            )));
        }

        debug!(
            event_id = event.event_id(),
            event_type = event.event_type(),
            "Committed entity + outbox items"
        );

        UseCaseResult::success(event)
    }

    async fn commit_delete<E, T, C>(
        &self,
        aggregate: &T,
        event: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        E: DomainEvent + Serialize + Send + 'static,
        T: Serialize + Persist + Send + Sync,
        C: Serialize + Send + Sync,
    {
        let mut txn = match self.pool.begin().await {
            Ok(t) => t,
            Err(e) => {
                error!("Failed to start transaction: {}", e);
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to start transaction: {}",
                    e
                )));
            }
        };

        if let Err(e) = aggregate.delete(&mut txn).await {
            error!("Failed to delete aggregate: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(format!(
                "Failed to delete aggregate: {}",
                e
            )));
        }

        if let Err(e) = Self::persist_outbox_items(
            &mut txn,
            &self.config.table_name,
            &event,
            command,
            &self.config.client_id,
        )
        .await
        {
            return UseCaseResult::failure(e);
        }

        if let Err(e) = txn.commit().await {
            error!("Failed to commit transaction: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(format!(
                "Failed to commit transaction: {}",
                e
            )));
        }

        debug!(
            event_id = event.event_id(),
            event_type = event.event_type(),
            "Committed delete + outbox items"
        );

        UseCaseResult::success(event)
    }

    async fn emit_event<E, C>(
        &self,
        event: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        E: DomainEvent + Serialize + Send + 'static,
        C: Serialize + Send + Sync,
    {
        let mut txn = match self.pool.begin().await {
            Ok(t) => t,
            Err(e) => {
                error!("Failed to start transaction: {}", e);
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to start transaction: {}",
                    e
                )));
            }
        };

        if let Err(e) = Self::persist_outbox_items(
            &mut txn,
            &self.config.table_name,
            &event,
            command,
            &self.config.client_id,
        )
        .await
        {
            return UseCaseResult::failure(e);
        }

        if let Err(e) = txn.commit().await {
            error!("Failed to commit transaction: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(format!(
                "Failed to commit transaction: {}",
                e
            )));
        }

        debug!(
            event_id = event.event_id(),
            event_type = event.event_type(),
            "Emitted event via outbox"
        );

        UseCaseResult::success(event)
    }

    async fn commit_all<E, C>(
        &self,
        aggregates: Vec<Box<dyn Aggregate>>,
        event: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        E: DomainEvent + Serialize + Send + 'static,
        C: Serialize + Send + Sync,
    {
        let mut txn = match self.pool.begin().await {
            Ok(t) => t,
            Err(e) => {
                error!("Failed to start transaction: {}", e);
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to start transaction: {}",
                    e
                )));
            }
        };

        for aggregate in &aggregates {
            if let Err(e) = aggregate.upsert(&mut txn).await {
                error!("Failed to persist aggregate: {}", e);
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to persist aggregate: {}",
                    e
                )));
            }
        }

        if let Err(e) = Self::persist_outbox_items(
            &mut txn,
            &self.config.table_name,
            &event,
            command,
            &self.config.client_id,
        )
        .await
        {
            return UseCaseResult::failure(e);
        }

        if let Err(e) = txn.commit().await {
            error!("Failed to commit transaction: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(format!(
                "Failed to commit transaction: {}",
                e
            )));
        }

        debug!(
            event_id = event.event_id(),
            event_type = event.event_type(),
            aggregate_count = aggregates.len(),
            "Committed multi-aggregate + outbox items"
        );

        UseCaseResult::success(event)
    }
}
