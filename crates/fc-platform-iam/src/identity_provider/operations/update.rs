//! Update Identity Provider Use Case (Go
//! `identityprovider/operations/update.go` `UpdateIdentityProvider`): the
//! provider's fields, and — when `allowedEmailDomains` is sent — the mappings
//! reconciled with it: additions mapped or claimed as on create, removals
//! falling back to the internal provider (its users handed back to internal
//! auth). One transaction when the handler runs it inside
//! `PgUnitOfWork::run`.

use async_trait::async_trait;
use fc_platform_core::shared::id::IdentityProviderId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::domains::{
    map_domain, move_mapping, normalize_domains, require_scope_for_new_domains, validate_domains,
    validate_mapping_scope, DomainDeps, INTERNAL_IDP_CODE,
};
use super::events::IdentityProviderUpdated;
use crate::email_domain_mapping::entity::EmailDomainMapping;
use crate::identity_provider::entity::IdentityProvider;
use crate::identity_provider::repository::IdentityProviderRepository;
use crate::role::ceiling;
use crate::role::repository::RoleRepository;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::usecase::AuditMasked;
use fc_platform_core::usecase::{
    Committed, ExecutionContext, OrNotFound, UnitOfWork, UseCase, UseCaseError,
};

/// Command for updating an existing identity provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateIdentityProviderCommand {
    pub idp_id: IdentityProviderId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_issuer_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_client_secret_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_multi_tenant: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oidc_issuer_pattern: Option<String>,
    /// The desired set of routed domains; absent leaves them as they are.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_email_domains: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mapping_scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_client_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync_roles_from_idp: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_role_ids: Option<Vec<String>>,
}

impl AuditMasked for UpdateIdentityProviderCommand {}

/// Use case for updating an existing identity provider.
pub struct UpdateIdentityProviderUseCase<U: UnitOfWork> {
    idp_repo: Arc<IdentityProviderRepository>,
    domains: DomainDeps,
    unit_of_work: Arc<U>,
    /// The allow-list's role ceiling (owner ruling 14).
    role_repo: Arc<RoleRepository>,
}

