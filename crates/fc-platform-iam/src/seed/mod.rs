//! Database Seeding
//!
//! Seeds for platform system data (built-in event types + their schemas).
//! Bootstrap of users / clients / applications / service accounts is owned
//! by `fc-dev init` (not auto-run at startup) — there's no
//! one-size-fits-all dev fixture and forcing a fixed set creates churn
//! whenever the data shape changes.

pub mod platform_event_schemas;
pub mod platform_event_types;
