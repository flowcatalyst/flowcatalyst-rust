//! Sync an application's connections (Go `connection/operations/sync.go`):
//! the listed connections are created (source API) or updated (API/CODE
//! rows only; UI rows are left alone but still count as synced), and with
//! `removeUnlisted` the application's other API/CODE rows are deleted,
//! unless a subscription still uses one (409 `CONNECTION_REFERENCED`, and
//! nothing is written). Every connection carries the application's service
//! account. One transaction, one `platform:admin:connection:synced` event.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

use crate::connection::entity::Connection;
use crate::connection::repository::ConnectionRepository;
use crate::connection::sync_plan::{ConnectionSyncPlan, SOURCE_API, SOURCE_CODE};
use crate::impl_domain_event;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::{
    ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::{ApplicationRepository, SubscriptionRepository};

/// Go `ConnectionsSynced`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionsSynced {
    #[serde(flatten)]
    pub metadata: EventMetadata,
    pub application_code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub created: u32,
    pub updated: u32,
    pub deleted: u32,
    pub synced_codes: Vec<String>,
}

impl_domain_event!(ConnectionsSynced);

impl ConnectionsSynced {
    pub const EVENT_TYPE: &'static str = "platform:admin:connection:synced";
}

/// One listed connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncConnectionInput {
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncConnectionsCommand {
    pub application_id: String,
    pub application_code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub connections: Vec<SyncConnectionInput>,
    pub remove_unlisted: bool,
}

impl crate::usecase::AuditMasked for SyncConnectionsCommand {}

