//! SeaORM Entity Models
//!
//! Database entity models generated from the PostgreSQL schema.
//! These map directly to database tables and are used by repositories.
//! Domain entities (in each aggregate's entity.rs) are the API-facing types.

pub mod tnt_clients;
pub mod iam_roles;
pub mod iam_permissions;
pub mod iam_role_permissions;
pub mod iam_principals;
pub mod iam_principal_roles;
pub mod iam_service_accounts;
pub mod iam_client_access_grants;
pub mod iam_principal_application_access;
