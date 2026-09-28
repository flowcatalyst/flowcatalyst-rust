//! Process Aggregate
//!
//! Free-form workflow / process documentation. The `body` field stores
//! diagram source verbatim (typically Mermaid); the platform renders it
//! client-side. Code format mirrors EventType: `{application}:{subdomain}:{process-name}`.

pub mod api;
pub mod entity;
pub mod operations;
pub mod repository;
pub mod routes;

pub use entity::{Process, ProcessCode, ProcessCodeError, ProcessSource, ProcessStatus};
pub use repository::ProcessRepository;
pub use routes::{processes_router, routes};
