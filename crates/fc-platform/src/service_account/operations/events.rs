//! Service Account Domain Events

use serde::{Deserialize, Serialize};
use crate::usecase::ExecutionContext;
use crate::usecase::domain_event::EventMetadata;
use crate::TsidGenerator;
use crate::impl_domain_event;

/// Event emitted when a new service account is created.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccountCreated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub service_account_id: String,
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_id: Option<String>,
    pub client_ids: Vec<String>,
}

impl_domain_event!(ServiceAccountCreated);

impl ServiceAccountCreated {
    const EVENT_TYPE: &'static str = "platform:iam:serviceaccount:created";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:serviceaccount";

    pub fn new(
        ctx: &ExecutionContext,
        service_account_id: &str,
        code: &str,
        name: &str,
        application_id: Option<&str>,
        client_ids: Vec<String>,
    ) -> Self {
        let event_id = TsidGenerator::generate();
        let subject = format!("platform.serviceaccount.{}", service_account_id);
        let message_group = format!("platform:serviceaccount:{}", service_account_id);

        Self {
            metadata: EventMetadata::new(
                event_id,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                subject,
                message_group,
                ctx.execution_id.clone(),
                ctx.correlation_id.clone(),
                ctx.causation_id.clone(),
                ctx.principal_id.clone(),
            ),
            service_account_id: service_account_id.to_string(),
            code: code.to_string(),
            name: name.to_string(),
            application_id: application_id.map(String::from),
            client_ids,
        }
    }
}

/// Event emitted when a service account is updated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccountUpdated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub service_account_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub client_ids_added: Vec<String>,
    pub client_ids_removed: Vec<String>,
}

impl_domain_event!(ServiceAccountUpdated);

impl ServiceAccountUpdated {
    const EVENT_TYPE: &'static str = "platform:iam:serviceaccount:updated";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:serviceaccount";

    pub fn new(
        ctx: &ExecutionContext,
        service_account_id: &str,
        name: Option<&str>,
        description: Option<&str>,
        client_ids_added: Vec<String>,
        client_ids_removed: Vec<String>,
    ) -> Self {
        let event_id = TsidGenerator::generate();
        let subject = format!("platform.serviceaccount.{}", service_account_id);
        let message_group = format!("platform:serviceaccount:{}", service_account_id);

        Self {
            metadata: EventMetadata::new(
                event_id,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                subject,
                message_group,
                ctx.execution_id.clone(),
                ctx.correlation_id.clone(),
                ctx.causation_id.clone(),
                ctx.principal_id.clone(),
            ),
            service_account_id: service_account_id.to_string(),
            name: name.map(String::from),
            description: description.map(String::from),
            client_ids_added,
            client_ids_removed,
        }
    }
}

/// Event emitted when a service account is deleted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccountDeleted {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub service_account_id: String,
    pub code: String,
}

impl_domain_event!(ServiceAccountDeleted);

impl ServiceAccountDeleted {
    const EVENT_TYPE: &'static str = "platform:iam:serviceaccount:deleted";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:serviceaccount";

    pub fn new(ctx: &ExecutionContext, service_account_id: &str, code: &str) -> Self {
        let event_id = TsidGenerator::generate();
        let subject = format!("platform.serviceaccount.{}", service_account_id);
        let message_group = format!("platform:serviceaccount:{}", service_account_id);

        Self {
            metadata: EventMetadata::new(
                event_id,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                subject,
                message_group,
                ctx.execution_id.clone(),
                ctx.correlation_id.clone(),
                ctx.causation_id.clone(),
                ctx.principal_id.clone(),
            ),
            service_account_id: service_account_id.to_string(),
            code: code.to_string(),
        }
    }
}

/// Event emitted when roles are assigned to a service account.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccountRolesAssigned {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub service_account_id: String,
    pub roles_added: Vec<String>,
    pub roles_removed: Vec<String>,
}

impl_domain_event!(ServiceAccountRolesAssigned);

impl ServiceAccountRolesAssigned {
    const EVENT_TYPE: &'static str = "platform:iam:serviceaccount:roles-assigned";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:serviceaccount";

