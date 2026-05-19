//! Unit of Work
//!
//! Atomic commit of entity state changes + domain events (+ optional audit logs)
//! within a single PostgreSQL transaction.
//!
//! The `OutboxUnitOfWork` writes events to the `outbox_messages` table in the
//! consumer's database. The fc-outbox-processor then forwards these to the
//! FlowCatalyst platform API.
//!
//! # Use Case Pattern
//!
//! Consumer apps build use cases that follow the same pattern as the platform:
//!
//! ```ignore
//! pub struct ShipOrderUseCase<U: UnitOfWork> {
//!     order_repo: Arc<OrderRepository>,
//!     unit_of_work: Arc<U>,
//! }
//!
//! impl<U: UnitOfWork> ShipOrderUseCase<U> {
//!     pub async fn execute(
//!         &self,
//!         command: ShipOrderCommand,
//!         ctx: ExecutionContext,
//!     ) -> UseCaseResult<OrderShipped> {
//!         // 1. Validate
//!         if command.tracking_number.is_empty() {
//!             return UseCaseResult::failure(
//!                 UseCaseError::validation("TRACKING_REQUIRED", "Tracking number is required"),
//!             );
//!         }
//!
//!         // 2. Load & check business rules
//!         let order = self.order_repo.find_by_id(&command.order_id).await
//!             .ok_or_else(|| UseCaseError::not_found("ORDER_NOT_FOUND", "Order not found"))?;
//!         if order.status != "confirmed" {
//!             return UseCaseResult::failure(
//!                 UseCaseError::business_rule("NOT_CONFIRMED", "Order must be confirmed to ship"),
//!             );
//!         }
//!
//!         // 3. Build domain event
//!         let event = OrderShipped {
//!             metadata: EventMetadata::builder()
//!                 .from(&ctx)
//!                 .event_type("shop:orders:order:shipped")
//!                 .spec_version("1.0")
//!                 .source("shop:orders")
//!                 .subject(format!("orders.order.{}", order.id))
//!                 .message_group(format!("orders:order:{}", order.id))
//!                 .build(),
//!             order_id: order.id.clone(),
//!             tracking_number: command.tracking_number.clone(),
//!         };
//!
//!         // 4. Atomic commit: entity + event (+ audit log if configured)
//!         self.unit_of_work.commit(&order, event, &command).await
//!     }
//! }
//! ```
//!
//! The handler checks authorization, builds the command, creates an
//! `ExecutionContext`, and calls `use_case.execute(cmd, ctx).await.into_result()?`.

use std::future::Future;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;
use sqlx::{PgPool, Postgres, Transaction};
use tokio::sync::Mutex;
use tracing::{debug, error};

use crate::tsid::TsidGenerator;
use crate::usecase::domain_event::DomainEvent;
use crate::usecase::error::UseCaseError;
use crate::usecase::result::UseCaseResult;

// ─── Traits ──────────────────────────────────────────────────────────────────

/// Trait for entities that have a unique string ID.
pub trait HasId {
    fn id(&self) -> &str;
    /// Legacy collection name. Unused in PostgreSQL implementation.
    fn collection_name() -> &'static str
    where
        Self: Sized,
    {
        ""
    }
}

