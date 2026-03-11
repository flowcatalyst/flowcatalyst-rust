//! # FlowCatalyst SDK for Rust
//!
//! Build event-driven applications with FlowCatalyst's domain event patterns,
//! transactional outbox, and platform API management.
//!
//! ## Core Patterns
//!
//! The SDK provides the same domain-driven design patterns used by the
//! FlowCatalyst platform itself:
//!
//! - **Domain Events** — CloudEvents-compatible event trait with metadata builder
//! - **Execution Context** — Distributed tracing with correlation/causation chains
//! - **Use Case Result** — Categorized errors (validation, not found, business rule)
//! - **Unit of Work** — Atomic commit of entity + event + audit log
//! - **TSID Generator** — Time-sorted unique IDs with typed prefixes
//!
//! ## Outbox Integration
//!
//! The outbox module writes events and audit logs to an `outbox_messages` table
//! in your database, transactionally with your entity changes. The
//! fc-outbox-processor then forwards these to the FlowCatalyst platform API.
//!
//! ## Platform API Client
//!
//! The client module (feature `client`) provides HTTP operations for managing
//! event types, subscriptions, connections, and sync operations.
//!
//! ## Quick Start
//!
//! ```ignore
//! use fc_sdk::usecase::{ExecutionContext, EventMetadata, DomainEvent};
//! use fc_sdk::outbox::{OutboxUnitOfWork, UnitOfWork};
//! use fc_sdk::tsid::{TsidGenerator, EntityType};
//! use serde::Serialize;
//!
//! // 1. Define your domain event
//! #[derive(Debug, Serialize)]
//! pub struct OrderCreated {
//!     pub metadata: EventMetadata,
//!     pub order_id: String,
//!     pub customer_id: String,
//! }
//! fc_sdk::impl_domain_event!(OrderCreated);
//!
//! // 2. Define your command
//! #[derive(Serialize)]
//! pub struct CreateOrderCommand {
//!     pub customer_id: String,
//!     pub items: Vec<String>,
//! }
//!
//! // 3. Execute use case
//! async fn create_order(
//!     uow: &OutboxUnitOfWork,
//!     order: &Order,
//!     cmd: &CreateOrderCommand,
//! ) {
//!     let ctx = ExecutionContext::create("user-123");
//!
//!     let event = OrderCreated {
//!         metadata: EventMetadata::builder()
//!             .from(&ctx)
//!             .event_type("shop:orders:order:created")
//!             .spec_version("1.0")
//!             .source("shop:orders")
//!             .subject(format!("orders.order.{}", order.id()))
//!             .message_group(format!("orders:order:{}", order.id()))
//!             .build(),
//!         order_id: order.id().to_string(),
//!         customer_id: cmd.customer_id.clone(),
//!     };
//!
//!     let result = uow.commit(order, event, cmd).await;
//! }
//! # struct Order { id: String }
//! # impl Order { fn id(&self) -> &str { &self.id } }
//! ```

pub mod tsid;
pub mod usecase;

#[cfg(any(
    feature = "outbox-postgres",
    feature = "outbox-sqlite",
    feature = "outbox-mysql"
))]
pub mod outbox;

#[cfg(feature = "client")]
pub mod client;

// Re-export key types at crate root
pub use tsid::{TsidGenerator, EntityType};
pub use usecase::{
    UseCaseResult, UseCaseError,
    DomainEvent, EventMetadata, EventMetadataBuilder,
    ExecutionContext, TracingContext,
};
