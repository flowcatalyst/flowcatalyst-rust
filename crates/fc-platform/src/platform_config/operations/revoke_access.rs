//! Revoke Platform Config Access Use Case.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::PlatformConfigAccessRevoked;
use crate::platform_config::access_entity::PlatformConfigAccess;
use crate::platform_config::access_repository::PlatformConfigAccessRepository;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};

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
        Self {
            access_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for RevokePlatformConfigAccessUseCase<U> {
    type Command = RevokePlatformConfigAccessCommand;
    type Event = PlatformConfigAccessRevoked;

    async fn validate(
        &self,
        command: &RevokePlatformConfigAccessCommand,
    ) -> Result<(), UseCaseError> {
        if command.application_code.trim().is_empty() {
            return Err(UseCaseError::validation(
                "APP_CODE_REQUIRED",
                "Application code is required",
            ));
        }
        if command.role_code.trim().is_empty() {
            return Err(UseCaseError::validation(
                "ROLE_CODE_REQUIRED",
                "Role code is required",
            ));
        }
        Ok(())
    }

    async fn authorize(
        &self,
        _command: &RevokePlatformConfigAccessCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: RevokePlatformConfigAccessCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<PlatformConfigAccessRevoked> {
        let (access, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work
            .commit_delete(&access, &*self.access_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> RevokePlatformConfigAccessUseCase<U> {
    async fn prepare(
        &self,
        command: &RevokePlatformConfigAccessCommand,
        ctx: &ExecutionContext,
    ) -> Result<(PlatformConfigAccess, PlatformConfigAccessRevoked), UseCaseError> {
        let access = self
            .access_repo
            .find_by_application_and_role(&command.application_code, &command.role_code)
            .await
            .or_not_found(
                "ACCESS_NOT_FOUND",
                format!(
                    "Config access for {}/{} not found",
                    command.application_code, command.role_code
                ),
            )?;

        let event = PlatformConfigAccessRevoked::new(
            ctx,
            &access.id,
            &access.application_code,
            &access.role_code,
        );
        Ok((access, event))
    }
}
