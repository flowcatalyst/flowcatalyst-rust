//! SeaORM Entity Models
//!
//! Database entity models mapping directly to PostgreSQL tables.
//! Domain entities (in each aggregate's entity.rs) are the API-facing types.

// Tenant tables
pub mod tnt_clients;
pub mod tnt_anchor_domains;
pub mod tnt_cors_allowed_origins;
pub mod tnt_client_auth_configs;
pub mod tnt_email_domain_mappings;
pub mod tnt_email_domain_mapping_clients;
pub mod tnt_email_domain_mapping_granted_clients;
pub mod tnt_email_domain_mapping_allowed_roles;

// IAM tables
pub mod iam_roles;
pub mod iam_permissions;
pub mod iam_role_permissions;
pub mod iam_principals;
pub mod iam_principal_roles;
pub mod iam_service_accounts;
pub mod iam_client_access_grants;
pub mod iam_principal_application_access;
pub mod iam_login_attempts;
pub mod iam_password_reset_tokens;
pub mod iam_oidc_login_states;
pub mod iam_refresh_tokens;
pub mod iam_authorization_codes;

// Application tables
pub mod app_applications;
pub mod app_client_configs;
pub mod app_platform_configs;
pub mod app_platform_config_access;

// Messaging tables
pub mod msg_events;
pub mod msg_events_read;
pub mod msg_event_types;
pub mod msg_event_type_spec_versions;
pub mod msg_subscriptions;
pub mod msg_subscription_event_types;
pub mod msg_subscription_custom_configs;
pub mod msg_dispatch_pools;
pub mod msg_dispatch_jobs;
pub mod msg_dispatch_jobs_read;
pub mod msg_connections;

// Audit tables
pub mod aud_logs;

// OAuth tables
pub mod oauth_identity_providers;
pub mod oauth_identity_provider_allowed_domains;
pub mod oauth_clients;
pub mod oauth_client_collections;
pub mod oauth_idp_role_mappings;
pub mod oauth_oidc_payloads;
