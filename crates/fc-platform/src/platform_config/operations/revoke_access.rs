//! Revoke Platform Config Access Use Case.

use std::sync::Arc;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::platform_config::access_repository::PlatformConfigAccessRepository;
use crate::usecase::{
    ExecutionContext, UseCase, UnitOfWork, UseCaseError, UseCaseResult,
};
use super::events::PlatformConfigAccessRevoked;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevokePlatformConfigAccessCommand {
    pub application_code: String,
    pub role_code: String,
}

pub struct RevokePlatformConfigAccessUseCase<U: UnitOfWork> {
    access_repo: Arc<PlatformConfigAccessRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> RevokePlatformConfigAccessUseCase<U> {
    pub fn new(access_repo: Arc<PlatformConfigAccessRepository>, unit_of_work: Arc<U>) -> Self {
        Self { access_repo, unit_of_work }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for RevokePlatformConfigAccessUseCase<U> {
    type Command = RevokePlatformConfigAccessCommand;
    type Event = PlatformConfigAccessRevoked;

    async fn validate(&self, command: &RevokePlatformConfigAccessCommand) -> Result<(), UseCaseError> {
        if command.application_code.trim().is_empty() {
            return Err(UseCaseError::validation("APP_CODE_REQUIRED", "Application code is required"));
        }
        if command.role_code.trim().is_empty() {
            return Err(UseCaseError::validation("ROLE_CODE_REQUIRED", "Role code is required"));
        }
        Ok(())
    }

    async fn authorize(&self, _command: &RevokePlatformConfigAccessCommand, _ctx: &ExecutionContext) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: RevokePlatformConfigAccessCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<PlatformConfigAccessRevoked> {
        let access = match self.access_repo
            .find_by_application_and_role(&command.application_code, &command.role_code)
            .await
        {
            Ok(Some(a)) => a,
            Ok(None) => {
                return UseCaseResult::failure(UseCaseError::not_found(
                    "ACCESS_NOT_FOUND",
                    format!("Config access for {}/{} not found", command.application_code, command.role_code),
                ));
            }
            Err(e) => {
                return UseCaseResult::failure(UseCaseError::commit(format!(
                    "fetch access: {}", e,
                )));
            }
        };

        let event = PlatformConfigAccessRevoked::new(
            &ctx,
            &access.id,
            &access.application_code,
            &access.role_code,
        );

        self.unit_of_work
            .commit_delete(&access, &*self.access_repo, event, &command)
            .await
    }
}
