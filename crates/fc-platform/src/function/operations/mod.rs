//! The function registry's write operations (Java `function/operations/`).
//! Every one is a [`UseCase`](crate::usecase::UseCase) that ends in a
//! unit-of-work commit, so each write leaves one `platform:function:*`
//! event and one audit row.
//!
//! A use case that must reach an existing resource carries the [`Caller`]:
//! Java reads the same context from its request-scoped `Auth.current()`.
//! [`FunctionOperations`] builds them per request.

pub mod access;
pub mod create;
pub mod delete;
pub mod domains;
pub mod events;
pub mod publish;
pub mod publish_checks;
pub mod put_policy;
pub mod retire;
pub mod settings;
pub mod trigger_sync;
pub mod update;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use serde::Serializer;

pub use access::Caller;
pub use create::{CreateCommand, CreateFunctionUseCase};
pub use delete::{DeleteCommand, DeleteFunctionUseCase};
pub use domains::{
    ClaimCommand, ClaimFunctionDomainUseCase, ReleaseCommand, ReleaseFunctionDomainUseCase,
};
pub use publish::{PublishCommand, PublishVersionUseCase};
pub use publish_checks::PublishChecks;
pub use put_policy::{PutFunctionPolicyUseCase, PutPolicyCommand, SignerInput};
pub use retire::{RetireCommand, RetireVersionUseCase};
pub use settings::{
    DeleteFunctionSecretUseCase, DeleteSecretCommand, SetConfigCommand, SetFunctionConfigUseCase,
    SetFunctionSecretUseCase, SetSecretCommand,
};
pub use trigger_sync::TriggerSync;
pub use update::{UpdateCommand, UpdateFunctionUseCase};

use super::artifact::ArtifactBlobStore;
use super::domain_repository::FunctionDomainRepository;
use super::policy_repository::ClientPolicyRepository;
use super::repository::FunctionRepository;
use super::route_repository::FunctionRouteRepository;
use super::settings_repository::FunctionSettingsRepository;
use super::version_repository::FunctionVersionRepository;
use super::{FunctionAddress, FunctionLimits};
use crate::usecase::UnitOfWork;
use crate::{ApplicationRepository, ClientRepository};
use fc_function_signing::Signatures;

/// A command's address, as its rendered string.
pub(crate) fn serialize_address<S: Serializer>(
    address: &FunctionAddress,
    s: S,
) -> Result<S::Ok, S::Error> {
    s.serialize_str(&address.render())
}

/// What the function use cases need, to build one per request.
pub struct FunctionOperations<U: UnitOfWork> {
    pub functions: Arc<FunctionRepository>,
    pub versions: Arc<FunctionVersionRepository>,
    pub applications: Arc<ApplicationRepository>,
    pub clients: Arc<ClientRepository>,
    pub settings: Arc<FunctionSettingsRepository>,
    pub policies: Arc<ClientPolicyRepository>,
    pub domains: Arc<FunctionDomainRepository>,
    pub routes: Arc<FunctionRouteRepository>,
    pub trigger_sync: TriggerSync,
    /// The platform defaults a policy's absent ceilings resolve to.
    pub limits: FunctionLimits,
    /// `FC_FN_SIGNATURES`, resolved once at startup.
    pub signatures: Signatures,
    /// `FC_FN_ARTIFACT_STORE`; `None` when unset.
    pub artifacts: Option<Arc<dyn ArtifactBlobStore>>,
    pub publish_checks: PublishChecks,
    pub unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> Clone for FunctionOperations<U> {
    fn clone(&self) -> Self {
        Self {
            functions: self.functions.clone(),
            versions: self.versions.clone(),
            applications: self.applications.clone(),
            clients: self.clients.clone(),
            settings: self.settings.clone(),
            policies: self.policies.clone(),
            domains: self.domains.clone(),
            routes: self.routes.clone(),
            trigger_sync: self.trigger_sync,
            limits: self.limits,
            signatures: self.signatures.clone(),
            artifacts: self.artifacts.clone(),
            publish_checks: self.publish_checks.clone(),
            unit_of_work: self.unit_of_work.clone(),
        }
    }
}

impl<U: UnitOfWork> FunctionOperations<U> {
    pub fn create(&self, caller: Caller) -> CreateFunctionUseCase<U> {
        CreateFunctionUseCase {
            functions: self.functions.clone(),
            applications: self.applications.clone(),
            clients: self.clients.clone(),
            unit_of_work: self.unit_of_work.clone(),
            caller,
        }
    }

