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

use std::sync::Arc;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::ApplicationRepository;
use crate::usecase::{
    ExecutionContext, UseCase, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::ApplicationServiceAccountProvisioned;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachServiceAccountToApplicationCommand {
    pub application_id: String,
    pub service_account_id: String,
    pub service_account_code: String,
}

pub struct AttachServiceAccountToApplicationUseCase<U: UnitOfWork> {
    application_repo: Arc<ApplicationRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> AttachServiceAccountToApplicationUseCase<U> {
    pub fn new(application_repo: Arc<ApplicationRepository>, unit_of_work: Arc<U>) -> Self {
        Self { application_repo, unit_of_work }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for AttachServiceAccountToApplicationUseCase<U> {
    type Command = AttachServiceAccountToApplicationCommand;
    type Event = ApplicationServiceAccountProvisioned;

    async fn validate(&self, command: &AttachServiceAccountToApplicationCommand) -> Result<(), UseCaseError> {
        if command.application_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "APPLICATION_ID_REQUIRED", "Application ID is required",
            ));
        }
        if command.service_account_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "SERVICE_ACCOUNT_ID_REQUIRED", "Service account ID is required",
            ));
        }
        Ok(())
    }

    async fn authorize(&self, _command: &AttachServiceAccountToApplicationCommand, _ctx: &ExecutionContext) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: AttachServiceAccountToApplicationCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ApplicationServiceAccountProvisioned> {
        let mut application = match self.application_repo.find_by_id(&command.application_id).await {
            Ok(Some(a)) => a,
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "APPLICATION_NOT_FOUND",
                    format!("Application '{}' not found", command.application_id),
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "fetch application: {}", e,
                )));
            }
        };

        // Business rule: can't overwrite an existing service account.
        if application.service_account_id.is_some() {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "APPLICATION_HAS_SERVICE_ACCOUNT",
                "Application already has a service account provisioned",
            ));
        }

        application.service_account_id = Some(command.service_account_id.clone());
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
