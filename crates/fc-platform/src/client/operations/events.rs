//! Client Domain Events

use serde::{Deserialize, Serialize};
use crate::usecase::ExecutionContext;
use crate::usecase::domain_event::EventMetadata;
use crate::TsidGenerator;
use crate::impl_domain_event;
use crate::client::entity::ClientStatus;

/// Event emitted when a new client is created.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientCreated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub client_id: String,
    pub name: String,
    pub identifier: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl_domain_event!(ClientCreated);

impl ClientCreated {
    const EVENT_TYPE: &'static str = "platform:iam:client:created";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(
        ctx: &ExecutionContext,
        client_id: &str,
        name: &str,
        identifier: &str,
        description: Option<&str>,
    ) -> Self {
        let event_id = TsidGenerator::generate();
        let subject = format!("platform.client.{}", client_id);
        let message_group = format!("platform:client:{}", client_id);

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
            client_id: client_id.to_string(),
            name: name.to_string(),
            identifier: identifier.to_string(),
            description: description.map(String::from),
        }
    }
}

/// Event emitted when a client is updated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientUpdated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub client_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl_domain_event!(ClientUpdated);

impl ClientUpdated {
    const EVENT_TYPE: &'static str = "platform:iam:client:updated";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(
        ctx: &ExecutionContext,
        client_id: &str,
        name: Option<&str>,
        description: Option<&str>,
    ) -> Self {
        let event_id = TsidGenerator::generate();
        let subject = format!("platform.client.{}", client_id);
        let message_group = format!("platform:client:{}", client_id);

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
            client_id: client_id.to_string(),
            name: name.map(String::from),
            description: description.map(String::from),
        }
    }
}

/// Event emitted when a client is activated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientActivated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub client_id: String,
    pub previous_status: String,
}

impl_domain_event!(ClientActivated);

impl ClientActivated {
    const EVENT_TYPE: &'static str = "platform:iam:client:activated";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(ctx: &ExecutionContext, client_id: &str, previous_status: ClientStatus) -> Self {
        let event_id = TsidGenerator::generate();
        let subject = format!("platform.client.{}", client_id);
        let message_group = format!("platform:client:{}", client_id);

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
            client_id: client_id.to_string(),
            previous_status: format!("{:?}", previous_status).to_uppercase(),
        }
    }
}

/// Event emitted when a client is suspended.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientSuspended {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub client_id: String,
    pub reason: String,
}

impl_domain_event!(ClientSuspended);

impl ClientSuspended {
    const EVENT_TYPE: &'static str = "platform:iam:client:suspended";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:iam";

    pub fn new(ctx: &ExecutionContext, client_id: &str, reason: &str) -> Self {
        let event_id = TsidGenerator::generate();
        let subject = format!("platform.client.{}", client_id);
        let message_group = format!("platform:client:{}", client_id);

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
            client_id: client_id.to_string(),
            reason: reason.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usecase::DomainEvent;

    #[test]
    fn test_client_created_event() {
        let ctx = ExecutionContext::create("user-123");
        let event = ClientCreated::new(&ctx, "client-1", "Acme Corp", "acme-corp", None);

        assert_eq!(event.event_type(), "platform:iam:client:created");
        assert_eq!(event.client_id, "client-1");
        assert_eq!(event.name, "Acme Corp");
        assert_eq!(event.identifier, "acme-corp");
    }

    #[test]
    fn test_client_suspended_event() {
        let ctx = ExecutionContext::create("user-123");
        let event = ClientSuspended::new(&ctx, "client-1", "Payment overdue");

        assert_eq!(event.event_type(), "platform:iam:client:suspended");
        assert_eq!(event.reason, "Payment overdue");
    }
}
