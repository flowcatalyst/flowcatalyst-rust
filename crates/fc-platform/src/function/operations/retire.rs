//! Java `function/operations/RetireVersion.java`: a version leaves service.
//! The live version never retires (`VERSION_IS_LIVE`, "promote another
//! version first"), nor one a named alias still points at
//! (`VERSION_ALIASED`, naming them, "move or remove them first"); a retired
//! version is refused (`VERSION_ALREADY_RETIRED`). Both alias checks read
//! the function, which is why it is loaded although only the version is
//! written.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde::Serialize;

use super::access::{function_by_address, resource_not_found};
use super::events::VersionRetired;
use crate::function::repository::FunctionRepository;
use crate::function::version_repository::FunctionVersionRepository;
use crate::function::FunctionAddress;
use crate::usecase::{AuditMasked, Committed, ExecutionContext, UnitOfWork, UseCase, UseCaseError};

/// `POST /api/functions/{address}/versions/{v}/retire`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetireCommand {
    #[serde(serialize_with = "super::serialize_address")]
    pub address: FunctionAddress,
    pub version: i32,
}

impl AuditMasked for RetireCommand {}

pub struct RetireVersionUseCase<U: UnitOfWork> {
    pub(crate) functions: Arc<FunctionRepository>,
    pub(crate) versions: Arc<FunctionVersionRepository>,
    pub(crate) unit_of_work: Arc<U>,
}

#[async_trait]
impl<U: UnitOfWork> UseCase for RetireVersionUseCase<U> {
    type Command = RetireCommand;
    type Event = VersionRetired;

    async fn validate(&self, _command: &RetireCommand) -> Result<(), UseCaseError> {
        Ok(())
    }

    /// Load-or-404 and reach are in `execute`, after the load.
    async fn authorize(
        &self,
        _command: &RetireCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: RetireCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<VersionRetired>, UseCaseError> {
        let f = function_by_address(&self.functions, &command.address, ctx.caller()).await?;
        let mut v = self
            .versions
            .find_by_function_and_version(&f.id, command.version)
            .await?
            .ok_or_else(|| {
                resource_not_found(
                    "FunctionVersion",
                    &format!("{}#{}", f.address.render(), command.version),
                )
            })?;
        if f.is_live(&v.id) {
            return Err(UseCaseError::business_rule(
                "VERSION_IS_LIVE",
                "promote another version first",
            ));
        }
        let mut pointing: Vec<&str> = f
            .aliases
            .iter()
            .filter(|a| a.version_id == v.id)
            .map(|a| a.alias.as_str())
            .collect();
        if !pointing.is_empty() {
            pointing.sort_unstable();
            return Err(UseCaseError::business_rule(
                "VERSION_ALIASED",
                format!(
                    "aliases {} point at this version; move or remove them first",
                    pointing.join(", ")
                ),
            ));
        }
        v.retire(Utc::now())?;
        let event = VersionRetired::new(&ctx, &f, &v);
        let (version, event) = (v, event);
        self.unit_of_work
            .commit(&version, &*self.versions, event, &command)
            .await
    }
}