    pub fn new(
        ctx: &ExecutionContext,
        service_account_id: &str,
        roles_added: Vec<String>,
        roles_removed: Vec<String>,
    ) -> Self {
        let event_id = TsidGenerator::generate();
        let subject = format!("platform.serviceaccount.{}", service_account_id);
        let message_group = format!("platform:serviceaccount:{}", service_account_id);

        Self {
            metadata: EventMetadata::new(
                event_id,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                subject,
                message_group,
                ctx.execution_id.clone(),
                ctx.correlation_id.clone(),
                ctx.causation_id.clone(),
                ctx.principal_id.clone(),
            ),
            service_account_id: service_account_id.to_string(),
            roles_added,
            roles_removed,
        }
    }
}

/// Event emitted when a service account's auth token is regenerated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccountTokenRegenerated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub service_account_id: String,
    pub code: String,
}

impl_domain_event!(ServiceAccountTokenRegenerated);

impl ServiceAccountTokenRegenerated {
    const EVENT_TYPE: &'static str = "platform:iam:serviceaccount:token-regenerated";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:serviceaccount";

    pub fn new(ctx: &ExecutionContext, service_account_id: &str, code: &str) -> Self {
        let event_id = TsidGenerator::generate();
        let subject = format!("platform.serviceaccount.{}", service_account_id);
        let message_group = format!("platform:serviceaccount:{}", service_account_id);

        Self {
            metadata: EventMetadata::new(
                event_id,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                subject,
                message_group,
                ctx.execution_id.clone(),
                ctx.correlation_id.clone(),
                ctx.causation_id.clone(),
                ctx.principal_id.clone(),
            ),
            service_account_id: service_account_id.to_string(),
            code: code.to_string(),
        }
    }
}

/// Event emitted when a service account's signing secret is regenerated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccountSecretRegenerated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub service_account_id: String,
    pub code: String,
}

impl_domain_event!(ServiceAccountSecretRegenerated);

impl ServiceAccountSecretRegenerated {
    const EVENT_TYPE: &'static str = "platform:iam:serviceaccount:secret-regenerated";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:serviceaccount";

    pub fn new(ctx: &ExecutionContext, service_account_id: &str, code: &str) -> Self {
        let event_id = TsidGenerator::generate();
        let subject = format!("platform.serviceaccount.{}", service_account_id);
        let message_group = format!("platform:serviceaccount:{}", service_account_id);

        Self {
            metadata: EventMetadata::new(
                event_id,
                Self::EVENT_TYPE,
                Self::SPEC_VERSION,
                Self::SOURCE,
                subject,
                message_group,
                ctx.execution_id.clone(),
                ctx.correlation_id.clone(),
                ctx.causation_id.clone(),
                ctx.principal_id.clone(),
            ),
            service_account_id: service_account_id.to_string(),
            code: code.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usecase::DomainEvent;

    #[test]
    fn test_service_account_created_event() {
        let ctx = ExecutionContext::create("admin-123");
        let event = ServiceAccountCreated::new(
            &ctx,
            "sa-1",
            "my-service",
            "My Service",
            None,
            vec!["client-1".to_string()],
        );

        assert_eq!(event.event_type(), "platform:iam:serviceaccount:created");
        assert_eq!(event.service_account_id, "sa-1");
        assert_eq!(event.code, "my-service");
    }

    #[test]
    fn test_service_account_deleted_event() {
        let ctx = ExecutionContext::create("admin-123");
        let event = ServiceAccountDeleted::new(&ctx, "sa-1", "my-service");

        assert_eq!(event.event_type(), "platform:iam:serviceaccount:deleted");
        assert_eq!(event.code, "my-service");
    }

    #[test]
    fn test_service_account_roles_assigned_event() {
        let ctx = ExecutionContext::create("admin-123");
        let event = ServiceAccountRolesAssigned::new(
            &ctx,
            "sa-1",
            vec!["ADMIN".to_string()],
            vec!["VIEWER".to_string()],
        );

        assert_eq!(event.event_type(), "platform:iam:serviceaccount:roles-assigned");
        assert_eq!(event.roles_added, vec!["ADMIN".to_string()]);
        assert_eq!(event.roles_removed, vec!["VIEWER".to_string()]);
    }
}
