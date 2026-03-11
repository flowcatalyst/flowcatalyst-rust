//! Connection Domain Events

use serde::{Deserialize, Serialize};
use crate::usecase::ExecutionContext;
use crate::usecase::domain_event::EventMetadata;
use crate::TsidGenerator;
use crate::EntityType;
use crate::impl_domain_event;

/// Event emitted when a new connection is created.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionCreated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub connection_id: String,
    pub code: String,
    pub name: String,
    pub endpoint: String,
    pub service_account_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

impl_domain_event!(ConnectionCreated);

impl ConnectionCreated {
    const EVENT_TYPE: &'static str = "platform:control-plane:connection:created";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:control-plane";

    pub fn new(
        ctx: &ExecutionContext,
        connection_id: &str,
        code: &str,
        name: &str,
        endpoint: &str,
        service_account_id: &str,
        client_id: Option<&str>,
    ) -> Self {
        let event_id = TsidGenerator::generate(EntityType::Event);
        let subject = format!("platform.connection.{}", connection_id);
        let message_group = format!("platform:connection:{}", connection_id);

        Self {
            metadata: EventMetadata::new(
                event_id, Self::EVENT_TYPE, Self::SPEC_VERSION, Self::SOURCE,
                subject, message_group,
                ctx.execution_id.clone(), ctx.correlation_id.clone(),
                ctx.causation_id.clone(), ctx.principal_id.clone(),
            ),
            connection_id: connection_id.to_string(),
            code: code.to_string(),
            name: name.to_string(),
            endpoint: endpoint.to_string(),
            service_account_id: service_account_id.to_string(),
            client_id: client_id.map(String::from),
        }
    }
}

/// Event emitted when a connection is updated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionUpdated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub connection_id: String,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

impl_domain_event!(ConnectionUpdated);

impl ConnectionUpdated {
    const EVENT_TYPE: &'static str = "platform:control-plane:connection:updated";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:control-plane";

    pub fn new(
        ctx: &ExecutionContext,
        connection_id: &str,
        code: &str,
        name: Option<&str>,
        endpoint: Option<&str>,
        status: Option<&str>,
    ) -> Self {
        let event_id = TsidGenerator::generate(EntityType::Event);
        let subject = format!("platform.connection.{}", connection_id);
        let message_group = format!("platform:connection:{}", connection_id);

        Self {
            metadata: EventMetadata::new(
                event_id, Self::EVENT_TYPE, Self::SPEC_VERSION, Self::SOURCE,
                subject, message_group,
                ctx.execution_id.clone(), ctx.correlation_id.clone(),
                ctx.causation_id.clone(), ctx.principal_id.clone(),
            ),
            connection_id: connection_id.to_string(),
            code: code.to_string(),
            name: name.map(String::from),
            endpoint: endpoint.map(String::from),
            status: status.map(String::from),
        }
    }
}

/// Event emitted when a connection is deleted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionDeleted {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub connection_id: String,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

impl_domain_event!(ConnectionDeleted);

impl ConnectionDeleted {
    const EVENT_TYPE: &'static str = "platform:control-plane:connection:deleted";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:control-plane";

    pub fn new(ctx: &ExecutionContext, connection_id: &str, code: &str, client_id: Option<&str>) -> Self {
        let event_id = TsidGenerator::generate(EntityType::Event);
        let subject = format!("platform.connection.{}", connection_id);
        let message_group = format!("platform:connection:{}", connection_id);

        Self {
            metadata: EventMetadata::new(
                event_id, Self::EVENT_TYPE, Self::SPEC_VERSION, Self::SOURCE,
                subject, message_group,
                ctx.execution_id.clone(), ctx.correlation_id.clone(),
                ctx.causation_id.clone(), ctx.principal_id.clone(),
            ),
            connection_id: connection_id.to_string(),
            code: code.to_string(),
            client_id: client_id.map(String::from),
        }
    }
}
