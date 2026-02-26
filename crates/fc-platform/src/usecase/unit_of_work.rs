//! Unit of Work
//!
//! Atomic commit of entity state changes, domain events, and audit logs
//! within a single MongoDB transaction.

use async_trait::async_trait;
use chrono::Utc;
use mongodb::{
    Client, Database,
    bson::{doc, Document, to_document},
};
use serde::Serialize;
use tracing::{debug, error};

use super::domain_event::DomainEvent;
use super::error::UseCaseError;
use super::result::UseCaseResult;
use crate::{Event, ContextData, AuditLog};

/// Unit of Work for atomic control plane operations.
///
/// Ensures that entity state changes, domain events, and audit logs are
/// committed atomically within a single MongoDB transaction.
///
/// **This is the ONLY way to create a successful `UseCaseResult`.**
/// The `UseCaseResult::success()` method is crate-private, so use cases
/// must go through UnitOfWork to return success. This guarantees that:
/// - Domain events are always emitted when state changes
/// - Audit logs are always created for operations
/// - Entity state and events are consistent (atomic commit)
///
/// # Usage in a use case:
///
/// ```ignore
/// pub async fn execute(&self, cmd: CreateEventTypeCommand, ctx: ExecutionContext)
///     -> UseCaseResult<EventTypeCreated>
/// {
///     // Validation - can return failure directly
///     if !is_valid(&cmd) {
///         return UseCaseResult::failure(UseCaseError::validation("INVALID", "..."));
///     }
///
///     // Create aggregate
///     let event_type = EventType::new(...);
///
///     // Create domain event
///     let event = EventTypeCreated::new(&ctx, &event_type);
///
///     // Atomic commit - only way to return success
///     self.unit_of_work.commit(&event_type, event, &cmd).await
/// }
/// ```
#[async_trait]
pub trait UnitOfWork: Send + Sync {
    /// Commit an entity change with its domain event atomically.
    ///
    /// Within a single MongoDB transaction:
    /// 1. Persists or updates the aggregate entity
    /// 2. Creates the domain event in the events collection
    /// 3. Creates the audit log entry
    ///
    /// If any step fails, the entire transaction is rolled back.
    async fn commit<E, T, C>(
        &self,
        aggregate: &T,
        event: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        E: DomainEvent + Serialize + Send + 'static,
        T: Serialize + HasId + Send + Sync,
        C: Serialize + Send + Sync;

    /// Commit a delete operation with its domain event atomically.
    ///
    /// Within a single MongoDB transaction:
    /// 1. Deletes the aggregate entity
    /// 2. Creates the domain event in the events collection
    /// 3. Creates the audit log entry
    async fn commit_delete<E, T, C>(
        &self,
        aggregate: &T,
        event: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        E: DomainEvent + Serialize + Send + 'static,
        T: Serialize + HasId + Send + Sync,
        C: Serialize + Send + Sync;

    /// Commit multiple entity changes with a domain event atomically.
    ///
    /// Use this for operations that create or update multiple aggregates,
    /// such as provisioning a service account (Principal + OAuthClient + Application).
    async fn commit_all<E, C>(
        &self,
        aggregates: Vec<Box<dyn SerializableAggregate>>,
        event: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        E: DomainEvent + Serialize + Send + 'static,
        C: Serialize + Send + Sync;
}

/// Trait for entities that have an ID field.
pub trait HasId {
    fn id(&self) -> &str;
    fn collection_name() -> &'static str;
}

/// Trait for serializable aggregates with collection info.
pub trait SerializableAggregate: Send + Sync {
    fn id(&self) -> &str;
    fn collection_name(&self) -> &str;
    fn to_document(&self) -> Result<Document, mongodb::bson::ser::Error>;
}

/// MongoDB implementation of UnitOfWork using multi-document transactions.
///
/// # Requirements:
/// - MongoDB 4.0+ (for multi-document transactions)
/// - Replica set deployment (transactions require replica set)
/// - Aggregates must implement `HasId` trait
#[derive(Clone)]
pub struct MongoUnitOfWork {
    client: Client,
    database: Database,
}

impl MongoUnitOfWork {
    pub fn new(client: Client, database: Database) -> Self {
        Self { client, database }
    }

    /// Extract aggregate type from subject string.
    /// Subject format: "platform.eventtype.123456789"
    /// Returns: "Eventtype" (capitalized)
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

    /// Extract entity ID from subject string.
    /// Subject format: "platform.eventtype.123456789"
    fn extract_entity_id(subject: &str) -> Option<String> {
        subject.split('.').nth(2).map(String::from)
    }

    /// Create an Event entity from a DomainEvent.
    fn create_event<E: DomainEvent>(event: &E) -> Event {
        let data_json = event.to_data_json();
        let data: serde_json::Value = serde_json::from_str(&data_json)
            .unwrap_or(serde_json::json!({}));

        Event {
            id: event.event_id().to_string(),
            event_type: event.event_type().to_string(),
            source: event.source().to_string(),
            subject: Some(event.subject().to_string()),
            time: event.time(),
            data,
            data_content_type: "application/json".to_string(),
            spec_version: event.spec_version().to_string(),
            message_group: Some(event.message_group().to_string()),
            correlation_id: Some(event.correlation_id().to_string()),
            causation_id: event.causation_id().map(String::from),
            deduplication_id: Some(format!("{}-{}", event.event_type(), event.event_id())),
            client_id: None, // Will be set by caller if needed
            context_data: vec![
                ContextData {
                    key: "principalId".to_string(),
                    value: event.principal_id().to_string(),
                },
                ContextData {
                    key: "aggregateType".to_string(),
                    value: Self::extract_aggregate_type(event.subject()),
                },
            ],
            created_at: Utc::now(),
        }
    }

    /// Create an AuditLog entry from a command and event (matches Java schema).
    fn create_audit_log<E: DomainEvent, C: Serialize>(
        event: &E,
        command: &C,
    ) -> AuditLog {
        let command_name = std::any::type_name::<C>()
            .rsplit("::")
            .next()
            .unwrap_or("Unknown")
            .to_string();

        let operation_json = serde_json::to_string(command).ok();

        AuditLog::new(
            Self::extract_aggregate_type(event.subject()),
            Self::extract_entity_id(event.subject()),
            command_name,
            operation_json,
            Some(event.principal_id().to_string()),
        ).with_performed_at(event.time())
    }
}

#[async_trait]
impl UnitOfWork for MongoUnitOfWork {
    async fn commit<E, T, C>(
        &self,
        aggregate: &T,
        event: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        E: DomainEvent + Serialize + Send + 'static,
        T: Serialize + HasId + Send + Sync,
        C: Serialize + Send + Sync,
    {
        let mut session = match self.client.start_session().await {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to start MongoDB session: {}", e);
                return UseCaseResult::failure(UseCaseError::commit(
                    format!("Failed to start session: {}", e)
                ));
            }
        };

        if let Err(e) = session.start_transaction().await {
            error!("Failed to start transaction: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(
                format!("Failed to start transaction: {}", e)
            ));
        }

        // 1. Persist aggregate
        let collection_name = T::collection_name();
        let collection = self.database.collection::<Document>(collection_name);
        let aggregate_doc = match to_document(aggregate) {
            Ok(d) => d,
            Err(e) => {
                let _ = session.abort_transaction().await;
                return UseCaseResult::failure(UseCaseError::commit(
                    format!("Failed to serialize aggregate: {}", e)
                ));
            }
        };

        let id = aggregate.id();

        // Use update with $set for upsert semantics
        let update_result = collection
            .update_one(
                doc! { "_id": id },
                doc! { "$set": &aggregate_doc },
            )
            .upsert(true)
            .session(&mut session)
            .await;

        if let Err(e) = update_result {
            let _ = session.abort_transaction().await;
            error!("Failed to persist aggregate: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(
                format!("Failed to persist aggregate: {}", e)
            ));
        }

        // 2. Create domain event
        let mongo_event = Self::create_event(&event);
        let events_collection = self.database.collection::<Event>("events");
        if let Err(e) = events_collection
            .insert_one(&mongo_event)
            .session(&mut session)
            .await
        {
            let _ = session.abort_transaction().await;
            error!("Failed to insert event: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(
                format!("Failed to insert event: {}", e)
            ));
        }

        // 3. Create audit log
        let audit_log = Self::create_audit_log(&event, command);
        let audit_collection = self.database.collection::<AuditLog>("audit_logs");
        if let Err(e) = audit_collection
            .insert_one(&audit_log)
            .session(&mut session)
            .await
        {
            let _ = session.abort_transaction().await;
            error!("Failed to insert audit log: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(
                format!("Failed to insert audit log: {}", e)
            ));
        }

