//! # FlowCatalyst common types (Apache-2.0)
//!
//! The part of the shared types the Rust SDK (`fc-sdk`) needs, in a crate
//! with the SDK's licence, so an application linking the SDK links no AGPL
//! code (owner decision #51). `fc-common` re-exports every item here at its
//! old path (`fc_common::tsid`, `fc_common::audit_redaction`,
//! `fc_common::OutboxStatus`, …), so the platform uses either name.
//!
//! - [`outbox`]: the outbox row's status codes and item types.
//! - [`tsid`]: prefixed TSID generation, the platform's entity-id format.
//! - [`audit_redaction`]: the rule that keeps secrets out of audit rows.
//!
//! Only add what an SDK must share with the platform; everything else stays
//! in `fc-common`.

pub mod audit_redaction;
pub mod outbox;
pub mod tsid;

pub use outbox::{OutboxItemType, OutboxStatus, UnknownOutboxItemType, UnknownOutboxStatus};
pub use tsid::EntityType;