/// Trait for domain entities that can be upserted/deleted within a PostgreSQL transaction.
///
/// Implement this for every aggregate that is passed to `UnitOfWork::commit`.
/// This matches the platform's `PgPersist` trait so that SDK consumers follow
/// the same conventions as the platform codebase.
///
/// # Example
///
/// ```ignore
/// use fc_sdk::outbox::{PgPersist, HasId};
/// use sqlx::{Postgres, Transaction};
///
/// struct Order { id: String, customer_id: String, total: f64 }
///
/// impl HasId for Order {
///     fn id(&self) -> &str { &self.id }
/// }
///
/// #[async_trait::async_trait]
/// impl PgPersist for Order {
///     async fn pg_upsert(&self, txn: &mut Transaction<'_, Postgres>) -> anyhow::Result<()> {
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
///     async fn pg_delete(&self, txn: &mut Transaction<'_, Postgres>) -> anyhow::Result<()> {
///         sqlx::query("DELETE FROM orders WHERE id = $1")
///             .bind(&self.id)
///             .execute(&mut **txn)
///             .await?;
///         Ok(())
///     }
/// }
/// ```
#[async_trait]
pub trait PgPersist: HasId + Send + Sync {
    /// Upsert the entity into the database within the given transaction.
    async fn pg_upsert(&self, txn: &mut Transaction<'_, Postgres>) -> anyhow::Result<()>;

    /// Delete the entity from the database within the given transaction.
    async fn pg_delete(&self, txn: &mut Transaction<'_, Postgres>) -> anyhow::Result<()>;
}

/// Trait for aggregates passed by value to `commit_all`.
/// Same as `PgPersist` but object-safe via `async_trait`.
#[async_trait]
pub trait PgAggregate: Send + Sync {
    fn id(&self) -> &str;
    async fn pg_upsert(&self, txn: &mut Transaction<'_, Postgres>) -> anyhow::Result<()>;
}

// ─── UnitOfWork trait ────────────────────────────────────────────────────────

/// Unit of Work for atomic domain operations.
///
/// Ensures entity state changes and domain events are committed atomically.
/// Audit logs are written when enabled (see [`OutboxConfig::audit_enabled`]).
///
/// Consumer apps use this the same way the platform does:
/// validate → build event → `uow.commit(entity, event, &cmd)`.
#[async_trait]
pub trait UnitOfWork: Send + Sync {
    /// Commit an entity upsert with its domain event (and optional audit log).
    async fn commit<E, T, C>(&self, aggregate: &T, event: E, command: &C) -> UseCaseResult<E>
    where
        E: DomainEvent + Serialize + Send + 'static,
        T: Serialize + HasId + PgPersist + Send + Sync,
        C: Serialize + Send + Sync;

    /// Commit an entity delete with its domain event (and optional audit log).
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

    /// Emit a domain event without an entity change (e.g., UserLoggedIn).
    async fn emit_event<E, C>(&self, event: E, command: &C) -> UseCaseResult<E>
    where
        E: DomainEvent + Serialize + Send + 'static,
        C: Serialize + Send + Sync;

    /// Commit multiple entity upserts with a single domain event.
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

// ─── OutboxConfig ────────────────────────────────────────────────────────────

/// Configuration for the outbox unit of work.
#[derive(Debug, Clone)]
pub struct OutboxConfig {
    /// Table name for outbox messages (default: "outbox_messages")
    pub table_name: String,
    /// Optional client_id for multi-tenant scoping
    pub client_id: Option<String>,
    /// Whether to write audit log entries alongside events (default: false).
    ///
    /// The platform always audits (control plane operations). Consumer apps
    /// should enable this only for admin/human-initiated operations, not for
    /// every transactional event.
    pub audit_enabled: bool,
}

impl Default for OutboxConfig {
    fn default() -> Self {
        Self {
            table_name: "outbox_messages".to_string(),
            client_id: None,
            audit_enabled: false,
        }
    }
}

// ─── OutboxUnitOfWork ────────────────────────────────────────────────────────

/// Outbox-backed implementation of [`UnitOfWork`].
///
/// Atomically persists entity changes and domain events to the `outbox_messages`
/// table. The fc-outbox-processor polls this table and forwards items to the
/// FlowCatalyst platform API.
///
/// # Example
///
/// ```ignore
/// use fc_sdk::outbox::{OutboxUnitOfWork, OutboxConfig};
///
/// // Events only (default for transactional operations)
/// let uow = OutboxUnitOfWork::new(pool.clone());
///
/// // Events + audit logs (for admin operations)
/// let uow = OutboxUnitOfWork::with_config(pool, OutboxConfig {
///     audit_enabled: true,
///     ..Default::default()
/// });
/// ```
#[derive(Clone)]
pub struct OutboxUnitOfWork {
    pool: PgPool,
    config: OutboxConfig,
}

impl OutboxUnitOfWork {
    /// Create a new OutboxUnitOfWork with default configuration (events only, no audit).
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
        let id = TsidGenerator::generate_untyped();

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
        audit_enabled: bool,
    ) -> Result<(), UseCaseError> {
        Self::write_event_outbox(txn, table, event, client_id).await?;
        if audit_enabled {
            Self::write_audit_outbox(txn, table, event, command, client_id).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl UnitOfWork for OutboxUnitOfWork {
    async fn commit<E, T, C>(&self, aggregate: &T, event: E, command: &C) -> UseCaseResult<E>
    where
        E: DomainEvent + Serialize + Send + 'static,
        T: Serialize + HasId + PgPersist + Send + Sync,
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

        if let Err(e) = aggregate.pg_upsert(&mut txn).await {
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
            self.config.audit_enabled,
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
            "Committed entity + outbox event"
        );

        UseCaseResult::success(event)
    }

    async fn commit_delete<E, T, C>(&self, aggregate: &T, event: E, command: &C) -> UseCaseResult<E>
    where
        E: DomainEvent + Serialize + Send + 'static,
        T: Serialize + HasId + PgPersist + Send + Sync,
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

        if let Err(e) = aggregate.pg_delete(&mut txn).await {
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
            self.config.audit_enabled,
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
            "Committed delete + outbox event"
        );

        UseCaseResult::success(event)
    }

    async fn emit_event<E, C>(&self, event: E, command: &C) -> UseCaseResult<E>
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
            self.config.audit_enabled,
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
        aggregates: Vec<Box<dyn PgAggregate>>,
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
            if let Err(e) = aggregate.pg_upsert(&mut txn).await {
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
            self.config.audit_enabled,
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
            "Committed multi-aggregate + outbox event"
        );

        UseCaseResult::success(event)
    }
}

// ─── TxScopedOutboxUnitOfWork ────────────────────────────────────────────────

/// UnitOfWork implementation that writes into an already-open transaction
/// owned by [`OutboxUnitOfWork::run`].
///
/// `commit()` / `commit_delete()` / `emit_event()` / `commit_all()` append
/// their rows to the shared transaction but do NOT close it — the outer
/// `run` does (commits on `UseCaseResult::Success`, rolls back on `Failure`).
///
/// This is the SDK analogue of the platform's `TxScopedUnitOfWork` and exists
/// so consumer applications can orchestrate **their own writes alongside
/// outbox writes** in a single, atomic transaction owned by the application.
/// Inside the closure passed to [`OutboxUnitOfWork::run`], use either:
///
/// - Multiple use cases that all share this `Arc<TxScopedOutboxUnitOfWork>`
///   so their aggregate writes + outbox rows commit together, or
/// - [`TxScopedOutboxUnitOfWork::with_tx`] for ad-hoc sqlx writes (non-aggregate
///   tables, raw SQL) that need to be atomic with the outbox rows.
pub struct TxScopedOutboxUnitOfWork {
    // tokio Mutex because the guard is held across `.await`. `Option` so
    // `run` can `.take()` the tx back out after the closure completes.
    tx: Mutex<Option<Transaction<'static, Postgres>>>,
    config: OutboxConfig,
}

impl TxScopedOutboxUnitOfWork {
    fn new(tx: Transaction<'static, Postgres>, config: OutboxConfig) -> Self {
        Self {
            tx: Mutex::new(Some(tx)),
            config,
        }
    }

    async fn take_tx(&self) -> Option<Transaction<'static, Postgres>> {
        self.tx.lock().await.take()
    }

    /// Run a closure with mutable access to the shared transaction.
    ///
    /// Use this for ad-hoc sqlx writes (non-aggregate rows, raw SQL) that
    /// must commit atomically with the outbox rows the UoW produces:
    ///
    /// ```ignore
    /// uow.run(|session| async move {
    ///     session
    ///         .with_tx(|txn| async move {
    ///             sqlx::query("UPDATE users SET last_seen_at = NOW() WHERE id = $1")
    ///                 .bind(&user_id)
    ///                 .execute(&mut **txn)
    ///                 .await
    ///                 .map_err(UseCaseError::commit)
    ///         })
    ///         .await?;
    ///     ship_order_uc.run(cmd, ctx).await.into_result()
    /// })
    /// .await
    /// ```
    pub async fn with_tx<F, Fut, R, E>(&self, f: F) -> Result<R, E>
    where
        F: for<'t> FnOnce(&'t mut Transaction<'static, Postgres>) -> Fut,
        Fut: Future<Output = Result<R, E>>,
        E: From<UseCaseError>,
    {
        let mut guard = self.tx.lock().await;
        let txn = guard.as_mut().ok_or_else(|| {
            E::from(UseCaseError::commit(
                "TxScopedOutboxUnitOfWork: transaction already finalized",
            ))
        })?;
        f(txn).await
    }
}

#[async_trait]
impl UnitOfWork for TxScopedOutboxUnitOfWork {
    async fn commit<E, T, C>(&self, aggregate: &T, event: E, command: &C) -> UseCaseResult<E>
    where
        E: DomainEvent + Serialize + Send + 'static,
        T: Serialize + HasId + PgPersist + Send + Sync,
        C: Serialize + Send + Sync,
    {
        let mut guard = self.tx.lock().await;
        let txn = match guard.as_mut() {
            Some(t) => t,
            None => {
                return UseCaseResult::failure(UseCaseError::commit(
                    "TxScopedOutboxUnitOfWork: transaction already finalized",
                ));
            }
        };

        if let Err(e) = aggregate.pg_upsert(txn).await {
            error!("Failed to persist aggregate in scoped tx: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(format!(
                "Failed to persist aggregate: {}",
                e
            )));
        }

        if let Err(e) = OutboxUnitOfWork::persist_outbox_items(
            txn,
            &self.config.table_name,
            &event,
            command,
            &self.config.client_id,
            self.config.audit_enabled,
        )
        .await
        {
            return UseCaseResult::failure(e);
        }

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
        let mut guard = self.tx.lock().await;
        let txn = match guard.as_mut() {
            Some(t) => t,
            None => {
                return UseCaseResult::failure(UseCaseError::commit(
                    "TxScopedOutboxUnitOfWork: transaction already finalized",
                ));
            }
        };

        if let Err(e) = aggregate.pg_delete(txn).await {
            error!("Failed to delete aggregate in scoped tx: {}", e);
            return UseCaseResult::failure(UseCaseError::commit(format!(
                "Failed to delete aggregate: {}",
                e
            )));
        }

        if let Err(e) = OutboxUnitOfWork::persist_outbox_items(
            txn,
            &self.config.table_name,
            &event,
            command,
            &self.config.client_id,
            self.config.audit_enabled,
        )
        .await
        {
            return UseCaseResult::failure(e);
        }

        UseCaseResult::success(event)
    }