        // Commit transaction
        if let Err(e) = session.commit_transaction().await {
            error!("Failed to commit transaction: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(
                format!("Failed to commit transaction: {}", e)
            ));
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
        T: Serialize + HasId + Send + Sync,
        C: Serialize + Send + Sync,
    {
        let mut session = match self.client.start_session().await {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to start MongoDB session: {}", e);
                return UseCaseResult::failure(UseCaseError::commit(
                    format!("Failed to start session: {}", e)
                ));
            }
        };

        if let Err(e) = session.start_transaction().await {
            error!("Failed to start transaction: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(
                format!("Failed to start transaction: {}", e)
            ));
        }

        // 1. Delete aggregate
        let collection_name = T::collection_name();
        let collection = self.database.collection::<Document>(collection_name);
        let id = aggregate.id();

        if let Err(e) = collection
            .delete_one(doc! { "_id": id })
            .session(&mut session)
            .await
        {
            let _ = session.abort_transaction().await;
            error!("Failed to delete aggregate: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(
                format!("Failed to delete aggregate: {}", e)
            ));
        }

        // 2. Create domain event
        let mongo_event = Self::create_event(&event);
        let events_collection = self.database.collection::<Event>("events");
        if let Err(e) = events_collection
            .insert_one(&mongo_event)
            .session(&mut session)
            .await
        {
            let _ = session.abort_transaction().await;
            error!("Failed to insert event: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(
                format!("Failed to insert event: {}", e)
            ));
        }

        // 3. Create audit log
        let audit_log = Self::create_audit_log(&event, command);
        let audit_collection = self.database.collection::<AuditLog>("audit_logs");
        if let Err(e) = audit_collection
            .insert_one(&audit_log)
            .session(&mut session)
            .await
        {
            let _ = session.abort_transaction().await;
            error!("Failed to insert audit log: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(
                format!("Failed to insert audit log: {}", e)
            ));
        }

        // Commit transaction
        if let Err(e) = session.commit_transaction().await {
            error!("Failed to commit transaction: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(
                format!("Failed to commit transaction: {}", e)
            ));
        }

        debug!(
            event_id = event.event_id(),
            event_type = event.event_type(),
            "Successfully committed delete transaction"
        );

        UseCaseResult::success(event)
    }

    async fn commit_all<E, C>(
        &self,
        aggregates: Vec<Box<dyn SerializableAggregate>>,
        event: E,
        command: &C,
    ) -> UseCaseResult<E>
    where
        E: DomainEvent + Serialize + Send + 'static,
        C: Serialize + Send + Sync,
    {
        let mut session = match self.client.start_session().await {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to start MongoDB session: {}", e);
                return UseCaseResult::failure(UseCaseError::commit(
                    format!("Failed to start session: {}", e)
                ));
            }
        };

        if let Err(e) = session.start_transaction().await {
            error!("Failed to start transaction: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(
                format!("Failed to start transaction: {}", e)
            ));
        }

        // 1. Persist all aggregates
        for aggregate in &aggregates {
            let collection_name = aggregate.collection_name();
            let collection = self.database.collection::<Document>(collection_name);

            let aggregate_doc = match aggregate.to_document() {
                Ok(d) => d,
                Err(e) => {
                    let _ = session.abort_transaction().await;
                    return UseCaseResult::failure(UseCaseError::commit(
                        format!("Failed to serialize aggregate: {}", e)
                    ));
                }
            };

            let id = aggregate.id();
            let update_result = collection
                .update_one(
                    doc! { "_id": id },
                    doc! { "$set": &aggregate_doc },
                )
                .with_options(mongodb::options::UpdateOptions::builder().upsert(true).build())
                .session(&mut session)
                .await;

            if let Err(e) = update_result {
                let _ = session.abort_transaction().await;
                error!("Failed to persist aggregate: {}", e);
                return UseCaseResult::failure(UseCaseError::commit(
                    format!("Failed to persist aggregate: {}", e)
                ));
            }
        }

        // 2. Create domain event
        let mongo_event = Self::create_event(&event);
        let events_collection = self.database.collection::<Event>("events");
        if let Err(e) = events_collection
            .insert_one(&mongo_event)
            .session(&mut session)
            .await
        {
            let _ = session.abort_transaction().await;
            error!("Failed to insert event: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(
                format!("Failed to insert event: {}", e)
            ));
        }

        // 3. Create audit log
        let audit_log = Self::create_audit_log(&event, command);
        let audit_collection = self.database.collection::<AuditLog>("audit_logs");
        if let Err(e) = audit_collection
            .insert_one(&audit_log)
            .session(&mut session)
            .await
        {
            let _ = session.abort_transaction().await;
            error!("Failed to insert audit log: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(
                format!("Failed to insert audit log: {}", e)
            ));
        }

        // Commit transaction
        if let Err(e) = session.commit_transaction().await {
            error!("Failed to commit transaction: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(
                format!("Failed to commit transaction: {}", e)
            ));
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