/// Go `validate.CodePattern`: `^[a-z][a-z0-9-]*$`.
fn is_code(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub struct SyncConnectionsUseCase<U: UnitOfWork> {
    connection_repo: Arc<ConnectionRepository>,
    application_repo: Arc<ApplicationRepository>,
    subscription_repo: Arc<SubscriptionRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> SyncConnectionsUseCase<U> {
    pub fn new(
        connection_repo: Arc<ConnectionRepository>,
        application_repo: Arc<ApplicationRepository>,
        subscription_repo: Arc<SubscriptionRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            connection_repo,
            application_repo,
            subscription_repo,
            unit_of_work,
        }
    }

    async fn plan(
        &self,
        command: &SyncConnectionsCommand,
        ctx: &ExecutionContext,
    ) -> Result<(ConnectionSyncPlan, ConnectionsSynced), UseCaseError> {
        let app = self
            .application_repo
            .find_by_id(&command.application_id)
            .await
            .or_not_found(
                "APPLICATION_NOT_FOUND",
                format!("Application not found: {}", command.application_code),
            )?;
        let service_account = app
            .service_account_id
            .clone()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| {
                UseCaseError::validation(
                    "APPLICATION_SERVICE_ACCOUNT_REQUIRED",
                    format!(
                        "Application '{}' has no provisioned service account; connections cannot be synced without one",
                        command.application_code
                    ),
                )
            })?;
        let existing = self
            .connection_repo
            .find_by_application_and_client(&command.application_code, command.client_id.as_deref())
            .await?;
        let syncable = |source: &str| source == SOURCE_API || source == SOURCE_CODE;

        let now = chrono::Utc::now();
        let (mut created, mut updated, mut deleted) = (0u32, 0u32, 0u32);
        let mut synced_codes = Vec::new();
        let mut saves = Vec::new();
        for input in &command.connections {
            let code = input.code.trim().to_lowercase();
            synced_codes.push(code.clone());
            match existing.iter().find(|(c, _)| c.code == code) {
                Some((_, source)) if !syncable(source) => {}
                Some((c, source)) => {
                    let mut c = c.clone();
                    c.name = input.name.trim().to_string();
                    c.description = input.description.clone();
                    c.external_id = input.external_id.clone();
                    c.service_account_id = service_account.clone();
                    c.updated_at = now;
                    saves.push((c, source.clone()));
                    updated += 1;
                }
                None => {
                    let mut c = Connection::new(&code, input.name.trim(), &service_account);
                    c.client_id = command.client_id.clone();
                    c.description = input.description.clone();
                    c.external_id = input.external_id.clone();
                    saves.push((c, SOURCE_API.to_string()));
                    created += 1;
                }
            }
        }

        let mut deletes = Vec::new();
        if command.remove_unlisted {
            let listed: HashSet<&str> = synced_codes.iter().map(String::as_str).collect();
            let candidates: Vec<&Connection> = existing
                .iter()
                .filter(|(c, s)| syncable(s) && !listed.contains(c.code.as_str()))
                .map(|(c, _)| c)
                .collect();
            let ids: Vec<String> = candidates.iter().map(|c| c.id.clone()).collect();
            let references = self.subscription_repo.codes_by_connection_ids(&ids).await?;
            for c in &candidates {
                let using: Vec<&str> = references
                    .iter()
                    .filter(|(cid, _)| *cid == c.id)
                    .map(|(_, code)| code.as_str())
                    .collect();
                if !using.is_empty() {
                    return Err(UseCaseError::business_rule(
                        "CONNECTION_REFERENCED",
                        format!(
                            "Connection '{}' cannot be removed: still referenced by subscription(s) {}",
                            c.code,
                            using.join(", ")
                        ),
                    ));
                }
            }
            deleted = ids.len() as u32;
            deletes = ids;
        }

        let app_code = &command.application_code;
        let group = if app_code.is_empty() {
            "platform:connections".to_string()
        } else {
            format!("platform:connections:{app_code}")
        };
        let event = ConnectionsSynced {
            metadata: EventMetadata::from_ctx(
                ctx,
                ConnectionsSynced::EVENT_TYPE,
                "1.0",
                "platform:admin",
                format!("platform.connections.{app_code}"),
                group,
            ),
            application_code: app_code.clone(),
            client_id: command.client_id.clone(),
            created,
            updated,
            deleted,
            synced_codes,
        };
        Ok((
            ConnectionSyncPlan {
                application_code: app_code.clone(),
                client_id: command.client_id.clone(),
                saves,
                deletes,
            },
            event,
        ))
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for SyncConnectionsUseCase<U> {
    type Command = SyncConnectionsCommand;
    type Event = ConnectionsSynced;

    async fn validate(&self, c: &SyncConnectionsCommand) -> Result<(), UseCaseError> {
        if c.application_code.trim().is_empty() {
            return Err(UseCaseError::validation(
                "APPLICATION_CODE_REQUIRED",
                "Application code is required",
            ));
        }
        let mut seen = HashSet::new();
        for input in &c.connections {
            let code = input.code.trim().to_lowercase();
            if code.is_empty() {
                return Err(UseCaseError::validation(
                    "CODE_REQUIRED",
                    "Connection code is required",
                ));
            }
            if !is_code(&code) {
                return Err(UseCaseError::validation(
                    "INVALID_CODE_FORMAT",
                    "Code must start with lowercase letter, contain only lowercase alphanumeric and hyphens",
                ));
            }
            if input.name.trim().is_empty() {
                return Err(UseCaseError::validation(
                    "NAME_REQUIRED",
                    "Connection name is required",
                ));
            }
            if !seen.insert(code.clone()) {
                return Err(UseCaseError::validation(
                    "DUPLICATE_CODE",
                    format!("Duplicate connection code '{code}' in sync request"),
                ));
            }
        }
        Ok(())
    }

    async fn authorize(
        &self,
        _c: &SyncConnectionsCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: SyncConnectionsCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<ConnectionsSynced> {
        let (plan, event) = match self.plan(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };
        self.unit_of_work
            .commit(&plan, &*self.connection_repo, event, &command)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::is_code;

    #[test]
    fn codes_are_lowercase_kebab() {
        assert!(is_code("orders-api2"));
        assert!(!is_code("2orders"));
        assert!(!is_code("Orders"));
        assert!(!is_code("a_b"));
    }
}
