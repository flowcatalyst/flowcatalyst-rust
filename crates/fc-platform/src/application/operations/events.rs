//! Application Domain Events

use serde::{Deserialize, Serialize};
use crate::usecase::ExecutionContext;
use crate::usecase::domain_event::EventMetadata;
use crate::TsidGenerator;
use crate::impl_domain_event;

/// Event emitted when a new application is created.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationCreated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub application_id: String,
    pub code: String,
    pub name: String,
    pub application_type: String,
}

impl_domain_event!(ApplicationCreated);

impl ApplicationCreated {
    const EVENT_TYPE: &'static str = "platform:iam:application:created";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:application";

    pub fn new(
        ctx: &ExecutionContext,
        application_id: &str,
        code: &str,
        name: &str,
        application_type: &str,
    ) -> Self {
        let event_id = TsidGenerator::generate();
        let subject = format!("platform.application.{}", application_id);
        let message_group = format!("platform:application:{}", application_id);

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
            application_id: application_id.to_string(),
            code: code.to_string(),
            name: name.to_string(),
            application_type: application_type.to_string(),
        }
    }
}

/// Event emitted when an application is updated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationUpdated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub application_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl_domain_event!(ApplicationUpdated);

impl ApplicationUpdated {
    const EVENT_TYPE: &'static str = "platform:iam:application:updated";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:application";

    pub fn new(
        ctx: &ExecutionContext,
        application_id: &str,
        name: Option<&str>,
        description: Option<&str>,
    ) -> Self {
        let event_id = TsidGenerator::generate();
        let subject = format!("platform.application.{}", application_id);
        let message_group = format!("platform:application:{}", application_id);

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
            application_id: application_id.to_string(),
            name: name.map(String::from),
            description: description.map(String::from),
        }
    }
}

/// Event emitted when an application is activated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationActivated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub application_id: String,
    pub code: String,
}

impl_domain_event!(ApplicationActivated);

impl ApplicationActivated {
    const EVENT_TYPE: &'static str = "platform:iam:application:activated";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:application";

    pub fn new(ctx: &ExecutionContext, application_id: &str, code: &str) -> Self {
        let event_id = TsidGenerator::generate();
        let subject = format!("platform.application.{}", application_id);
        let message_group = format!("platform:application:{}", application_id);

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
            application_id: application_id.to_string(),
            code: code.to_string(),
        }
    }
}

/// Event emitted when an application is deactivated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationDeactivated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub application_id: String,
    pub code: String,
}

impl_domain_event!(ApplicationDeactivated);

impl ApplicationDeactivated {
    const EVENT_TYPE: &'static str = "platform:iam:application:deactivated";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:application";

    pub fn new(ctx: &ExecutionContext, application_id: &str, code: &str) -> Self {
        let event_id = TsidGenerator::generate();
        let subject = format!("platform.application.{}", application_id);
        let message_group = format!("platform:application:{}", application_id);

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
            application_id: application_id.to_string(),
            code: code.to_string(),
        }
    }
}

/// Event emitted when a service account is provisioned for an application.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationServiceAccountProvisioned {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub application_id: String,
    pub application_code: String,
    pub service_account_id: String,
    pub service_account_code: String,
}

impl_domain_event!(ApplicationServiceAccountProvisioned);

impl ApplicationServiceAccountProvisioned {
    const EVENT_TYPE: &'static str = "platform:iam:application:service-account-provisioned";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:application";

    pub fn new(
        ctx: &ExecutionContext,
        application_id: &str,
        application_code: &str,
        service_account_id: &str,
        service_account_code: &str,
    ) -> Self {
        let event_id = TsidGenerator::generate();
        let subject = format!("platform.application.{}", application_id);
        let message_group = format!("platform:application:{}", application_id);

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
            application_id: application_id.to_string(),
            application_code: application_code.to_string(),
            service_account_id: service_account_id.to_string(),
            service_account_code: service_account_code.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usecase::DomainEvent;

    #[test]
    fn test_application_created_event() {
        let ctx = ExecutionContext::create("admin-123");
        let event = ApplicationCreated::new(
            &ctx,
            "app-1",
            "orders",
            "Orders Application",
            "APPLICATION",
        );

        assert_eq!(event.event_type(), "platform:iam:application:created");
        assert_eq!(event.application_id, "app-1");
        assert_eq!(event.code, "orders");
    }

    #[test]
    fn test_application_service_account_provisioned_event() {
        let ctx = ExecutionContext::create("admin-123");
        let event = ApplicationServiceAccountProvisioned::new(
            &ctx,
            "app-1",
            "orders",
            "sa-1",
            "app:orders",
        );

        assert_eq!(event.event_type(), "platform:iam:application:service-account-provisioned");
        assert_eq!(event.service_account_id, "sa-1");
    }
}