impl<U: UnitOfWork> UpdateIdentityProviderUseCase<U> {
    pub fn new(
        idp_repo: Arc<IdentityProviderRepository>,
        domains: DomainDeps,
        unit_of_work: Arc<U>,
        role_repo: Arc<RoleRepository>,
    ) -> Self {
        Self {
            idp_repo,
            domains,
            unit_of_work,
            role_repo,
        }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for UpdateIdentityProviderUseCase<U> {
    type Command = UpdateIdentityProviderCommand;
    type Event = IdentityProviderUpdated;

    async fn validate(&self, command: &UpdateIdentityProviderCommand) -> Result<(), UseCaseError> {
        if command.idp_id.as_str().trim().is_empty() {
            return Err(UseCaseError::validation("ID_REQUIRED", "id is required"));
        }
        if command.name.as_deref().is_some_and(|n| n.trim().is_empty()) {
            return Err(UseCaseError::validation(
                "NAME_REQUIRED",
                "name cannot be empty",
            ));
        }
        if let Some(domains) = &command.allowed_email_domains {
            validate_domains(domains)?;
        }
        validate_mapping_scope(
            command.mapping_scope.as_deref(),
            command.primary_client_id.as_deref(),
        )?;
        super::require_sealed_secret(command.oidc_client_secret_ref.as_deref())
    }

    /// Anchors only, as on create. A changed allow-list is bounded by the
    /// caller's role ceiling against the stored one (owner ruling 14); a missing
    /// provider is left to `execute`'s 404.
    async fn authorize(
        &self,
        command: &UpdateIdentityProviderCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        checks::require_anchor_scope(ctx.caller())?;
        if let Some(after) = command.allowed_role_ids.as_deref() {
            if let Some(existing) = self.idp_repo.find_by_id(&command.idp_id).await? {
                ceiling::require_role_ref_change(
                    ctx.caller(),
                    &self.role_repo,
                    &existing.allowed_role_ids,
                    after,
                )
                .await?;
            }
        }
        Ok(())
    }

    async fn execute(
        &self,
        command: UpdateIdentityProviderCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<IdentityProviderUpdated>, UseCaseError> {
        let mut idp = self
            .idp_repo
            .find_by_id(&command.idp_id)
            .await
            .or_not_found(
                "NOT_FOUND",
                format!("Identity provider with ID '{}' not found", command.idp_id),
            )?;

        // Without a scope, every listed domain must already be mapped;
        // checked before anything is written.
        if let Some(domains) = &command.allowed_email_domains {
            let (scope, _) = validate_mapping_scope(
                command.mapping_scope.as_deref(),
                command.primary_client_id.as_deref(),
            )?;
            require_scope_for_new_domains(&self.domains, &normalize_domains(domains), scope)
                .await?;
        }

        if let Some(ref name) = command.name {
            idp.name = name.trim().to_string();
        }
        if let Some(ref issuer_url) = command.oidc_issuer_url {
            idp.oidc_issuer_url = Some(issuer_url.clone());
        }
        if let Some(ref client_id) = command.oidc_client_id {
            idp.oidc_client_id = Some(client_id.clone());
        }
        if let Some(ref secret_ref) = command.oidc_client_secret_ref {
            idp.oidc_client_secret_ref = Some(secret_ref.clone());
        }
        if let Some(mt) = command.oidc_multi_tenant {
            idp.oidc_multi_tenant = mt;
        }
        if let Some(ref pattern) = command.oidc_issuer_pattern {
            idp.oidc_issuer_pattern = Some(pattern.clone());
        }
        if let Some(sync) = command.sync_roles_from_idp {
            idp.sync_roles_from_idp = sync;
        }
        if let Some(ref roles) = command.allowed_role_ids {
            idp.allowed_role_ids = roles.clone();
        }
        idp.updated_at = chrono::Utc::now();

        // A provider that is (or becomes) multi-tenant must not route any
        // domain whose mapping pins no tenant (owner ruling 2026-09-25,
        // item 3). New and claimed mappings are checked as they are written.
        if idp.oidc_multi_tenant {
            let unpinned = self
                .domains
                .edm_repo
                .find_unpinned_domains_for_identity_provider(&idp.id)
                .await?;
            // Domains left out of a sent list are released, so they need no
            // pin; every other routed domain does.
            let desired: Option<Vec<String>> = command
                .allowed_email_domains
                .as_deref()
                .map(normalize_domains);
            let still_routed: Vec<String> = unpinned
                .into_iter()
                .filter(|d| desired.as_ref().is_none_or(|keep| keep.contains(d)))
                .collect();
            if !still_routed.is_empty() {
                return Err(UseCaseError::validation(
                    "TENANT_PIN_REQUIRED",
                    format!(
                        "A multi-tenant identity provider needs every mapped domain to pin its tenant; set requiredOidcTenantId on: {}",
                        still_routed.join(", ")
                    ),
                ));
            }
        }

        let event = IdentityProviderUpdated::new(&ctx, &idp.id, &idp.code);

        let updated = self
            .unit_of_work
            .commit(&idp, &*self.idp_repo, event, &command)
            .await?;
        self.reconcile_domains(&idp, &command, &ctx).await?;
        Ok(updated)
    }
}

impl<U: UnitOfWork> UpdateIdentityProviderUseCase<U> {
    /// Go's domain reconciliation: map (or claim) every desired domain, then
    /// fall every other domain routed here back to the internal provider.
    async fn reconcile_domains(
        &self,
        idp: &IdentityProvider,
        command: &UpdateIdentityProviderCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        let Some(domains) = &command.allowed_email_domains else {
            return Ok(());
        };
        let desired = normalize_domains(domains);
        let (scope, client) = validate_mapping_scope(
            command.mapping_scope.as_deref(),
            command.primary_client_id.as_deref(),
        )?;
        let current = self
            .domains
            .edm_repo
            .find_by_identity_provider(&idp.id)
            .await?;

        for domain in &desired {
            map_domain(
                &*self.unit_of_work,
                &self.domains,
                idp,
                domain,
                scope,
                client.as_deref(),
                ctx,
                command,
            )
            .await?;
        }

        // Removals keep the mapping (and its client and 2FA settings); only
        // the routing changes. The internal provider releases nothing.
        if idp.code == INTERNAL_IDP_CODE {
            return Ok(());
        }
        let released: Vec<&EmailDomainMapping> = current
            .iter()
            .filter(|m| !desired.contains(&m.email_domain))
            .collect();
        let Some(first) = released.first() else {
            return Ok(());
        };
        let internal = self
            .idp_repo
            .find_by_code(INTERNAL_IDP_CODE)
            .await?
            .ok_or_else(|| {
                UseCaseError::internal(
                    "SEED",
                    format!(
                        "internal identity provider missing; cannot release domain '{}'",
                        first.email_domain
                    ),
                )
            })?;
        for mapping in released {
            move_mapping(
                &*self.unit_of_work,
                &self.domains,
                mapping,
                &internal,
                None,
                ctx,
                command,
            )
            .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = UpdateIdentityProviderCommand {
            idp_id: IdentityProviderId::from_wire("idp-123"),
            name: Some("Updated Name".to_string()),
            oidc_issuer_url: None,
            oidc_client_id: None,
            oidc_client_secret_ref: None,
            oidc_multi_tenant: None,
            oidc_issuer_pattern: None,
            allowed_email_domains: None,
            mapping_scope: None,
            primary_client_id: None,
            sync_roles_from_idp: None,
            allowed_role_ids: None,
        };

        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("idpId"));
        assert!(json.contains("Updated Name"));
    }
}
