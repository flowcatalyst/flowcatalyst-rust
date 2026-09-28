//! FlowCatalyst Platform: tenancy, identity and access.
//!
//! Clients, applications, principals, roles, service accounts, OAuth
//! clients, identity providers, email-domain mappings, platform config,
//! audit logs. Until they move to their own crates, it also holds the
//! sign-in flows (`auth`, `mfa`, `webauthn`, `portal`) and messaging
//! (event types, events, subscriptions, connections, dispatch).
//!
//! Every module keeps its path from the single `fc-platform` crate this was
//! split from (docs/plans/build-speed-2026-09-28.md, section 6); `fc-platform`
//! re-exports them at `fc_platform::<module>`.

pub mod app_docs;
pub mod application;
pub mod application_openapi_spec;
pub mod audit;
pub mod auth;
pub mod client;
pub mod connection;
pub mod cors;
pub mod developer_credential;
pub mod dispatch_job;
pub mod dispatch_job_actions;
pub mod dispatch_pool;
pub mod email_domain_mapping;
pub mod event;
pub mod event_type;
pub mod identity_provider;
pub mod login_attempt;
pub mod mfa;
pub mod password_reset;
pub mod platform_config;
pub mod portal;
pub mod principal;
pub mod process;
pub mod role;
pub mod scheduler;
pub mod seed;
pub mod service_account;
pub mod shared;
pub mod subscription;
pub mod webauthn;
