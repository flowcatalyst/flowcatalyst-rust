//! Outbox Integration
//!
//! Provides an outbox-backed [`UnitOfWork`] that writes domain events and
//! audit logs as outbox items. The fc-outbox-processor polls these and
//! forwards them to the FlowCatalyst platform API.
//!
//! # Architecture
//!
//! ```text
//! Your Application
//!   ├── Business Logic (use cases)
//!   ├── Entity Persistence (your tables)
//!   └── Outbox Items (outbox_messages table)
//!         ↓
//! fc-outbox-processor (polls outbox_messages)
//!         ↓
//! FlowCatalyst Platform API
//!   ├── /api/events/batch      (EVENT items)
//!   ├── /api/dispatch/jobs/batch (DISPATCH_JOB items)
//!   └── /api/audit/logs/batch  (AUDIT_LOG items)
//! ```
//!
//! # Quick Start
//!
//! ```ignore
//! use fc_sdk::outbox::{OutboxUnitOfWork, schema};
//!
//! // 1. Initialize the outbox table
//! let pool = sqlx::PgPool::connect("postgresql://localhost/myapp").await?;
//! schema::init_outbox_schema(&pool).await?;
//!
//! // 2. Create the UnitOfWork
//! let uow = OutboxUnitOfWork::new(pool.clone());
//!
//! // 3. Use in your use cases
//! let result = uow.commit(&order, order_created_event, &create_cmd).await;
//! ```

pub mod unit_of_work;
pub mod payload;
pub mod schema;

pub use unit_of_work::{
    OutboxUnitOfWork, OutboxConfig,
    UnitOfWork, HasId, Persist, Aggregate,
};
pub use payload::{
    DispatchJobPayload, AuditLogPayload,
    write_dispatch_job, write_event, write_audit_log,
    emit_event, emit_dispatch_job, emit_audit_log,
};
pub use schema::{init_outbox_schema, init_outbox_schema_with_table, CREATE_OUTBOX_TABLE_SQL};