    async fn emit_event<E, C>(&self, event: E, command: &C) -> UseCaseResult<E>
    where
        E: DomainEvent + Serialize + Send + 'static,
        C: Serialize + Send + Sync,
    {
        let mut guard = self.tx.lock().await;
        let txn = match guard.as_mut() {
            Some(t) => t,
            None => {
                return UseCaseResult::failure(UseCaseError::commit(
                    "TxScopedOutboxUnitOfWork: transaction already finalized",
                ));
            }
        };

        if let Err(e) = OutboxUnitOfWork::persist_outbox_items(
            txn,
            &self.config.table_name,
            &event,
            command,
            &self.config.client_id,
            self.config.audit_enabled,
        )
        .await
        {
            return UseCaseResult::failure(e);
        }

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
        let mut guard = self.tx.lock().await;
        let txn = match guard.as_mut() {
            Some(t) => t,
            None => {
                return UseCaseResult::failure(UseCaseError::commit(
                    "TxScopedOutboxUnitOfWork: transaction already finalized",
                ));
            }
        };

        for aggregate in &aggregates {
            if let Err(e) = aggregate.pg_upsert(txn).await {
                error!("Failed to persist aggregate in scoped batch: {}", e);
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to persist aggregate: {}",
                    e
                )));
            }
        }

        if let Err(e) = OutboxUnitOfWork::persist_outbox_items(
            txn,
            &self.config.table_name,
            &event,
            command,
            &self.config.client_id,
            self.config.audit_enabled,
        )
        .await
        {
            return UseCaseResult::failure(e);
        }

        UseCaseResult::success(event)
    }
}

