//! Java `function/operations/MarkVersionReady.java`: a `PUBLISHED` version
//! becomes `READY` the first time a host reports it loaded or registered
//! (spec `function-api.md` §6.2 step 2), with a `version:ready` event and an
//! audit row.
//!
//! The use case guards itself, atomically with the write: it reads the
//! version under its row lock (`SELECT … FOR UPDATE`, a
//! [`VersionById`] locked read) inside the transaction it commits in, so it
//! must run on a transaction-scoped unit of work (`PgUnitOfWork::run`), as
//! Java's `TxOperation`. Of two heartbeats racing for one version, the loser
//! waits on the lock, sees the winner's `READY`, and is refused with `409
//! VERSION_NOT_PUBLISHED`, which the heartbeat swallows: exactly one event,
//! never two, and never a failed heartbeat.
//!
//! No reach check: the caller is a host, gated on
//! `platform:function:host:control` by the control route, as in Java.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde::Serialize;

use super::access::resource_not_found;
use super::events::VersionReady;
use crate::function::entity::{FunctionVersion, VersionState};
use crate::function::repository::FunctionRepository;
use crate::function::version_repository::{FunctionVersionRepository, VersionById};
use crate::usecase::{
    AuditMasked, ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};

/// The refusal the heartbeat catches and ignores.
pub const VERSION_NOT_PUBLISHED: &str = "VERSION_NOT_PUBLISHED";

/// Java `MarkVersionReadyCommand`: not built from a request body; the
/// heartbeat resolves the reported address and version itself.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkVersionReadyCommand {
    pub version_id: String,
    /// The reporting host, carried onto the event.
    pub host_id: String,
}

impl AuditMasked for MarkVersionReadyCommand {}

pub struct MarkVersionReadyUseCase<U: UnitOfWork> {
    pub(crate) functions: Arc<FunctionRepository>,
    pub(crate) versions: Arc<FunctionVersionRepository>,
    pub(crate) unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> MarkVersionReadyUseCase<U> {
    pub fn new(
        functions: Arc<FunctionRepository>,
        versions: Arc<FunctionVersionRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            functions,
            versions,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for MarkVersionReadyUseCase<U> {
    type Command = MarkVersionReadyCommand;
    type Event = VersionReady;

    async fn validate(&self, _command: &MarkVersionReadyCommand) -> Result<(), UseCaseError> {
        Ok(())
    }

    /// Java `Operation.Authorize.publicAccess()`: the control route's gate
    /// is the whole authorization.
    async fn authorize(
        &self,
        _command: &MarkVersionReadyCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: MarkVersionReadyCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<VersionReady> {
        let (version, event) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };
        self.unit_of_work
            .commit(&version, &*self.versions, event, &command)
            .await
    }
}

impl<U: UnitOfWork> MarkVersionReadyUseCase<U> {
    async fn prepare(
        &self,
        command: &MarkVersionReadyCommand,
        ctx: &ExecutionContext,
    ) -> Result<(FunctionVersion, VersionReady), UseCaseError> {
        let mut v = self
            .unit_of_work
            .read_locked(&*self.versions, &VersionById(command.version_id.clone()))
            .await?
            .ok_or_else(|| resource_not_found("FunctionVersion", &command.version_id))?;
        if v.state != VersionState::Published {
            return Err(UseCaseError::business_rule(
                VERSION_NOT_PUBLISHED,
                format!(
                    "version {} is not PUBLISHED (already {})",
                    v.version,
                    v.state.name()
                ),
            ));
        }
        let f = self
            .functions
            .find_by_id(&v.function_id)
            .await?
            .ok_or_else(|| resource_not_found("Function", &v.function_id))?;
        v.mark_ready(Utc::now());
        let event = VersionReady::new(ctx, &f, &v, &command.host_id);
        Ok((v, event))
    }
}
