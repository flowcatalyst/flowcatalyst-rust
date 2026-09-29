//! The audit record of an administrator minting an access token for a
//! service account (`POST /api/service-accounts/{id}/token`).
//!
//! Go writes a best-effort `aud_logs` row (`TOKEN_MINTED_BY_ADMIN`) and no
//! event. Rust records it through the unit of work, so the audit row comes
//! with a `platform:iam:serviceaccount:token-minted` event (a Rust addition).
//! The token itself is minted by the handler; nothing is persisted.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use fc_platform_core::impl_domain_event;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::usecase::domain_event::EventMetadata;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{Committed, ExecutionContext, UnitOfWork, UseCase, UseCaseError};

/// An administrator minted a token for a service account.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccountTokenMinted {
    #[serde(flatten)]
    pub metadata: EventMetadata,
    pub service_account_id: String,
    pub code: String,
}

impl_domain_event!(ServiceAccountTokenMinted);

impl ServiceAccountTokenMinted {
    pub const EVENT_TYPE: &'static str = "platform:iam:serviceaccount:token-minted";

    pub fn new(ctx: &ExecutionContext, service_account_id: &str, code: &str) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                "1.0",
                "platform:iam",
                format!("platform.serviceaccount.{}", service_account_id),
                format!("platform:serviceaccount:{}", service_account_id),
            ),
            service_account_id: service_account_id.to_string(),
            code: code.to_string(),
        }
    }
}

/// Record a token mint (Go audit operation `TOKEN_MINTED_BY_ADMIN`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MintServiceAccountTokenCommand {
    pub service_account_id: String,
    pub code: String,
}

impl AuditMasked for MintServiceAccountTokenCommand {}

pub struct RecordServiceAccountTokenMintUseCase<U: UnitOfWork> {
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> RecordServiceAccountTokenMintUseCase<U> {
    pub fn new(unit_of_work: Arc<U>) -> Self {
        Self { unit_of_work }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for RecordServiceAccountTokenMintUseCase<U> {
    type Command = MintServiceAccountTokenCommand;
    type Event = ServiceAccountTokenMinted;

    async fn validate(&self, c: &MintServiceAccountTokenCommand) -> Result<(), UseCaseError> {
        if c.service_account_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "SERVICE_ACCOUNT_ID_REQUIRED",
                "Service account ID is required",
            ));
        }
        Ok(())
    }

    /// Service accounts are written by anchors only: an account's tier follows
    /// its client links, so a non-anchor could otherwise mint an ANCHOR-tier
    /// account (the `can_*_service_accounts` rules).
    /// The handler's gate checks this, with the permission, before the body
    /// is read; here it holds for every caller (fc-web, orchestrations).
    async fn authorize(
        &self,
        _command: &MintServiceAccountTokenCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(checks::require_anchor_scope(ctx.caller())?)
    }

    async fn execute(
        &self,
        command: MintServiceAccountTokenCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<ServiceAccountTokenMinted>, UseCaseError> {
        let event =
            ServiceAccountTokenMinted::new(&ctx, &command.service_account_id, &command.code);
        self.unit_of_work.emit_event(event, &command).await
    }
}
