//! Unit of Work — PostgreSQL via SeaORM
//!
//! Atomic commit of entity state changes, domain events, and audit logs
//! within a single PostgreSQL transaction.

use async_trait::async_trait;
use chrono::Utc;
use sea_orm::{DatabaseConnection, DatabaseTransaction, EntityTrait, TransactionTrait, Set};
use serde::Serialize;
use tracing::{debug, error};

use super::domain_event::DomainEvent;
use super::error::UseCaseError;
use super::result::UseCaseResult;
use crate::entities::{msg_events, aud_logs};

// ─── Traits ──────────────────────────────────────────────────────────────────

/// Trait for entities that have a unique string ID.
pub trait HasId {
    fn id(&self) -> &str;
    /// Legacy collection name. Unused in PostgreSQL implementation.
    fn collection_name() -> &'static str where Self: Sized { "" }
}

/// Trait for domain entities that can be upserted/deleted within a PostgreSQL transaction.
///
/// Implement this for every aggregate that is passed to `UnitOfWork::commit`.
#[async_trait]
pub trait PgPersist: HasId + Send + Sync {
    /// Upsert the entity into the database within the given transaction.
    async fn pg_upsert(&self, txn: &DatabaseTransaction) -> crate::shared::error::Result<()>;

    /// Delete the entity from the database within the given transaction.
    async fn pg_delete(&self, txn: &DatabaseTransaction) -> crate::shared::error::Result<()>;
}

/// Trait for aggregates passed by value to `commit_all`.
/// Same as `PgPersist` but object-safe via `async_trait`.
#[async_trait]
pub trait PgAggregate: Send + Sync {
    fn id(&self) -> &str;
    async fn pg_upsert(&self, txn: &DatabaseTransaction) -> crate::shared::error::Result<()>;
}

// ─── UnitOfWork trait ────────────────────────────────────────────────────────

/// Unit of Work for atomic control plane operations.
///
/// Ensures entity state changes, domain events, and audit logs are committed
/// atomically in a single PostgreSQL transaction.
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
        T: Serialize + HasId + PgPersist + Send + Sync,
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
        T: Serialize + HasId + PgPersist + Send + Sync,
        C: Serialize + Send + Sync;

    /// Emit a domain event and audit log without an entity change.
    ///
    /// Used for events that don't modify an entity directly (e.g., UserLoggedIn).
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
        aggregates: Vec<Box<dyn PgAggregate>>,
        event: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        E: DomainEvent + Serialize + Send + 'static,
        C: Serialize + Send + Sync;
}

// ─── PgUnitOfWork ────────────────────────────────────────────────────────────

/// PostgreSQL implementation of `UnitOfWork` using SeaORM transactions.
#[derive(Clone)]
pub struct PgUnitOfWork {
    db: DatabaseConnection,
}

impl PgUnitOfWork {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    pub fn from_ref(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
    }

    // ── Subject parsing helpers ───────────────────────────────

    /// "platform.eventtype.123" → "Eventtype"
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

    /// "platform.eventtype.123" → Some("123")
    fn extract_entity_id(subject: &str) -> String {
        subject.split('.').nth(2).unwrap_or("").to_string()
    }

    // ── Builder helpers ───────────────────────────────────────

    fn build_event_model<E: DomainEvent>(event: &E) -> msg_events::ActiveModel {
        let data_json: serde_json::Value = serde_json::from_str(&event.to_data_json())
            .unwrap_or(serde_json::json!({}));

        let context_data = serde_json::json!([
            {"key": "principalId", "value": event.principal_id()},
            {"key": "aggregateType", "value": Self::extract_aggregate_type(event.subject())},
        ]);

        msg_events::ActiveModel {
            id: Set(event.event_id().to_string()),
            spec_version: Set(Some(event.spec_version().to_string())),
            event_type: Set(event.event_type().to_string()),
            source: Set(event.source().to_string()),
            subject: Set(Some(event.subject().to_string())),
            time: Set(event.time().into()),
            data: Set(Some(sea_orm::JsonValue::from(data_json))),
            correlation_id: Set(Some(event.correlation_id().to_string())),
            causation_id: Set(event.causation_id().map(String::from)),
            deduplication_id: Set(Some(format!("{}-{}", event.event_type(), event.event_id()))),
            message_group: Set(Some(event.message_group().to_string())),
            client_id: Set(None),
            context_data: Set(Some(sea_orm::JsonValue::from(context_data))),
            created_at: Set(Utc::now().into()),
        }
    }

