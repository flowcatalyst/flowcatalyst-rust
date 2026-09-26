//! The command `POST /api/applications/{id}/provision-service-account`
//! records.
//!
//! Provisioning runs three use cases in one transaction (create the service
//! account, attach it to the application, mint its OAuth client), each
//! writing its own domain event. Go runs them as one operation and records
//! its single `ProvisionServiceAccountCommand` on each of the three audit
//! rows (application/operations/provision_service_account.go, every
//! `usecasepgx.CommitScoped` given `cmd`), so the handler runs them under
//! [`PgUnitOfWork::run_as`](crate::usecase::PgUnitOfWork::run_as) with this
//! command.

use serde::{Deserialize, Serialize};

/// Go `ProvisionServiceAccountCommand`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvisionServiceAccountCommand {
    pub application_id: String,
}

impl crate::usecase::AuditMasked for ProvisionServiceAccountCommand {}
