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
//! ## Running Scripts (Rust analogue of `tsx scripts/fc-sync.ts`)
//!
//! In the TypeScript SDK you'd add `"fc:sync": "tsx scripts/fc-sync.ts"` to
//! your `package.json`. The Rust equivalent is **`cargo run --example`**.
//! Drop a file in `examples/` of any crate that depends on `fc-sdk`:
//!
//! ```text
//! my-app/
//! ├── Cargo.toml
//! ├── src/main.rs
//! └── examples/
//!     └── fc-sync.rs       # cargo run --example fc-sync
//! ```
//!
//! Cargo auto-discovers everything under `examples/` — no Cargo.toml edit
//! needed. Wire it to a one-word command via a `justfile` recipe:
//!
//! ```text
//! fc-sync:
//!     cargo run --example fc-sync --features client
//! ```
//!
//! Now `just fc-sync` is the Rust analogue of `pnpm fc:sync`.
//!
//! ### Performance tiers
//!
//! | Scenario                                  | Time        |
//! |-------------------------------------------|-------------|
//! | Cold (deps not built)                     | 30s–2min    |
//! | Warm `cargo run`, script unchanged        | ~200–500ms  |
//! | Pre-built binary invoked directly         | ~10–50ms    |
//!
//! For scripts you run dozens of times during dev, build once and invoke
//! the binary directly:
//!
//! ```bash
//! cargo build --release --example fc-sync --features client
//! ./target/release/examples/fc-sync     # native startup, no cargo tax
//! ```
//!
//! ### Worked examples in this crate
//!
//! Three examples ship with the SDK and double as documentation. Each is
//! a self-contained, runnable Rust file:
//!
//! - **`examples/fc-sync.rs`** — push roles, event types, and subscriptions
//!   for an application. The direct analogue of the TypeScript
//!   `"fc:sync"` pattern.
//!   `cargo run --example fc-sync --features client`
//! - **`examples/list-event-types.rs`** — smallest possible smoke test of
//!   the API client. Lists event types currently registered on the
//!   platform.
//!   `cargo run --example list-event-types --features client`
//! - **`examples/scheduled-jobs-runner.rs`** — end-to-end Axum server
//!   hosting a [`scheduled_jobs::ScheduledJobRunner`] with two handlers,
//!   memory-backed concurrency lock, and on-error hook.
//!   `cargo run --example scheduled-jobs-runner --features scheduled-jobs-runner,axum`
//!
//! All three read `FC_BASE_URL` and `FC_TOKEN` from the environment.
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

#[cfg(any(feature = "cache", feature = "cache-postgres", feature = "cache-redis"))]
pub mod cache;

#[cfg(any(feature = "lock", feature = "lock-postgres", feature = "lock-redis"))]
pub mod lock;

#[cfg(feature = "client")]
pub mod client;

#[cfg(feature = "client")]
pub mod sync;

#[cfg(feature = "auth")]
pub mod auth;

#[cfg(feature = "webhook")]
pub mod webhook;

#[cfg(feature = "scheduled-jobs-runner")]
pub mod scheduled_jobs;

// Re-export key types at crate root
pub use tsid::{EntityType, TsidGenerator};
pub use usecase::{
    DomainEvent, EventMetadata, EventMetadataBuilder, ExecutionContext, TracingContext, UseCase,
    UseCaseError, UseCaseResult,
};
