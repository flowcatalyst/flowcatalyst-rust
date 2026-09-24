//! Grant Platform Config Access Use Case.
//!
//! Creates OR updates a role→config-app access grant. A single use case
//! handles both create and update semantics, keyed by (application_code,
//! role_code), with `was_created` flag on the emitted event so consumers
//! can distinguish.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::PlatformConfigAccessGranted;
use crate::platform_config::access_entity::PlatformConfigAccess;
use crate::platform_config::access_repository::PlatformConfigAccessRepository;
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantPlatformConfigAccessCommand {
    pub application_code: String,
    pub role_code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_read: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_write: Option<bool>,
}

impl crate::usecase::AuditMasked for GrantPlatformConfigAccessCommand {}

pub struct GrantPlatformConfigAccessUseCase<U: UnitOfWork> {
    access_repo: Arc<PlatformConfigAccessRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> GrantPlatformConfigAccessUseCase<U> {
    pub fn new(access_repo: Arc<PlatformConfigAccessRepository>, unit_of_work: Arc<U>) -> Self {
        Self {
            access_repo,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for GrantPlatformConfigAccessUseCase<U> {
    type Command = GrantPlatformConfigAccessCommand;
    type Event = PlatformConfigAccessGranted;

    async fn validate(
        &self,
        command: &GrantPlatformConfigAccessCommand,
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
        _command: &GrantPlatformConfigAccessCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: GrantPlatformConfigAccessCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<PlatformConfigAccessGranted> {
        let (access, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        self.unit_of_work
            .commit(&access, &*self.access_repo, event, &command)
            .await
    }
}

impl<U: UnitOfWork> GrantPlatformConfigAccessUseCase<U> {
    async fn prepare(
        &self,
        command: &GrantPlatformConfigAccessCommand,
        ctx: &ExecutionContext,
    ) -> Result<(PlatformConfigAccess, PlatformConfigAccessGranted), UseCaseError> {
        let existing = self
            .access_repo
            .find_by_application_and_role(&command.application_code, &command.role_code)
            .await?;

        let (mut access, was_created) = match existing {
            Some(a) => (a, false),
            None => (
                PlatformConfigAccess::new(&command.application_code, &command.role_code),
                true,
            ),
        };

        if let Some(cr) = command.can_read {
            access.can_read = cr;
        }
        if let Some(cw) = command.can_write {
            access.can_write = cw;
        }

        let event = PlatformConfigAccessGranted {
            metadata: PlatformConfigAccessGranted::metadata_for(ctx, &access.id),
            access_id: access.id.clone(),
            application_code: access.application_code.clone(),
            role_code: access.role_code.clone(),
            can_read: access.can_read,
            can_write: access.can_write,
            was_created,
        };
        Ok((access, event))
    }
}
