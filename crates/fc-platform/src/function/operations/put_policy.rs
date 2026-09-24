//! Java `function/operations/PutFunctionPolicy.java`: replaces a client's
//! (or the platform's) signer allow-list and resource ceilings wholesale.
//! The handler gates anchor scope and `platform:function:policy:manage`;
//! this only checks that a client owner exists.

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde::{Serialize, Serializer};

use super::access::resource_not_found;
use super::events::PolicyUpdated;
use crate::function::entity::{ClientPolicy, SignerRule};
use crate::function::policy_repository::ClientPolicyRepository;
use crate::function::{java_is_blank, FunctionOwner, Runtime};
use crate::usecase::{
    AuditMasked, ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult,
};
use crate::ClientRepository;

/// One signer rule as sent; runtimes still raw.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignerInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    pub runtimes: Vec<String>,
}

/// `PUT /api/function-policies/{owner}`: a full replacement. An absent
/// ceiling means the platform default applies.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PutPolicyCommand {
    #[serde(serialize_with = "serialize_owner")]
    pub owner: FunctionOwner,
    pub signers: Vec<SignerInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_duration_ms: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_concurrency: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_wasm_memory_mb: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_db_pool_size: Option<i32>,
}

fn serialize_owner<S: Serializer>(owner: &FunctionOwner, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(owner.to_wire())
}

impl AuditMasked for PutPolicyCommand {}

pub struct PutFunctionPolicyUseCase<U: UnitOfWork> {
    pub(crate) policies: Arc<ClientPolicyRepository>,
    pub(crate) clients: Arc<ClientRepository>,
    pub(crate) unit_of_work: Arc<U>,
}

fn require_valid_ceiling(value: Option<i32>, field: &str) -> Result<(), UseCaseError> {
    match value {
        Some(v) if v <= 0 => Err(UseCaseError::validation(
            "CEILING_INVALID",
            format!("{field} must be a positive integer"),
        )),
        _ => Ok(()),
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for PutFunctionPolicyUseCase<U> {
    type Command = PutPolicyCommand;
    type Event = PolicyUpdated;

    async fn validate(&self, command: &PutPolicyCommand) -> Result<(), UseCaseError> {
        let mut seen: HashSet<(&str, &str)> = HashSet::new();
        for signer in &command.signers {
            let (Some(issuer), Some(subject)) = (
                signer.issuer.as_deref().filter(|s| !java_is_blank(s)),
                signer.subject.as_deref().filter(|s| !java_is_blank(s)),
            ) else {
                return Err(UseCaseError::validation(
                    "SIGNER_INVALID",
                    "issuer and subject are required",
                ));
            };
            if signer.runtimes.is_empty() {
                return Err(UseCaseError::validation(
                    "RUNTIME_INVALID",
                    "at least one runtime is required",
                ));
            }
            for runtime in &signer.runtimes {
                Runtime::parse_strict(runtime)?;
            }
            if !seen.insert((issuer, subject)) {
                return Err(UseCaseError::validation(
                    "SIGNER_DUPLICATE",
                    format!("duplicate signer: issuer '{issuer}', subject '{subject}'"),
                ));
            }
        }
        require_valid_ceiling(command.max_duration_ms, "maxDurationMs")?;
        require_valid_ceiling(command.max_concurrency, "maxConcurrency")?;
        require_valid_ceiling(command.max_wasm_memory_mb, "maxWasmMemoryMb")?;
        require_valid_ceiling(command.max_db_pool_size, "maxDbPoolSize")?;
        Ok(())
    }

    /// Anchor scope and the permission are the handler's.
    async fn authorize(
        &self,
        _command: &PutPolicyCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: PutPolicyCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<PolicyUpdated> {
        let policy = match self.prepare(&command).await {
            Ok(p) => p,
            Err(e) => return UseCaseResult::failure(e),
        };
        let event = PolicyUpdated::new(&ctx, &policy);
        self.unit_of_work
            .commit(&policy, &*self.policies, event, &command)
            .await
    }
}

impl<U: UnitOfWork> PutFunctionPolicyUseCase<U> {
    async fn prepare(&self, command: &PutPolicyCommand) -> Result<ClientPolicy, UseCaseError> {
        if let FunctionOwner::Client(client_id) = &command.owner {
            if self.clients.find_by_id(client_id).await?.is_none() {
                return Err(resource_not_found("Client", client_id));
            }
        }
        let now = Utc::now();
        let created_at = self
            .policies
            .find_by_owner(&command.owner)
            .await?
            .map(|p| p.created_at)
            .unwrap_or(now);
        let mut signers = Vec::with_capacity(command.signers.len());
        for s in &command.signers {
            let runtimes = s
                .runtimes
                .iter()
                .map(|r| Runtime::parse_strict(r))
                .collect::<Result<Vec<_>, _>>()?;
            signers.push(SignerRule::new(
                s.issuer.clone().unwrap_or_default(),
                s.subject.clone().unwrap_or_default(),
                runtimes,
            ));
        }
        Ok(ClientPolicy {
            owner: command.owner.clone(),
            signers,
            max_duration_ms: command.max_duration_ms,
            max_concurrency: command.max_concurrency,
            max_wasm_memory_mb: command.max_wasm_memory_mb,
            max_db_pool_size: command.max_db_pool_size,
            created_at,
            updated_at: now,
        })
    }
}