    pub fn update(&self, caller: Caller) -> UpdateFunctionUseCase<U> {
        UpdateFunctionUseCase {
            functions: self.functions.clone(),
            trigger_sync: self.trigger_sync,
            unit_of_work: self.unit_of_work.clone(),
            caller,
        }
    }

    pub fn delete(&self, caller: Caller) -> DeleteFunctionUseCase<U> {
        DeleteFunctionUseCase {
            functions: self.functions.clone(),
            trigger_sync: self.trigger_sync,
            unit_of_work: self.unit_of_work.clone(),
            caller,
        }
    }

    pub fn publish(&self, caller: Caller) -> PublishVersionUseCase<U> {
        self.publish_in(caller, self.unit_of_work.clone())
    }

    /// Publish on a given unit of work: a transaction-scoped one
    /// (`PgUnitOfWork::run`), since publish reserves its version number
    /// under a row lock in the transaction it commits in.
    pub fn publish_in<V: UnitOfWork>(
        &self,
        caller: Caller,
        unit_of_work: Arc<V>,
    ) -> PublishVersionUseCase<V> {
        PublishVersionUseCase {
            functions: self.functions.clone(),
            versions: self.versions.clone(),
            policies: self.policies.clone(),
            limits: self.limits,
            signatures: self.signatures.clone(),
            artifacts: self.artifacts.clone(),
            checks: self.publish_checks.clone(),
            unit_of_work,
            caller,
        }
    }

    pub fn retire(&self, caller: Caller) -> RetireVersionUseCase<U> {
        RetireVersionUseCase {
            functions: self.functions.clone(),
            versions: self.versions.clone(),
            unit_of_work: self.unit_of_work.clone(),
            caller,
        }
    }

    pub fn set_config(&self, caller: Caller) -> SetFunctionConfigUseCase<U> {
        SetFunctionConfigUseCase {
            functions: self.functions.clone(),
            settings: self.settings.clone(),
            unit_of_work: self.unit_of_work.clone(),
            caller,
        }
    }

    pub fn set_secret(&self, caller: Caller) -> SetFunctionSecretUseCase<U> {
        SetFunctionSecretUseCase {
            functions: self.functions.clone(),
            settings: self.settings.clone(),
            unit_of_work: self.unit_of_work.clone(),
            caller,
        }
    }

    pub fn delete_secret(&self, caller: Caller) -> DeleteFunctionSecretUseCase<U> {
        DeleteFunctionSecretUseCase {
            functions: self.functions.clone(),
            settings: self.settings.clone(),
            unit_of_work: self.unit_of_work.clone(),
            caller,
        }
    }

    /// No caller: the handler's anchor-and-permission gate is the whole
    /// authorization, as in Java.
    pub fn put_policy(&self) -> PutFunctionPolicyUseCase<U> {
        PutFunctionPolicyUseCase {
            policies: self.policies.clone(),
            clients: self.clients.clone(),
            unit_of_work: self.unit_of_work.clone(),
        }
    }

    pub fn claim_domain(&self, caller: Caller) -> ClaimFunctionDomainUseCase<U> {
        ClaimFunctionDomainUseCase {
            domains: self.domains.clone(),
            unit_of_work: self.unit_of_work.clone(),
            caller,
        }
    }

    pub fn release_domain(&self, caller: Caller) -> ReleaseFunctionDomainUseCase<U> {
        ReleaseFunctionDomainUseCase {
            domains: self.domains.clone(),
            routes: self.routes.clone(),
            functions: self.functions.clone(),
            unit_of_work: self.unit_of_work.clone(),
            caller,
        }
    }
}