impl OutboxUnitOfWork {
    /// Run a closure inside a single, application-orchestrated transaction.
    ///
    /// The closure receives an `Arc<TxScopedOutboxUnitOfWork>` that implements
    /// [`UnitOfWork`] and exposes [`TxScopedOutboxUnitOfWork::with_tx`] for
    /// ad-hoc writes. Use cases or repositories constructed against the scoped
    /// UoW all share the same transaction. The closure's `UseCaseResult`
    /// drives commit (on `Success`) vs rollback (on `Failure`).
    ///
    /// Use this when the consumer application needs to compose multiple
    /// aggregate writes (or non-aggregate writes via `with_tx`) with the
    /// outbox event/audit rows so they commit atomically:
    ///
    /// ```ignore
    /// uow.run(|session| async move {
    ///     let order_uc = ShipOrderUseCase::new(order_repo, session.clone());
    ///     let ledger_uc = DebitAccountUseCase::new(ledger_repo, session.clone());
    ///
    ///     order_uc.run(ship_cmd, ctx.clone()).await.into_result()?;
    ///     ledger_uc.run(debit_cmd, ctx).await.into_result()?;
    ///     UseCaseResult::success(())
    /// })
    /// .await
    /// ```
    ///
    /// The tx boundary lives in the application's handler; use cases stay
    /// tx-agnostic — they only see the `UnitOfWork` trait, so the same use
    /// case body works whether invoked against the standalone
    /// [`OutboxUnitOfWork`] (one use case per tx) or `TxScopedOutboxUnitOfWork`
    /// (many use cases per tx).
    ///
    /// This mirrors `PgUnitOfWork::run` from the platform crate so consumer
    /// apps and the platform follow the same orchestration pattern.
    pub async fn run<F, Fut, R>(&self, f: F) -> UseCaseResult<R>
    where
        F: FnOnce(Arc<TxScopedOutboxUnitOfWork>) -> Fut + Send,
        Fut: Future<Output = UseCaseResult<R>> + Send,
        R: Send + 'static,
    {
        let tx = match self.pool.begin().await {
            Ok(t) => t,
            Err(e) => {
                error!("Failed to start orchestration transaction: {}", e);
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "Failed to start transaction: {}",
                    e
                )));
            }
        };

        let scoped = Arc::new(TxScopedOutboxUnitOfWork::new(tx, self.config.clone()));
        let result = f(Arc::clone(&scoped)).await;

        // Reclaim the tx. If the scoped UoW has outstanding references
        // (e.g. a use case leaked it into a spawned task) we can't reclaim
        // — drop without explicit commit, which rolls back.
        let tx_opt = scoped.take_tx().await;

        if let Some(tx) = tx_opt {
            match &result {
                UseCaseResult::Success(_) => {
                    if let Err(e) = tx.commit().await {
                        error!("Failed to commit orchestration tx: {}", e);
                        return UseCaseResult::failure(UseCaseError::commit(format!(
                            "Failed to commit transaction: {}",
                            e
                        )));
                    }
                    debug!("Orchestration tx committed");
                }
                UseCaseResult::Failure(err) => {
                    let _ = tx.rollback().await;
                    debug!(error = %err.code(), "Orchestration tx rolled back");
                }
            }
        }

        result
    }
}

