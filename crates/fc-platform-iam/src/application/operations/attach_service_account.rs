//! Attach-Service-Account-to-Application Use Case.
//!
//! Sets `Application.service_account_id` and emits
//! `ApplicationServiceAccountProvisioned`. Mutates the Application aggregate,
//! so it lives on the Application side rather than the ServiceAccount side.
//!
//! Called from `provision_service_account` in an orchestration tx alongside
//! `CreateServiceAccountUseCase`. The handler uses `PgUnitOfWork::run(...)`
//! so both commits live in one DB transaction — either both succeed or
//! both roll back.

use async_trait::async_trait;
use fc_platform_core::shared::id::ApplicationId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::ApplicationServiceAccountProvisioned;
use crate::application::repository::ApplicationRepository;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachServiceAccountToApplicationCommand {
    pub application_id: ApplicationId,
    /// The account's own id (`sac_…`), as sent: what the event and the
    /// audited command carry (Go `AttachServiceAccountCommand`).
    pub service_account_id: String,
    pub service_account_code: String,
    /// The account's SERVICE principal (`prn_…`), which the application
    /// stores (`app_applications.service_account_id` is a foreign key to
    /// `iam_principals`, as in Go: `app.ServiceAccountID = &saPrincipal.ID`).
    /// Resolved by the caller; not part of Go's command, so not audited.
    #[serde(skip)]
    pub service_principal_id: String,
}

impl AuditMasked for AttachServiceAccountToApplicationCommand {}

pub struct AttachServiceAccountToApplicationUseCase<U: UnitOfWork> {
    application_repo: Arc<ApplicationRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> AttachServiceAccountToApplicationUseCase<U> {
    pub fn new(application_repo: Arc<ApplicationRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            application_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for AttachServiceAccountToApplicationUseCase<U> {
    type Command = AttachServiceAccountToApplicationCommand;
    type Event = ApplicationServiceAccountProvisioned;

    async fn validate(
        &self,
        command: &AttachServiceAccountToApplicationCommand,
    ) -> Result<(), UseCaseError> {
        if command.application_id.as_str().trim().is_empty() {
            return Err(UseCaseError::validation(
                "APPLICATION_ID_REQUIRED",
                "Application ID is required",
            ));
        }
        if command.service_account_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "SERVICE_ACCOUNT_ID_REQUIRED",
                "Service account ID is required",
            ));
        }
        Ok(())
    }

    /// Applications are platform-owner data, written by anchors only (the
    /// rule every application handler applies).
    /// The handler's gate checks this, with the permission, before the body
    /// is read; here it holds for every caller (fc-web, orchestrations).
    async fn authorize(
        &self,
        _command: &AttachServiceAccountToApplicationCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(checks::require_anchor_scope(ctx.caller())?)
    }

    async fn execute(
        &self,
        command: AttachServiceAccountToApplicationCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<ApplicationServiceAccountProvisioned>, UseCaseError> {
        let mut application = self
            .application_repo
            .find_by_id(&command.application_id)
            .await
            .or_not_found(
                "APPLICATION_NOT_FOUND",
                format!("Application '{}' not found", command.application_id),
            )?;

        // Business rule: can't overwrite an existing service account.
        if application.service_account_id.is_some() {
            return Err(UseCaseError::business_rule(
                "APPLICATION_HAS_SERVICE_ACCOUNT",
                "Application already has a service account provisioned",
            ));
        }

        application.service_account_id = Some(command.service_principal_id.clone());
        application.updated_at = chrono::Utc::now();

        let event = ApplicationServiceAccountProvisioned::new(
            &ctx,
            &application.id,
            &application.code,
            &command.service_account_id,
            &command.service_account_code,
        );

        self.unit_of_work
            .commit(&application, &*self.application_repo, event, &command)
            .await
    }
}
