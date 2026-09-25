//! Move a user between client tiers (Go
//! `principal/operations/set_client_association.go`):
//!
//! - `clientId: "*"`                    → ANCHOR, no home client (mode ignored)
//! - `mode: CHANGE_CLIENT` + a client   → CLIENT, that client as home
//! - `mode: TO_PARTNER` + a client      → PARTNER, no home client; grants for
//!   the client and, when the user was a CLIENT user of another client, for
//!   that old home client too. Existing grants are kept.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::events::UserUpdated;
use crate::principal::entity::UserScope;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::{ClientRepository, PrincipalRepository};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetClientAssociationCommand {
    pub user_id: String,
    pub client_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

impl crate::usecase::AuditMasked for SetClientAssociationCommand {}

pub struct SetClientAssociationUseCase<U: UnitOfWork> {
    principal_repo: Arc<PrincipalRepository>,
    client_repo: Arc<ClientRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> SetClientAssociationUseCase<U> {
    pub fn new(
        principal_repo: Arc<PrincipalRepository>,
        client_repo: Arc<ClientRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            principal_repo,
            client_repo,
            unit_of_work,
        }
    }

    async fn require_client(&self, client_id: &str) -> Result<(), UseCaseError> {
        self.client_repo
            .find_by_id(client_id)
            .await
            .or_not_found("CLIENT_NOT_FOUND", format!("Client not found: {client_id}"))
            .map(|_| ())
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for SetClientAssociationUseCase<U> {
    type Command = SetClientAssociationCommand;
    type Event = UserUpdated;

    async fn validate(&self, c: &SetClientAssociationCommand) -> Result<(), UseCaseError> {
        if c.user_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "USER_ID_REQUIRED",
                "User ID is required",
            ));
        }
        if c.client_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "CLIENT_ID_REQUIRED",
                "clientId is required (use \"*\" for anchor)",
            ));
        }
        Ok(())
    }

    async fn authorize(
        &self,
        _c: &SetClientAssociationCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: SetClientAssociationCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<UserUpdated> {
        let mut p = match self
            .principal_repo
            .find_by_id(&command.user_id)
            .await
            .or_not_found(
                "USER_NOT_FOUND",
                format!("User not found: {}", command.user_id),
            ) {
            Ok(p) => p,
            Err(e) => return UseCaseResult::failure(e),
        };
        if !p.is_user() {
            return UseCaseResult::failure(UseCaseError::business_rule(
                "NOT_A_USER",
                "Client association only applies to USER principals",
            ));
        }
        let target = command.client_id.trim();
        let mode = command
            .mode
            .as_deref()
            .map(|m| m.trim().to_ascii_uppercase())
            .unwrap_or_default();
        if target == "*" {
            p.scope = UserScope::Anchor;
            p.client_id = None;
        } else if mode == "CHANGE_CLIENT" {
            if let Err(e) = self.require_client(target).await {
                return UseCaseResult::failure(e);
            }
            p.scope = UserScope::Client;
            p.client_id = Some(target.to_string());
        } else if mode == "TO_PARTNER" {
            if let Err(e) = self.require_client(target).await {
                return UseCaseResult::failure(e);
            }
            let mut grants = Vec::new();
            if p.scope == UserScope::Client {
                if let Some(home) = p
                    .client_id
                    .as_deref()
                    .filter(|h| !h.is_empty() && *h != target)
                {
                    grants.push(home.to_string());
                }
            }
            grants.push(target.to_string());
            for g in grants {
                if !p.assigned_clients.contains(&g) {
                    p.assigned_clients.push(g);
                }
            }
            p.scope = UserScope::Partner;
            p.client_id = None;
        } else {
            return UseCaseResult::failure(UseCaseError::validation(
                "MODE_REQUIRED",
                "mode must be CHANGE_CLIENT or TO_PARTNER for a specific clientId (use \"*\" for anchor)",
            ));
        }
        p.updated_at = chrono::Utc::now();
        let event = UserUpdated::new(&ctx, &p.id, &p.name);
        self.unit_of_work
            .commit(&p, &*self.principal_repo, event, &command)
            .await
    }
}
