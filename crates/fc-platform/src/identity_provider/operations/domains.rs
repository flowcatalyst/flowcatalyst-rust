//! The email-domain side of an identity-provider write (Go
//! `identityprovider/operations/create.go` `mapDomainTx` and friends): the
//! domains a create or update names are routed to the provider through the
//! email-domain mappings, the one source of domain → provider routing. A
//! domain with no mapping gets one, a domain routed elsewhere is claimed
//! (moved, with the move's side effects), and on update a domain dropped
//! from the list falls back to the internal provider.
//!
//! Every write goes through the unit of work the use case holds; the handler
//! runs the use case in one transaction (`PgUnitOfWork::run`), so the
//! provider and its mappings land together or not at all.

use serde::Serialize;
use std::sync::Arc;

use crate::email_domain_mapping::entity::{EmailDomainMapping, ScopeType};
use crate::email_domain_mapping::operations::move_provider::plan_move;
use crate::email_domain_mapping::operations::{
    EmailDomainMappingCreated, EmailDomainMappingUpdated,
};
use crate::email_domain_mapping::provider_move_repository::ProviderMoveRepository;
use crate::usecase::{AuditMasked, ExecutionContext, UnitOfWork, UseCaseError};
use crate::{EmailDomainMappingRepository, IdentityProvider, PrincipalRepository};

/// The seeded internal (password) provider's code: a domain released from
/// another provider falls back to it, and it cannot be deleted.
pub const INTERNAL_IDP_CODE: &str = "internal";

/// The repositories the domain orchestration writes through.
#[derive(Clone)]
pub struct DomainDeps {
    pub edm_repo: Arc<EmailDomainMappingRepository>,
    pub principal_repo: Arc<PrincipalRepository>,
    pub move_repo: Arc<ProviderMoveRepository>,
}

/// Lower-cased, trimmed, blank-free and de-duplicated, order kept.
pub fn normalize_domains(domains: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(domains.len());
    for d in domains {
        let n = d.trim().to_lowercase();
        if !n.is_empty() && !out.contains(&n) {
            out.push(n);
        }
    }
    out
}

/// Go `validateDomains`: the mapping create's DNS-name shape check.
pub fn validate_domains(domains: &[String]) -> Result<(), UseCaseError> {
    for d in normalize_domains(domains) {
        if !d.contains('.') || d.contains([' ', '/', '@']) {
            return Err(UseCaseError::validation(
                "INVALID_EMAIL_DOMAIN",
                format!("Email domain '{d}' must be a valid DNS name (e.g. example.com)"),
            ));
        }
    }
    Ok(())
}

/// Go `validateMappingScope`: the scope new mappings get (ANCHOR or CLIENT;
/// PARTNER mappings are managed on the email-domain page), with the client
/// CLIENT needs and ANCHOR forbids, and no client without a scope. Returns
/// the scope (none when neither field is set) and the trimmed client id.
pub fn validate_mapping_scope(
    mapping_scope: Option<&str>,
    primary_client_id: Option<&str>,
) -> Result<(Option<ScopeType>, Option<String>), UseCaseError> {
    let client = primary_client_id
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(str::to_string);
    let Some(scope) = mapping_scope else {
        if client.is_some() {
            return Err(UseCaseError::validation(
                "MAPPING_SCOPE_REQUIRED",
                "mappingScope is required when primaryClientId is set",
            ));
        }
        return Ok((None, None));
    };
    let scope = match scope {
        "ANCHOR" => ScopeType::Anchor,
        "CLIENT" => ScopeType::Client,
        _ => {
            return Err(UseCaseError::validation(
                "INVALID_MAPPING_SCOPE",
                "mappingScope must be ANCHOR or CLIENT; partner mappings are managed on the email-domain page",
            ))
        }
    };
    match scope {
        ScopeType::Client if client.is_none() => Err(UseCaseError::validation(
            "PRIMARY_CLIENT_REQUIRED",
            "primaryClientId is required when mappingScope is CLIENT",
        )),
        ScopeType::Anchor if client.is_some() => Err(UseCaseError::validation(
            "PRIMARY_CLIENT_NOT_ALLOWED",
            "primaryClientId is not allowed when mappingScope is ANCHOR",
        )),
        _ => Ok((Some(scope), client)),
    }
}

/// Go `requireScopeForNewDomains`: without a scope, every domain must
/// already have a mapping (a claim or a link needs no scope choice). Checked
/// before anything is written.
pub async fn require_scope_for_new_domains(
    deps: &DomainDeps,
    domains: &[String],
    scope: Option<ScopeType>,
) -> Result<(), UseCaseError> {
    if scope.is_some() {
        return Ok(());
    }
    for d in domains {
        if deps.edm_repo.find_by_email_domain(d).await?.is_none() {
            return Err(UseCaseError::validation(
                "MAPPING_SCOPE_REQUIRED",
                format!(
                    "mappingScope is required: domain '{d}' has no mapping yet; choose ANCHOR or CLIENT"
                ),
            ));
        }
    }
    Ok(())
}