    fn build_audit_model<E: DomainEvent, C: Serialize>(event: &E, command: &C) -> aud_logs::ActiveModel {
        let command_name = std::any::type_name::<C>()
            .rsplit("::")
            .next()
            .unwrap_or("Unknown")
            .to_string();

        let operation_json = serde_json::to_value(command).ok().map(sea_orm::JsonValue::from);

        aud_logs::ActiveModel {
            id: Set(crate::TsidGenerator::generate(crate::EntityType::AuditLog)),
            entity_type: Set(Self::extract_aggregate_type(event.subject())),
            entity_id: Set(Self::extract_entity_id(event.subject())),
            operation: Set(command_name),
            operation_json: Set(operation_json),
            principal_id: Set(Some(event.principal_id().to_string())),
            application_id: Set(None),
            client_id: Set(None),
            performed_at: Set(event.time().into()),
        }
    }

    async fn persist_event_and_audit<E: DomainEvent, C: Serialize>(
        txn: &DatabaseTransaction,
        event: &E,
        command: &C,
    ) -> Result<(), UseCaseError> {
        let event_model = Self::build_event_model(event);
        if let Err(e) = msg_events::Entity::insert(event_model).exec(txn).await {
            error!("Failed to insert domain event: {}", e);
            return Err(UseCaseError::commit(format!("Failed to insert domain event: {}", e)));
        }

        let audit_model = Self::build_audit_model(event, command);
        if let Err(e) = aud_logs::Entity::insert(audit_model).exec(txn).await {
            error!("Failed to insert audit log: {}", e);
            return Err(UseCaseError::commit(format!("Failed to insert audit log: {}", e)));
        }

        Ok(())
    }
}

