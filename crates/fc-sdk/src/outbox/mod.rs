//! Outbox Integration
//!
//! Provides the same use case infrastructure used by the FlowCatalyst platform,
//! so consumer Rust apps follow the same pattern: validation → business rules →
//! domain event → atomic commit via `UnitOfWork`.
//!
//! ## 1. Use Case Pattern (recommended)
//!
//! Build use cases with [`UnitOfWork`] that atomically commit entity state +
//! domain events (+ optional audit logs for admin operations).
//!
//! ```ignore
//! use fc_sdk::outbox::{OutboxUnitOfWork, UnitOfWork, PgPersist, HasId, schema};
//! use fc_sdk::usecase::{ExecutionContext, EventMetadata, UseCaseResult, UseCaseError};
//!
//! // 1. Initialize the outbox table
//! let pool = sqlx::PgPool::connect("postgresql://localhost/myapp").await?;
//! schema::init_outbox_schema(&pool).await?;
//!
//! // 2. Create the UnitOfWork
//! let uow = OutboxUnitOfWork::new(pool.clone());
//!
//! // 3. In your use case: validate, check business rules, commit
//! let ctx = ExecutionContext::create("user-123");
//! let event = OrderCreated { metadata: EventMetadata::builder().from(&ctx)..., ... };
//! let result = uow.commit(&order, event, &create_cmd).await;
//! ```
//!
//! For unit testing use cases without a database, use [`InMemoryUnitOfWork`].
//!
//! ## 2. Simple Outbox Pattern (lightweight, matches TS/Laravel SDKs)
//!
//! Use [`OutboxManager`] with builder DTOs when you don't need the full use case ceremony.
//!
//! ```ignore
//! use fc_sdk::outbox::{OutboxManager, SqlxPgDriver, CreateEventDto};
//!
//! let driver = SqlxPgDriver::new(pool);
//! let outbox = OutboxManager::new(Box::new(driver), "clt_0HZXEQ5Y8JY5Z");
//!
//! let id = outbox.create_event(
//!     CreateEventDto::new("user.registered", serde_json::json!({"userId": "123"}))
//!         .source("user-service")
//!         .message_group("users:user:123"),
//! ).await?;
//! ```
//!
//! # Architecture
//!
//! ```text
//! Your Application
//!   ├── Handlers (authorization → command → ExecutionContext → use_case.execute())
//!   ├── Use Cases (validation → business rules → UnitOfWork::commit())
//!   ├── Entity Persistence (PgPersist — your tables)
//!   └── Outbox Items (outbox_messages table)
//!         ↓
//! fc-outbox-processor (polls outbox_messages)
//!         ↓
//! FlowCatalyst Platform API
//!   ├── /api/events/batch       (EVENT items)
//!   ├── /api/dispatch-jobs/batch (DISPATCH_JOB items)
//!   └── /api/audit-logs/batch       (AUDIT_LOG items)
//! ```

// Use case pattern (UnitOfWork + DomainEvent + PgPersist)
pub mod unit_of_work;
pub mod payload;
pub mod schema;

// Simple outbox pattern (OutboxManager + DTOs + Driver)
pub mod driver;
pub mod dto;
pub mod manager;
pub mod sqlx_pg_driver;

// ─── Full DDD pattern re-exports ────────────────────────────────────────────

pub use unit_of_work::{
    OutboxUnitOfWork, OutboxConfig, InMemoryUnitOfWork,
    UnitOfWork, HasId, PgPersist, PgAggregate,
};
pub use payload::{
    DispatchJobPayload, AuditLogPayload,
    write_dispatch_job, write_event, write_audit_log,
    emit_event, emit_dispatch_job, emit_audit_log,
};
pub use schema::{init_outbox_schema, init_outbox_schema_with_table, CREATE_OUTBOX_TABLE_SQL};

// ─── Simple outbox pattern re-exports ───────────────────────────────────────

pub use driver::{OutboxDriver, OutboxMessage, OutboxStatus, MessageType};
pub use dto::{CreateEventDto, CreateDispatchJobDto, CreateAuditLogDto, ContextDataEntry};
pub use manager::OutboxManager;
pub use sqlx_pg_driver::SqlxPgDriver;