/// Go `mapDomainTx`: route `domain` to `idp`. A new mapping gets `scope`
/// (and `client` when CLIENT); a mapping already here gains `client` only
/// when it has none; a mapping routed elsewhere is claimed (moved), gaining
/// `client` only when it has none. A mapping's scope is never changed.
#[allow(clippy::too_many_arguments)]
pub async fn map_domain<U, C>(
    uow: &U,
    deps: &DomainDeps,
    idp: &IdentityProvider,
    domain: &str,
    scope: Option<ScopeType>,
    client: Option<&str>,
    ctx: &ExecutionContext,
    command: &C,
) -> Result<(), UseCaseError>
where
    U: UnitOfWork,
    C: Serialize + AuditMasked + Send + Sync,
{
    let Some(mut existing) = deps.edm_repo.find_by_email_domain(domain).await? else {
        let scope = scope.ok_or_else(|| {
            UseCaseError::internal(
                "INVARIANT_MAPPING_SCOPE",
                format!("a new mapping for '{domain}' has no scope"),
            )
        })?;
        let mut mapping = EmailDomainMapping::new(domain, &idp.id, scope);
        if scope == ScopeType::Client {
            mapping.primary_client_id = client.map(str::to_string);
        }
        crate::email_domain_mapping::operations::require_tenant_pin(
            idp.oidc_multi_tenant,
            &mapping,
        )?;
        let event = EmailDomainMappingCreated::new(ctx, &mapping.id, &mapping.email_domain);
        return uow
            .commit(&mapping, &*deps.edm_repo, event, command)
            .await
            .into_result()
            .map(|_| ());
    };
    let link = client
        .filter(|_| existing.primary_client_id.is_none())
        .map(str::to_string);
    if existing.identity_provider_id == idp.id {
        let Some(link) = link else {
            return Ok(());
        };
        existing.primary_client_id = Some(link);
        existing.updated_at = chrono::Utc::now();
        let event = EmailDomainMappingUpdated::new(ctx, &existing.id, &existing.email_domain);
        return uow
            .commit(&existing, &*deps.edm_repo, event, command)
            .await
            .into_result()
            .map(|_| ());
    }
    move_mapping(uow, deps, &existing, idp, link, ctx, command).await
}

/// Re-point `mapping` to `target` with the move's side effects (Go
/// `MoveMappingTx`).
pub async fn move_mapping<U, C>(
    uow: &U,
    deps: &DomainDeps,
    mapping: &EmailDomainMapping,
    target: &IdentityProvider,
    link_client: Option<String>,
    ctx: &ExecutionContext,
    command: &C,
) -> Result<(), UseCaseError>
where
    U: UnitOfWork,
    C: Serialize + AuditMasked + Send + Sync,
{
    let (mv, event) = plan_move(&deps.principal_repo, mapping, target, link_client, ctx).await?;
    uow.commit(&mv, &*deps.move_repo, event, command)
        .await
        .into_result()
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domains_are_normalised_and_deduplicated() {
        let got = normalize_domains(&[
            " Acme.COM ".to_string(),
            "acme.com".to_string(),
            "".to_string(),
            "b.test".to_string(),
        ]);
        assert_eq!(got, ["acme.com", "b.test"]);
    }

    #[test]
    fn the_mapping_scope_contract_is_gos() {
        assert!(matches!(
            validate_mapping_scope(None, None),
            Ok((None, None))
        ));
        let err = validate_mapping_scope(None, Some("clt_1")).unwrap_err();
        assert_eq!(err.code(), "MAPPING_SCOPE_REQUIRED");
        let err = validate_mapping_scope(Some("PARTNER"), None).unwrap_err();
        assert_eq!(err.code(), "INVALID_MAPPING_SCOPE");
        let err = validate_mapping_scope(Some("CLIENT"), Some(" ")).unwrap_err();
        assert_eq!(err.code(), "PRIMARY_CLIENT_REQUIRED");
        let err = validate_mapping_scope(Some("ANCHOR"), Some("clt_1")).unwrap_err();
        assert_eq!(err.code(), "PRIMARY_CLIENT_NOT_ALLOWED");
        let (scope, client) = validate_mapping_scope(Some("CLIENT"), Some(" clt_1 ")).unwrap();
        assert_eq!(scope, Some(ScopeType::Client));
        assert_eq!(client.as_deref(), Some("clt_1"));
    }

    #[test]
    fn domains_must_look_like_dns_names() {
        assert!(validate_domains(&["acme.com".to_string()]).is_ok());
        let err = validate_domains(&["nodot".to_string()]).unwrap_err();
        assert_eq!(err.code(), "INVALID_EMAIL_DOMAIN");
    }
}
