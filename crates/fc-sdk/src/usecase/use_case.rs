//! Use Case Trait
//!
//! Enforces the use case contract: every operation must explicitly handle
//! validation, authorization, and execution — even if a step is a no-op.
//!
//! Handlers call [`UseCase::run`] which executes the pipeline in order:
//! **validate → authorize → execute**.
//!
//! # Example
//!
//! ```ignore
//! use fc_sdk::usecase::{UseCase, UseCaseResult, UseCaseError, ExecutionContext, EventMetadata};
//! use fc_sdk::outbox::UnitOfWork;
//!
//! pub struct ShipOrderUseCase<U: UnitOfWork> {
//!     order_repo: Arc<OrderRepository>,
//!     unit_of_work: Arc<U>,
//! }
//!
//! #[async_trait::async_trait]
//! impl<U: UnitOfWork> UseCase for ShipOrderUseCase<U> {
//!     type Command = ShipOrderCommand;
//!     type Event = OrderShipped;
//!
//!     async fn validate(&self, command: &ShipOrderCommand) -> Result<(), UseCaseError> {
//!         if command.tracking_number.is_empty() {
//!             return Err(UseCaseError::validation("TRACKING_REQUIRED", "Tracking number is required"));
//!         }
//!         Ok(())
//!     }
//!
//!     async fn authorize(&self, command: &ShipOrderCommand, ctx: &ExecutionContext) -> Result<(), UseCaseError> {
//!         // Resource-level check: does this principal own this order?
//!         let order = self.order_repo.find_by_id(&command.order_id).await
//!             .ok_or_else(|| UseCaseError::not_found("ORDER_NOT_FOUND", "Order not found"))?;
//!         if order.owner_id != ctx.principal_id {
//!             return Err(UseCaseError::business_rule("NOT_OWNER", "You don't own this order"));
//!         }
//!         Ok(())
//!     }
//!
//!     async fn execute(&self, command: ShipOrderCommand, ctx: ExecutionContext) -> UseCaseResult<OrderShipped> {
//!         let mut order = self.order_repo.find_by_id(&command.order_id).await.unwrap();
//!         order.status = "shipped".to_string();
//!         order.tracking_number = Some(command.tracking_number.clone());
//!
//!         let event = OrderShipped {
//!             metadata: EventMetadata::from_ctx(
//!                 &ctx,
//!                 "shop:orders:order:shipped",
//!                 "1.0",
//!                 "shop:orders",
//!                 format!("orders.order.{}", order.id),
//!                 format!("orders:order:{}", order.id),
//!             ),
//!             order_id: order.id.clone(),
//!             tracking_number: command.tracking_number.clone(),
//!         };
//!
//!         self.unit_of_work.commit(&order, event, &command).await
//!     }
//! }
//! ```
//!
//! In the handler:
//!
//! ```ignore
//! let event = state.ship_order.run(cmd, ctx).await.into_result()?;
//! ```

use async_trait::async_trait;
use serde::Serialize;

use super::domain_event::DomainEvent;
use super::error::UseCaseError;
use super::execution_context::ExecutionContext;
use super::result::UseCaseResult;

/// Trait that every use case must implement.
///
/// Forces explicit handling of three concerns:
///
/// 1. **`validate`** — Input validation (field presence, format, length).
///    Return `Ok(())` if no validation is needed.
///
/// 2. **`authorize`** — Resource-level authorization (ownership, access checks).
///    Return `Ok(())` if no authorization is needed.
///    Note: role/permission checks can also live in the handler before calling `run`.
///
/// 3. **`execute`** — Business logic: load aggregate, check business rules,
///    build domain event, call `unit_of_work.commit()`.
///
/// The provided [`run`](UseCase::run) method calls them in order and short-circuits
/// on the first error.
#[async_trait]
pub trait UseCase: Send + Sync {
    /// The command (input DTO) for this use case.
    type Command: Serialize + Send + Sync;

    /// The domain event emitted on success.
    type Event: DomainEvent + 'static;

    /// Validate the command inputs.
    ///
    /// Check field presence, format, length, patterns — anything that doesn't
    /// require loading data from the database.
    ///
    /// Return `Ok(())` if valid, or `Err(UseCaseError::validation(...))` if not.
    async fn validate(&self, command: &Self::Command) -> Result<(), UseCaseError>;

    /// Authorize the operation.
    ///
    /// Check resource-level access: does this principal own this entity?
    /// Can they access this client? This runs after validation so you can
    /// trust the command fields are well-formed.
    ///
    /// Return `Ok(())` if authorized, or `Err(UseCaseError)` if not.
    async fn authorize(
        &self,
        command: &Self::Command,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError>;

    /// Execute the core business logic.
    ///
    /// Load aggregates, check business rules (uniqueness, state transitions),
    /// build the domain event, and call `unit_of_work.commit()`.
    ///
    /// This is only called after `validate` and `authorize` both pass.
    async fn execute(
        &self,
        command: Self::Command,
        ctx: ExecutionContext,
    ) -> UseCaseResult<Self::Event>;

    /// Run the full pipeline: validate → authorize → execute.
    ///
    /// This is what handlers call. Do not override this.
    async fn run(
        &self,
        command: Self::Command,
        ctx: ExecutionContext,
    ) -> UseCaseResult<Self::Event> {
        if let Err(e) = self.validate(&command).await {
            return UseCaseResult::failure(e);
        }
        if let Err(e) = self.authorize(&command, &ctx).await {
            return UseCaseResult::failure(e);
        }
        self.execute(command, ctx).await
    }
}