/// In-memory UnitOfWork for testing.
#[cfg(test)]
pub struct InMemoryUnitOfWork {
    pub committed_events: std::sync::Mutex<Vec<String>>,
    pub committed_audit_logs: std::sync::Mutex<Vec<String>>,
}

#[cfg(test)]
impl InMemoryUnitOfWork {
    pub fn new() -> Self {
        Self {
            committed_events: std::sync::Mutex::new(Vec::new()),
            committed_audit_logs: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[cfg(test)]
#[async_trait]
impl UnitOfWork for InMemoryUnitOfWork {
    async fn commit<E, T, C>(
        &self,
        _aggregate: &T,
        event: E,
        _command: &C,
    ) -> UseCaseResult<E>
    where
        E: DomainEvent + Serialize + Send + 'static,
        T: Serialize + HasId + Send + Sync,
        C: Serialize + Send + Sync,
    {
        self.committed_events
            .lock()
            .unwrap()
            .push(event.event_id().to_string());
        self.committed_audit_logs
            .lock()
            .unwrap()
            .push(format!("{}-audit", event.event_id()));
        UseCaseResult::success(event)
    }

    async fn commit_delete<E, T, C>(
        &self,
        _aggregate: &T,
        event: E,
        _command: &C,
    ) -> UseCaseResult<E>
    where
        E: DomainEvent + Serialize + Send + 'static,
        T: Serialize + HasId + Send + Sync,
        C: Serialize + Send + Sync,
    {
        self.committed_events
            .lock()
            .unwrap()
            .push(event.event_id().to_string());
        UseCaseResult::success(event)
    }

    async fn commit_all<E, C>(
        &self,
        _aggregates: Vec<Box<dyn SerializableAggregate>>,
        event: E,
        _command: &C,
    ) -> UseCaseResult<E>
    where
        E: DomainEvent + Serialize + Send + 'static,
        C: Serialize + Send + Sync,
    {
        self.committed_events
            .lock()
            .unwrap()
            .push(event.event_id().to_string());
        UseCaseResult::success(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_aggregate_type() {
        assert_eq!(
            MongoUnitOfWork::extract_aggregate_type("platform.eventtype.123"),
            "Eventtype"
        );
        assert_eq!(
            MongoUnitOfWork::extract_aggregate_type("platform.user.abc"),
            "User"
        );
        assert_eq!(
            MongoUnitOfWork::extract_aggregate_type(""),
            "Unknown"
        );
    }

    #[test]
    fn test_extract_entity_id() {
        assert_eq!(
            MongoUnitOfWork::extract_entity_id("platform.user.123"),
            Some("123".to_string())
        );
        assert_eq!(
            MongoUnitOfWork::extract_entity_id("platform.user"),
            None
        );
    }

}
