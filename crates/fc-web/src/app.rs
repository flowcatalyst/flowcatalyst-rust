//! Pages, layouts, layers and routes. Registered by `.discover()`; every
//! path is explicit (no module-derived routes), so a handler's URL is
//! greppable and `tests/auth_convention_test.rs` can check it.

mod applications;
mod audit_log;
mod clients;
mod connections;
mod dispatch_jobs;
mod dispatch_pools;
mod event_types;
mod events;
mod login;
mod nav;
mod roles;
mod shell;
mod subscriptions;
