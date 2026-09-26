//! Create Identity Provider Use Case (Go
//! `identityprovider/operations/create.go` `CreateIdentityProvider`): the
//! provider, and each listed email domain routed to it through the mappings
//! (created when unknown, claimed when routed elsewhere), in one transaction
//! when the handler runs it inside `PgUnitOfWork::run`.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::domains::{
    map_domain, normalize_domains, require_scope_for_new_domains, validate_domains,
    validate_mapping_scope, DomainDeps,
};
use super::events::IdentityProviderCreated;
use crate::identity_provider::entity::IdentityProviderType;
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult};
use crate::IdentityProviderRepository;

/// Command for creating a new identity provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateIdentityProviderCommand {
    pub code: String,
    pub name: String,
    pub idp_type: IdentityProviderType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_issuer_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_client_secret_ref: Option<String>,
    #[serde(default)]
    pub oidc_multi_tenant: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_issuer_pattern: Option<String>,
    /// The domains to route to the provider (mapped or claimed).
    #[serde(default)]
    pub allowed_email_domains: Vec<String>,
    /// The scope a brand-new mapping gets: `ANCHOR` or `CLIENT`. Required
    /// when a listed domain has no mapping yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mapping_scope: Option<String>,
    /// Linked on mappings that are new (CLIENT) or have no client yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_client_id: Option<String>,
    /// Reconcile users' IDP_SYNC roles from the token at login.
    #[serde(default)]
    pub sync_roles_from_idp: bool,
    /// Platform roles (by id) role sync may confer; empty = no restriction.
    #[serde(default)]
    pub allowed_role_ids: Vec<String>,
}

impl crate::usecase::AuditMasked for CreateIdentityProviderCommand {}

/// Use case for creating a new identity provider.
pub struct CreateIdentityProviderUseCase<U: UnitOfWork> {
    idp_repo: Arc<IdentityProviderRepository>,
    domains: DomainDeps,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> CreateIdentityProviderUseCase<U> {
    pub fn new(
        idp_repo: Arc<IdentityProviderRepository>,
        domains: DomainDeps,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            idp_repo,
            domains,
            unit_of_work,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for CreateIdentityProviderUseCase<U> {
    type Command = CreateIdentityProviderCommand;
    type Event = IdentityProviderCreated;

    async fn validate(&self, command: &CreateIdentityProviderCommand) -> Result<(), UseCaseError> {
        if command.code.trim().is_empty() {
            return Err(UseCaseError::validation(
                "CODE_REQUIRED",
                "code is required",
            ));
        }
        if command.name.trim().is_empty() {
            return Err(UseCaseError::validation(
                "NAME_REQUIRED",
                "name is required",
            ));
        }
        if command.idp_type == IdentityProviderType::Oidc {
            let blank = |v: &Option<String>| v.as_deref().is_none_or(|s| s.trim().is_empty());
            if blank(&command.oidc_issuer_url) {
                return Err(UseCaseError::validation(
                    "OIDC_ISSUER_REQUIRED",
                    "OIDC IDPs require oidcIssuerUrl",
                ));
            }
            if blank(&command.oidc_client_id) {
                return Err(UseCaseError::validation(
                    "OIDC_CLIENT_ID_REQUIRED",
                    "OIDC IDPs require oidcClientId",
                ));
            }
        }
        validate_domains(&command.allowed_email_domains)?;
        validate_mapping_scope(
            command.mapping_scope.as_deref(),
            command.primary_client_id.as_deref(),
        )?;
        super::require_sealed_secret(command.oidc_client_secret_ref.as_deref())
    }

    async fn authorize(
        &self,
        _command: &CreateIdentityProviderCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: CreateIdentityProviderCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<IdentityProviderCreated> {
        let (idp, event, scope, client, domains) = match self.prepare(&command, &ctx).await {
            Ok(v) => v,
            Err(e) => return UseCaseResult::failure(e),
        };

        let created = self
            .unit_of_work
            .commit(&idp, &*self.idp_repo, event, &command)
            .await;
        if created.as_result().is_err() {
            return created;
        }
        for domain in &domains {
            if let Err(e) = map_domain(
                &*self.unit_of_work,
                &self.domains,
                &idp,
                domain,
                scope,
                client.as_deref(),
                &ctx,
                &command,
            )
            .await
            {
                return UseCaseResult::failure(e);
            }
        }
        created
    }
}

type Prepared = (
    crate::IdentityProvider,
    IdentityProviderCreated,
    Option<crate::email_domain_mapping::entity::ScopeType>,
    Option<String>,
    Vec<String>,
);

impl<U: UnitOfWork> CreateIdentityProviderUseCase<U> {
    async fn prepare(
        &self,
        command: &CreateIdentityProviderCommand,
        ctx: &ExecutionContext,
    ) -> Result<Prepared, UseCaseError> {
        if self.idp_repo.find_by_code(&command.code).await?.is_some() {
            return Err(UseCaseError::business_rule(
                "CODE_EXISTS",
                format!(
                    "Identity provider with code '{}' already exists",
                    command.code
                ),
            ));
        }
        let (scope, client) = validate_mapping_scope(
            command.mapping_scope.as_deref(),
            command.primary_client_id.as_deref(),
        )?;
        let domains = normalize_domains(&command.allowed_email_domains);
        require_scope_for_new_domains(&self.domains, &domains, scope).await?;

        // Go stores the OIDC fields whatever the type.
        let mut idp = crate::IdentityProvider::new(&command.code, &command.name, command.idp_type);
        idp.oidc_issuer_url = command.oidc_issuer_url.clone();
        idp.oidc_client_id = command.oidc_client_id.clone();
        idp.oidc_client_secret_ref = command.oidc_client_secret_ref.clone();
        idp.oidc_multi_tenant = command.oidc_multi_tenant;
        idp.oidc_issuer_pattern = command.oidc_issuer_pattern.clone();
        idp.sync_roles_from_idp = command.sync_roles_from_idp;
        idp.allowed_role_ids = command.allowed_role_ids.clone();

        let event = IdentityProviderCreated::new(ctx, &idp.id, &idp.code);
        Ok((idp, event, scope, client, domains))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = CreateIdentityProviderCommand {
            code: "google-oidc".to_string(),
            name: "Google OIDC".to_string(),
            idp_type: IdentityProviderType::Oidc,
            oidc_issuer_url: Some("https://accounts.google.com".to_string()),
            oidc_client_id: Some("client-123".to_string()),
            oidc_client_secret_ref: None,
            oidc_multi_tenant: false,
            oidc_issuer_pattern: None,
            allowed_email_domains: vec![],
            mapping_scope: None,
            primary_client_id: None,
            sync_roles_from_idp: false,
            allowed_role_ids: vec![],
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("google-oidc"));
        assert!(json.contains("idpType"));
    }
}
