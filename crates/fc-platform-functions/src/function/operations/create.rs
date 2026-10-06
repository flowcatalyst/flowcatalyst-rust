//! Java `function/operations/CreateFunction.java`: a function under an
//! application, owned by a client or the platform.
//!
//! Reach is split as in Java: `authorize` checks the command's own
//! `clientId` (a 403 is fine here, nothing exists yet to hide), and
//! `execute` checks the application once it is loaded.

use crate::function::operations::access::FunctionReach;
use fc_platform_core::shared::id::ClientId;
use fc_platform_core::shared::id::OptionIdExt;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;

use super::access::resource_not_found;
use super::events::FunctionCreated;
use crate::function::entity::Function;
use crate::function::repository::FunctionRepository;
use crate::function::{java_is_blank, DnsLabel, FunctionAddress, FunctionOwner, Runtime};
use fc_platform_core::directory::{ApplicationDirectory, ClientDirectory};
use fc_platform_core::usecase::{
    AuditMasked, Committed, ExecutionContext, UnitOfWork, UseCase, UseCaseError,
};

/// `POST /api/functions`. An absent or blank `clientId` means a
/// platform-owned function.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCommand {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<ClientId>,
}

impl AuditMasked for CreateCommand {}

impl CreateCommand {
    fn client_id(&self) -> Option<&str> {
        blank_to_none(self.client_id.as_id_str())
    }
}

pub(crate) fn blank_to_none(s: Option<&str>) -> Option<&str> {
    s.filter(|s| !java_is_blank(s))
}

/// Java `ApplicationCode.FORMAT_MESSAGE`.
const APPLICATION_CODE_FORMAT_MESSAGE: &str = "code must start with a lowercase letter and \
     contain only lowercase alphanumerics, hyphens, and underscores";

/// Java `ApplicationCode.parse`: trimmed and lower-cased, then
/// `^[a-z][a-z0-9_-]*$`.
fn parse_application_code(raw: &str) -> Result<String, UseCaseError> {
    let code = raw.trim_matches(|c: char| c <= ' ').to_lowercase();
    if java_is_blank(&code) {
        return Err(UseCaseError::validation(
            "CODE_REQUIRED",
            "code is required",
        ));
    }
    let mut chars = code.chars();
    let valid = chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
    if !valid {
        return Err(UseCaseError::validation(
            "INVALID_CODE_FORMAT",
            APPLICATION_CODE_FORMAT_MESSAGE,
        ));
    }
    Ok(code)
}

pub struct CreateFunctionUseCase<U: UnitOfWork> {
    pub(crate) functions: Arc<FunctionRepository>,
    pub(crate) applications: Arc<dyn ApplicationDirectory>,
    pub(crate) clients: Arc<dyn ClientDirectory>,
    pub(crate) unit_of_work: Arc<U>,
}

#[async_trait]
impl<U: UnitOfWork> UseCase for CreateFunctionUseCase<U> {
    type Command = CreateCommand;
    type Event = FunctionCreated;

    async fn validate(&self, command: &CreateCommand) -> Result<(), UseCaseError> {
        if command
            .application_code
            .as_deref()
            .is_none_or(java_is_blank)
        {
            return Err(UseCaseError::validation(
                "APPLICATION_CODE_REQUIRED",
                "applicationCode is required",
            ));
        }
        DnsLabel::parse("serviceName", command.service_name.as_deref().unwrap_or(""))?;
        DnsLabel::parse("name", command.name.as_deref().unwrap_or(""))?;
        Runtime::parse_strict(command.runtime.as_deref().unwrap_or(""))?;
        Ok(())
    }

    async fn authorize(
        &self,
        command: &CreateCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        ctx.caller()
            .check_scope_access(command.client_id().map(ClientId::from_wire).as_ref())
    }

    async fn execute(
        &self,
        command: CreateCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<FunctionCreated>, UseCaseError> {
        let code = parse_application_code(command.application_code.as_deref().unwrap_or(""))?;
        let application = self
            .applications
            .find_by_code(&code)
            .await?
            .ok_or_else(|| resource_not_found("Application", &code))?;

        // R1: the application's stored code must itself be a DNS label.
        let application_label =
            DnsLabel::parse("applicationCode", &application.code).map_err(|_| {
                UseCaseError::validation(
                    "APPLICATION_CODE_NOT_ADDRESSABLE",
                    format!(
                        "application code '{}' cannot be part of a function address: it must be a \
                     DNS label (a-z, 0-9, '-'); codes with '_' or longer than 63 characters \
                     cannot own functions",
                        application.code
                    ),
                )
            })?;

        let owner = match command.client_id() {
            Some(client_id) => {
                let client = self
                    .clients
                    .find_by_id(&ClientId::from_wire(client_id))
                    .await?
                    .ok_or_else(|| resource_not_found("Client", client_id))?;
                FunctionOwner::Client(client.id.into_string())
            }
            None => FunctionOwner::Platform,
        };

        ctx.caller()
            .check_application_access(&application.id, &application.code)?;

        let service =
            DnsLabel::parse("serviceName", command.service_name.as_deref().unwrap_or(""))?;
        let name = DnsLabel::parse("name", command.name.as_deref().unwrap_or(""))?;
        let address =
            FunctionAddress::new(application_label.value(), service.value(), name.value())
                .map_err(|e| UseCaseError::validation("ADDRESS_INVALID", e.to_string()))?;
        if self.functions.find_by_address(&address).await?.is_some() {
            return Err(UseCaseError::business_rule(
                "FUNCTION_EXISTS",
                format!("function '{}' already exists", address.render()),
            ));
        }

        let runtime = Runtime::parse_strict(command.runtime.as_deref().unwrap_or(""))?;
        let function = Function::create(
            application.id,
            address,
            owner,
            runtime,
            command.description.clone(),
        );
        let event = FunctionCreated::new(&ctx, &function);
        self.unit_of_work
            .commit(&function, &*self.functions, event, &command)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn application_codes_parse_as_java_does() {
        assert_eq!(
            parse_application_code("  Logistics_Portal ").unwrap(),
            "logistics_portal"
        );
        assert_eq!(parse_application_code("billing").unwrap(), "billing");
        assert_eq!(
            parse_application_code(" ").unwrap_err().code(),
            "CODE_REQUIRED"
        );
        let err = parse_application_code("9lives").unwrap_err();
        assert_eq!(err.code(), "INVALID_CODE_FORMAT");
        assert_eq!(err.message(), APPLICATION_CODE_FORMAT_MESSAGE);
    }

    #[test]
    fn blank_client_id_means_the_platform() {
        let cmd = CreateCommand {
            client_id: Some(ClientId::from_wire("  ")),
            ..Default::default()
        };
        assert_eq!(cmd.client_id(), None);
    }
}