#[async_trait]
impl UnitOfWork for PgUnitOfWork {
    async fn commit<E, T, C>(
        &self,
        aggregate: &T,
        event: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        E: DomainEvent + Serialize + Send + 'static,
        T: Serialize + HasId + PgPersist + Send + Sync,
        C: Serialize + Send + Sync,
    {
        let txn = match self.db.begin().await {
            Ok(t) => t,
            Err(e) => {
                error!("Failed to start transaction: {}", e);
                return UseCaseResult::failure(UseCaseError::commit(format!("Failed to start transaction: {}", e)));
            }
        };

        if let Err(e) = aggregate.pg_upsert(&txn).await {
            let _ = txn.rollback().await;
            error!("Failed to persist aggregate: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(format!("Failed to persist aggregate: {}", e)));
        }

        if let Err(e) = Self::persist_event_and_audit(&txn, &event, command).await {
            let _ = txn.rollback().await;
            return UseCaseResult::failure(e);
        }

        if let Err(e) = txn.commit().await {
            error!("Failed to commit transaction: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(format!("Failed to commit transaction: {}", e)));
        }

        debug!(
            event_id = event.event_id(),
            event_type = event.event_type(),
            "Successfully committed transaction"
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
        T: Serialize + HasId + PgPersist + Send + Sync,
        C: Serialize + Send + Sync,
    {
        let txn = match self.db.begin().await {
            Ok(t) => t,
            Err(e) => {
                error!("Failed to start transaction: {}", e);
                return UseCaseResult::failure(UseCaseError::commit(format!("Failed to start transaction: {}", e)));
            }
        };

        if let Err(e) = aggregate.pg_delete(&txn).await {
            let _ = txn.rollback().await;
            error!("Failed to delete aggregate: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(format!("Failed to delete aggregate: {}", e)));
        }

        if let Err(e) = Self::persist_event_and_audit(&txn, &event, command).await {
            let _ = txn.rollback().await;
            return UseCaseResult::failure(e);
        }

        if let Err(e) = txn.commit().await {
            error!("Failed to commit transaction: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(format!("Failed to commit transaction: {}", e)));
        }

        debug!(
            event_id = event.event_id(),
            event_type = event.event_type(),
            "Successfully committed delete transaction"
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
        let txn = match self.db.begin().await {
            Ok(t) => t,
            Err(e) => {
                error!("Failed to start transaction: {}", e);
                return UseCaseResult::failure(UseCaseError::commit(format!("Failed to start transaction: {}", e)));
            }
        };

        if let Err(e) = Self::persist_event_and_audit(&txn, &event, command).await {
            let _ = txn.rollback().await;
            return UseCaseResult::failure(e);
        }

        if let Err(e) = txn.commit().await {
            error!("Failed to commit transaction: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(format!("Failed to commit transaction: {}", e)));
        }

        debug!(
            event_id = event.event_id(),
            event_type = event.event_type(),
            "Successfully emitted domain event"
        );

        UseCaseResult::success(event)
    }

    async fn commit_all<E, C>(
        &self,
        aggregates: Vec<Box<dyn PgAggregate>>,
        event: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        E: DomainEvent + Serialize + Send + 'static,
        C: Serialize + Send + Sync,
    {
        let txn = match self.db.begin().await {
            Ok(t) => t,
            Err(e) => {
                error!("Failed to start transaction: {}", e);
                return UseCaseResult::failure(UseCaseError::commit(format!("Failed to start transaction: {}", e)));
            }
        };

        for aggregate in &aggregates {
            if let Err(e) = aggregate.pg_upsert(&txn).await {
                let _ = txn.rollback().await;
                error!("Failed to persist aggregate: {}", e);
                return UseCaseResult::failure(UseCaseError::commit(format!("Failed to persist aggregate: {}", e)));
            }
        }

        if let Err(e) = Self::persist_event_and_audit(&txn, &event, command).await {
            let _ = txn.rollback().await;
            return UseCaseResult::failure(e);
        }

        if let Err(e) = txn.commit().await {
            error!("Failed to commit transaction: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(format!("Failed to commit transaction: {}", e)));
        }

        debug!(
            event_id = event.event_id(),
            event_type = event.event_type(),
            aggregate_count = aggregates.len(),
            "Successfully committed multi-aggregate transaction"
        );

        UseCaseResult::success(event)
    }
}

// ─── InMemory (tests) ─────────────────────────────────────────────────────────

#[cfg(test)]
pub struct InMemoryUnitOfWork {
    pub committed_events: std::sync::Mutex<Vec<String>>,
}

#[cfg(test)]
impl InMemoryUnitOfWork {
    pub fn new() -> Self {
        Self { committed_events: std::sync::Mutex::new(Vec::new()) }
    }
}

#[cfg(test)]
#[async_trait]
impl UnitOfWork for InMemoryUnitOfWork {
    async fn commit<E, T, C>(&self, _aggregate: &T, event: E, _command: &C) -> UseCaseResult<E>
    where
        E: DomainEvent + Serialize + Send + 'static,
        T: Serialize + HasId + PgPersist + Send + Sync,
        C: Serialize + Send + Sync,
    {
        self.committed_events.lock().unwrap().push(event.event_id().to_string());
        UseCaseResult::success(event)
    }

    async fn commit_delete<E, T, C>(&self, _aggregate: &T, event: E, _command: &C) -> UseCaseResult<E>
    where
        E: DomainEvent + Serialize + Send + 'static,
        T: Serialize + HasId + PgPersist + Send + Sync,
        C: Serialize + Send + Sync,
    {
        self.committed_events.lock().unwrap().push(event.event_id().to_string());
        UseCaseResult::success(event)
    }

    async fn emit_event<E, C>(&self, event: E, _command: &C) -> UseCaseResult<E>
    where
        E: DomainEvent + Serialize + Send + 'static,
        C: Serialize + Send + Sync,
    {
        self.committed_events.lock().unwrap().push(event.event_id().to_string());
        UseCaseResult::success(event)
    }

    async fn commit_all<E, C>(&self, _aggregates: Vec<Box<dyn PgAggregate>>, event: E, _command: &C) -> UseCaseResult<E>
    where
        E: DomainEvent + Serialize + Send + 'static,
        C: Serialize + Send + Sync,
    {
        self.committed_events.lock().unwrap().push(event.event_id().to_string());
        UseCaseResult::success(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_aggregate_type() {
        assert_eq!(PgUnitOfWork::extract_aggregate_type("platform.eventtype.123"), "Eventtype");
        assert_eq!(PgUnitOfWork::extract_aggregate_type("platform.user.abc"), "User");
        assert_eq!(PgUnitOfWork::extract_aggregate_type(""), "Unknown");
    }

    #[test]
    fn test_extract_entity_id() {
        assert_eq!(PgUnitOfWork::extract_entity_id("platform.user.123"), "123");
        assert_eq!(PgUnitOfWork::extract_entity_id("platform.user"), "");
    }
}