// ─── InMemoryUnitOfWork (tests) ──────────────────────────────────────────────

/// In-memory implementation of [`UnitOfWork`] for unit testing use cases.
///
/// Records committed event IDs so tests can assert which events were emitted
/// without needing a database.
///
/// # Example
///
/// ```ignore
/// use fc_sdk::outbox::InMemoryUnitOfWork;
/// use std::sync::Arc;
///
/// let uow = Arc::new(InMemoryUnitOfWork::new());
/// let use_case = ShipOrderUseCase::new(mock_repo, uow.clone());
///
/// let result = use_case.execute(cmd, ctx).await;
/// assert!(result.is_success());
/// assert_eq!(uow.committed_events().len(), 1);
/// ```
pub struct InMemoryUnitOfWork {
    committed_events: std::sync::Mutex<Vec<String>>,
}

impl InMemoryUnitOfWork {
    pub fn new() -> Self {
        Self {
            committed_events: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Get the list of committed event IDs.
    pub fn committed_events(&self) -> Vec<String> {
        self.committed_events.lock().unwrap().clone()
    }

    /// Check if any events were committed.
    pub fn has_commits(&self) -> bool {
        !self.committed_events.lock().unwrap().is_empty()
    }

    /// Clear all recorded events.
    pub fn clear(&self) {
        self.committed_events.lock().unwrap().clear();
    }
}

impl Default for InMemoryUnitOfWork {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UnitOfWork for InMemoryUnitOfWork {
    async fn commit<E, T, C>(&self, _aggregate: &T, event: E, _command: &C) -> UseCaseResult<E>
    where
        E: DomainEvent + Serialize + Send + 'static,
        T: Serialize + HasId + PgPersist + Send + Sync,
        C: Serialize + Send + Sync,
    {
        self.committed_events
            .lock()
            .unwrap()
            .push(event.event_id().to_string());
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
        T: Serialize + HasId + PgPersist + Send + Sync,
        C: Serialize + Send + Sync,
    {
        self.committed_events
            .lock()
            .unwrap()
            .push(event.event_id().to_string());
        UseCaseResult::success(event)
    }

    async fn emit_event<E, C>(&self, event: E, _command: &C) -> UseCaseResult<E>
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

    async fn commit_all<E, C>(
        &self,
        _aggregates: Vec<Box<dyn PgAggregate>>,
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
    use crate::usecase::EventMetadata;

    // ─── OutboxConfig ───────────────────────────────────────────────────

    #[test]
    fn outbox_config_default() {
        let config = OutboxConfig::default();
        assert_eq!(config.table_name, "outbox_messages");
        assert!(config.client_id.is_none());
        assert!(!config.audit_enabled);
    }

    #[test]
    fn outbox_config_custom() {
        let config = OutboxConfig {
            table_name: "custom_outbox".to_string(),
            client_id: Some("clt_123".to_string()),
            audit_enabled: true,
        };
        assert_eq!(config.table_name, "custom_outbox");
        assert_eq!(config.client_id.as_deref(), Some("clt_123"));
        assert!(config.audit_enabled);
    }

    #[test]
    fn outbox_config_clone() {
        let config = OutboxConfig {
            table_name: "t".into(),
            client_id: Some("c".into()),
            audit_enabled: true,
        };
        let cloned = config.clone();
        assert_eq!(cloned.table_name, "t");
        assert_eq!(cloned.client_id.as_deref(), Some("c"));
        assert!(cloned.audit_enabled);
    }

    // ─── extract_aggregate_type ─────────────────────────────────────────

    #[test]
    fn extract_aggregate_type_standard_subject() {
        assert_eq!(
            OutboxUnitOfWork::extract_aggregate_type("orders.order.123"),
            "Order"
        );
    }

    #[test]
    fn extract_aggregate_type_capitalizes_first_letter() {
        assert_eq!(
            OutboxUnitOfWork::extract_aggregate_type("fulfillment.shipment.abc"),
            "Shipment"
        );
    }

    #[test]
    fn extract_aggregate_type_single_segment() {
        assert_eq!(
            OutboxUnitOfWork::extract_aggregate_type("single"),
            "Unknown"
        );
    }

    #[test]
    fn extract_aggregate_type_empty_second_segment() {
        assert_eq!(OutboxUnitOfWork::extract_aggregate_type("a..c"), "");
    }

    #[test]
    fn extract_aggregate_type_empty_string() {
        assert_eq!(OutboxUnitOfWork::extract_aggregate_type(""), "Unknown");
    }

    // ─── extract_entity_id ──────────────────────────────────────────────

    #[test]
    fn extract_entity_id_standard_subject() {
        assert_eq!(
            OutboxUnitOfWork::extract_entity_id("orders.order.ord_123"),
            "ord_123"
        );
    }

    #[test]
    fn extract_entity_id_no_third_segment() {
        assert_eq!(OutboxUnitOfWork::extract_entity_id("orders.order"), "");
    }

    #[test]
    fn extract_entity_id_single_segment() {
        assert_eq!(OutboxUnitOfWork::extract_entity_id("single"), "");
    }

    #[test]
    fn extract_entity_id_empty() {
        assert_eq!(OutboxUnitOfWork::extract_entity_id(""), "");
    }

    #[test]
    fn extract_entity_id_many_segments() {
        assert_eq!(OutboxUnitOfWork::extract_entity_id("a.b.c.d.e"), "c");
    }

    // ─── InMemoryUnitOfWork ─────────────────────────────────────────────

    #[derive(Debug, Clone, Serialize)]
    struct FakeEvent {
        pub metadata: EventMetadata,
    }
    crate::impl_domain_event!(FakeEvent);

    fn make_fake_event(event_id: &str) -> FakeEvent {
        FakeEvent {
            metadata: EventMetadata::new(
                event_id.into(),
                "test:event",
                "1.0",
                "test",
                "test.entity.1".into(),
                "test:entity:1".into(),
                "exec-1".into(),
                "corr-1".into(),
                None,
                "prn_test".into(),
            ),
        }
    }

    #[derive(Serialize)]
    struct FakeCommand {
        name: String,
    }

    #[derive(Serialize)]
    struct FakeEntity {
        id: String,
    }

    impl HasId for FakeEntity {
        fn id(&self) -> &str {
            &self.id
        }
    }

    #[async_trait]
    impl PgPersist for FakeEntity {
        async fn pg_upsert(&self, _txn: &mut Transaction<'_, Postgres>) -> anyhow::Result<()> {
            Ok(())
        }
        async fn pg_delete(&self, _txn: &mut Transaction<'_, Postgres>) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn in_memory_uow_new_is_empty() {
        let uow = InMemoryUnitOfWork::new();
        assert!(uow.committed_events().is_empty());
        assert!(!uow.has_commits());
    }

    #[test]
    fn in_memory_uow_default_is_empty() {
        let uow = InMemoryUnitOfWork::default();
        assert!(uow.committed_events().is_empty());
    }

    #[tokio::test]
    async fn in_memory_uow_commit_records_event() {
        let uow = InMemoryUnitOfWork::new();
        let event = make_fake_event("evt_commit_1");
        let entity = FakeEntity { id: "e_1".into() };
        let cmd = FakeCommand {
            name: "test".into(),
        };

        let result = uow.commit(&entity, event, &cmd).await;
        assert!(result.is_success());
        assert!(uow.has_commits());
        assert_eq!(uow.committed_events(), vec!["evt_commit_1"]);
    }

    #[tokio::test]
    async fn in_memory_uow_commit_delete_records_event() {
        let uow = InMemoryUnitOfWork::new();
        let event = make_fake_event("evt_delete_1");
        let entity = FakeEntity { id: "e_1".into() };
        let cmd = FakeCommand { name: "del".into() };

        let result = uow.commit_delete(&entity, event, &cmd).await;
        assert!(result.is_success());
        assert_eq!(uow.committed_events(), vec!["evt_delete_1"]);
    }

    #[tokio::test]
    async fn in_memory_uow_emit_event_records_event() {
        let uow = InMemoryUnitOfWork::new();
        let event = make_fake_event("evt_emit_1");
        let cmd = FakeCommand {
            name: "emit".into(),
        };

        let result = uow.emit_event(event, &cmd).await;
        assert!(result.is_success());
        assert_eq!(uow.committed_events(), vec!["evt_emit_1"]);
    }

    #[tokio::test]
    async fn in_memory_uow_commit_all_records_event() {
        let uow = InMemoryUnitOfWork::new();
        let event = make_fake_event("evt_all_1");
        let cmd = FakeCommand { name: "all".into() };
        let aggregates: Vec<Box<dyn PgAggregate>> = vec![];

        let result = uow.commit_all(aggregates, event, &cmd).await;
        assert!(result.is_success());
        assert_eq!(uow.committed_events(), vec!["evt_all_1"]);
    }

    #[tokio::test]
    async fn in_memory_uow_multiple_commits() {
        let uow = InMemoryUnitOfWork::new();
        let entity = FakeEntity { id: "e_1".into() };
        let cmd = FakeCommand { name: "t".into() };

        uow.commit(&entity, make_fake_event("a"), &cmd).await;
        uow.commit(&entity, make_fake_event("b"), &cmd).await;
        uow.emit_event(make_fake_event("c"), &cmd).await;

        assert_eq!(uow.committed_events().len(), 3);
        assert_eq!(uow.committed_events(), vec!["a", "b", "c"]);
    }

    #[tokio::test]
    async fn in_memory_uow_clear() {
        let uow = InMemoryUnitOfWork::new();
        let entity = FakeEntity { id: "e_1".into() };
        let cmd = FakeCommand { name: "t".into() };

        uow.commit(&entity, make_fake_event("x"), &cmd).await;
        assert!(uow.has_commits());

        uow.clear();
        assert!(!uow.has_commits());
        assert!(uow.committed_events().is_empty());
    }

    #[tokio::test]
    async fn in_memory_uow_returns_event_on_success() {
        let uow = InMemoryUnitOfWork::new();
        let entity = FakeEntity { id: "e_1".into() };
        let cmd = FakeCommand { name: "t".into() };

        let result = uow
            .commit(&entity, make_fake_event("evt_return"), &cmd)
            .await;

        let event = result.unwrap();
        assert_eq!(event.event_id(), "evt_return");
        assert_eq!(event.event_type(), "test:event");
    }

    // ─── TxScopedOutboxUnitOfWork (compile-time bounds) ─────────────────

    /// Compile-time assertion that `TxScopedOutboxUnitOfWork` implements
    /// the `UnitOfWork` trait. Constructing one requires a real `Transaction`
    /// from a live pool, so runtime behaviour is exercised by the postgres
    /// integration tests in `fc-platform/tests/postgres_integration_tests.rs`.
    #[test]
    fn tx_scoped_outbox_uow_implements_unit_of_work() {
        fn assert_uow<T: UnitOfWork>() {}
        assert_uow::<TxScopedOutboxUnitOfWork>();
    }
}
