//! Claiming and releasing a zone (Java `function/operations/ClaimFunctionDomain.java`
//! and `ReleaseFunctionDomain.java`; specs `function-zones-and-aliases.md`
//! §1 and `function-domains-no-dns.md`).
//!
//! A claim of `d` covers `d` and every hostname under it, and is usable at
//! once: there is no DNS verification. No two claims may nest, whoever owns
//! them: claiming `d` fails with `409 DOMAIN_TAKEN` when an existing claim
//! equals or covers `d`, or is covered by `d`. The error never names the
//! holder. Releasing fails with `409 DOMAIN_IN_USE`, naming the functions,
//! while any public route is under the zone.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde::{Serialize, Serializer};

use super::access::{domain_by_hostname, Caller};
use super::events::{DomainClaimed, DomainReleased};
use crate::function::domain_repository::FunctionDomainRepository;
use crate::function::entity::FunctionDomain;
use crate::function::repository::FunctionRepository;
use crate::function::route_repository::FunctionRouteRepository;
use crate::function::{FunctionOwner, Hostname};
use crate::usecase::{
    AuditMasked, ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};

fn domain_taken() -> UseCaseError {
    UseCaseError::business_rule("DOMAIN_TAKEN", "hostname is already claimed")
}

// ── Claim ───────────────────────────────────────────────────────────────────

/// `POST /api/function-domains`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimCommand {
    #[serde(serialize_with = "serialize_owner")]
    pub owner: FunctionOwner,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
}

fn serialize_owner<S: Serializer>(owner: &FunctionOwner, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(owner.to_wire())
}

impl AuditMasked for ClaimCommand {}

pub struct ClaimFunctionDomainUseCase<U: UnitOfWork> {
    pub(crate) domains: Arc<FunctionDomainRepository>,
    pub(crate) unit_of_work: Arc<U>,
    pub(crate) caller: Caller,
}

#[async_trait]
impl<U: UnitOfWork> UseCase for ClaimFunctionDomainUseCase<U> {
    type Command = ClaimCommand;
    type Event = DomainClaimed;

    async fn validate(&self, command: &ClaimCommand) -> Result<(), UseCaseError> {
        Hostname::parse(command.hostname.as_deref().unwrap_or(""))?;
        Ok(())
    }

    /// The command's own owner: nothing exists yet to hide, so a scope
    /// mismatch is a real 403.
    async fn authorize(
        &self,
        command: &ClaimCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        self.caller
            .check_scope_access(command.owner.client_id_or_none())
    }

    async fn execute(
        &self,
        command: ClaimCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<DomainClaimed> {
        let claimed = match self.prepare(&command).await {
            Ok(d) => d,
            Err(e) => return UseCaseResult::failure(e),
        };
        let event = DomainClaimed::new(&ctx, &claimed);
        self.unit_of_work
            .commit(&claimed, &*self.domains, event, &command)
            .await
    }
}

impl<U: UnitOfWork> ClaimFunctionDomainUseCase<U> {
    async fn prepare(&self, command: &ClaimCommand) -> Result<FunctionDomain, UseCaseError> {
        let hostname = Hostname::parse(command.hostname.as_deref().unwrap_or(""))?;
        // Equal to or covered by an existing claim: `covering` looks at the
        // hostname itself and each ancestor.
        if self.domains.covering(&hostname).await?.is_some() {
            return Err(domain_taken());
        }
        // Covering an existing claim.
        if self.domains.any_under(&hostname).await? {
            return Err(domain_taken());
        }
        Ok(FunctionDomain::claim(
            command.owner.clone(),
            hostname,
            Utc::now(),
        ))
    }
}

// ── Release ─────────────────────────────────────────────────────────────────

/// `DELETE /api/function-domains/{hostname}`. Any hostname under a zone
/// releases the zone that covers it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseCommand {
    pub hostname: String,
}

impl AuditMasked for ReleaseCommand {}

pub struct ReleaseFunctionDomainUseCase<U: UnitOfWork> {
    pub(crate) domains: Arc<FunctionDomainRepository>,
    pub(crate) routes: Arc<FunctionRouteRepository>,
    pub(crate) functions: Arc<FunctionRepository>,
    pub(crate) unit_of_work: Arc<U>,
    pub(crate) caller: Caller,
}

#[async_trait]
impl<U: UnitOfWork> UseCase for ReleaseFunctionDomainUseCase<U> {
    type Command = ReleaseCommand;
    type Event = DomainReleased;

    async fn validate(&self, _command: &ReleaseCommand) -> Result<(), UseCaseError> {
        Ok(())
    }

    /// Load-or-404 and reach are in `execute`, after the load.
    async fn authorize(
        &self,
        _command: &ReleaseCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: ReleaseCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<DomainReleased> {
        let domain = match self.prepare(&command).await {
            Ok(d) => d,
            Err(e) => return UseCaseResult::failure(e),
        };
        let event = DomainReleased::new(&ctx, &domain);
        self.unit_of_work
            .commit_delete(&domain, &*self.domains, event, &command)
            .await
    }
}

impl<U: UnitOfWork> ReleaseFunctionDomainUseCase<U> {
    async fn prepare(&self, command: &ReleaseCommand) -> Result<FunctionDomain, UseCaseError> {
        let hostname = Hostname::parse(&command.hostname)?;
        let domain = domain_by_hostname(&self.domains, &hostname, &self.caller).await?;
        let using = self.routes.list_under(&domain.hostname).await?;
        if !using.is_empty() {
            let mut function_ids: Vec<String> = Vec::new();
            for route in &using {
                if !function_ids.contains(&route.function_id) {
                    function_ids.push(route.function_id.clone());
                }
            }
            let addresses: HashMap<String, String> = self
                .functions
                .find_by_ids(&function_ids)
                .await?
                .into_iter()
                .map(|f| (f.id.clone(), f.address.render()))
                .collect();
            let mut names: Vec<String> = function_ids
                .iter()
                .map(|id| addresses.get(id).cloned().unwrap_or_else(|| id.clone()))
                .collect();
            names.sort();
            return Err(UseCaseError::business_rule(
                "DOMAIN_IN_USE",
                format!("domain is in use by: {}", names.join(", ")),
            ));
        }
        Ok(domain)
    }
}
