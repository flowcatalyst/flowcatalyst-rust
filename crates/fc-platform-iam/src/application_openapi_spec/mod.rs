//! Application OpenAPI Spec aggregate
//!
//! Stores the OpenAPI document an application advertises. Versioned: at most
//! one `CURRENT` row per application, prior rows flipped to `ARCHIVED` with
//! computed change_notes. The platform itself is treated as one of these
//! applications (seeded row with `code='platform'`), so its dynamic
//! utoipa-generated spec is stored the same way and refreshed by the
//! "Sync All" button on the dashboard.

pub mod entity;
pub mod operations;
pub mod repository;
