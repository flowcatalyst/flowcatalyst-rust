//! Event Type Domain Events

use serde::{Deserialize, Serialize};
use crate::usecase::ExecutionContext;
use crate::usecase::domain_event::EventMetadata;
use crate::TsidGenerator;
use crate::impl_domain_event;

/// Event emitted when a new event type is created.
///
/// Event type: `platform:control-plane:eventtype:created`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventTypeCreated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    // Event-specific data
    pub event_type_id: String,
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub application: String,
    pub subdomain: String,
    pub aggregate: String,
    pub event_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

impl_domain_event!(EventTypeCreated);

impl EventTypeCreated {
    const EVENT_TYPE: &'static str = "platform:control-plane:eventtype:created";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:control-plane";

    /// Create a new EventTypeCreated event from an ExecutionContext.
    pub fn new(
        ctx: &ExecutionContext,
        event_type_id: &str,
        code: &str,
        name: &str,
        description: Option<&str>,
        application: &str,
        subdomain: &str,
        aggregate: &str,
        event_name: &str,
        client_id: Option<&str>,
    ) -> Self {
        let event_id = TsidGenerator::generate();
        let subject = format!("platform.eventtype.{}", event_type_id);
        let message_group = format!("platform:eventtype:{}", event_type_id);

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
            event_type_id: event_type_id.to_string(),
            code: code.to_string(),
            name: name.to_string(),
            description: description.map(String::from),
            application: application.to_string(),
            subdomain: subdomain.to_string(),
            aggregate: aggregate.to_string(),
            event_name: event_name.to_string(),
            client_id: client_id.map(String::from),
        }
    }

    /// Builder for EventTypeCreated
    pub fn builder() -> EventTypeCreatedBuilder {
        EventTypeCreatedBuilder::new()
    }
}

/// Builder for EventTypeCreated
pub struct EventTypeCreatedBuilder {
    event_id: Option<String>,
    execution_id: Option<String>,
    correlation_id: Option<String>,
    causation_id: Option<String>,
    principal_id: Option<String>,
    event_type_id: Option<String>,
    code: Option<String>,
    name: Option<String>,
    description: Option<String>,
    application: Option<String>,
    subdomain: Option<String>,
    aggregate: Option<String>,
    event_name: Option<String>,
    client_id: Option<String>,
}

impl EventTypeCreatedBuilder {
    pub fn new() -> Self {
        Self {
            event_id: None,
            execution_id: None,
            correlation_id: None,
            causation_id: None,
            principal_id: None,
            event_type_id: None,
            code: None,
            name: None,
            description: None,
            application: None,
            subdomain: None,
            aggregate: None,
            event_name: None,
            client_id: None,
        }
    }

    /// Initialize from execution context (like Java's .from(ctx))
    pub fn from_context(mut self, ctx: &ExecutionContext) -> Self {
        self.event_id = Some(TsidGenerator::generate());
        self.execution_id = Some(ctx.execution_id.clone());
        self.correlation_id = Some(ctx.correlation_id.clone());
        self.causation_id = ctx.causation_id.clone();
        self.principal_id = Some(ctx.principal_id.clone());
        self
    }

    pub fn event_type_id(mut self, id: impl Into<String>) -> Self {
        self.event_type_id = Some(id.into());
        self
    }

    pub fn code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    pub fn application(mut self, app: impl Into<String>) -> Self {
        self.application = Some(app.into());
        self
    }

    pub fn subdomain(mut self, subdomain: impl Into<String>) -> Self {
        self.subdomain = Some(subdomain.into());
        self
    }

    pub fn aggregate(mut self, aggregate: impl Into<String>) -> Self {
        self.aggregate = Some(aggregate.into());
        self
    }

    pub fn event_name(mut self, event_name: impl Into<String>) -> Self {
        self.event_name = Some(event_name.into());
        self
    }

    pub fn client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = Some(client_id.into());
        self
    }

    pub fn build(self) -> EventTypeCreated {
        let event_id = self.event_id.unwrap_or_else(|| TsidGenerator::generate());
        let event_type_id = self.event_type_id.expect("event_type_id is required");
        let subject = format!("platform.eventtype.{}", event_type_id);
        let message_group = format!("platform:eventtype:{}", event_type_id);

        EventTypeCreated {
            metadata: EventMetadata::new(
                event_id,
                EventTypeCreated::EVENT_TYPE,
                EventTypeCreated::SPEC_VERSION,
                EventTypeCreated::SOURCE,
                subject,
                message_group,
                self.execution_id.expect("execution_id is required"),
                self.correlation_id.expect("correlation_id is required"),
                self.causation_id,
                self.principal_id.expect("principal_id is required"),
            ),
            event_type_id,
            code: self.code.expect("code is required"),
            name: self.name.expect("name is required"),
            description: self.description,
            application: self.application.expect("application is required"),
            subdomain: self.subdomain.expect("subdomain is required"),
            aggregate: self.aggregate.expect("aggregate is required"),
            event_name: self.event_name.expect("event_name is required"),
            client_id: self.client_id,
        }
    }
}

impl Default for EventTypeCreatedBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Event emitted when an event type is updated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventTypeUpdated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub event_type_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl_domain_event!(EventTypeUpdated);

impl EventTypeUpdated {
    const EVENT_TYPE: &'static str = "platform:control-plane:eventtype:updated";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:control-plane";

    pub fn new(
        ctx: &ExecutionContext,
        event_type_id: &str,
        name: Option<&str>,
        description: Option<&str>,
    ) -> Self {
        let event_id = TsidGenerator::generate();
        let subject = format!("platform.eventtype.{}", event_type_id);
        let message_group = format!("platform:eventtype:{}", event_type_id);

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
            event_type_id: event_type_id.to_string(),
            name: name.map(String::from),
            description: description.map(String::from),
        }
    }
}

/// Event emitted when an event type is archived.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventTypeArchived {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub event_type_id: String,
    pub code: String,
}

impl_domain_event!(EventTypeArchived);

impl EventTypeArchived {
    const EVENT_TYPE: &'static str = "platform:control-plane:eventtype:archived";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:control-plane";

    pub fn new(ctx: &ExecutionContext, event_type_id: &str, code: &str) -> Self {
        let event_id = TsidGenerator::generate();
        let subject = format!("platform.eventtype.{}", event_type_id);
        let message_group = format!("platform:eventtype:{}", event_type_id);

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
            event_type_id: event_type_id.to_string(),
            code: code.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usecase::DomainEvent;

    #[test]
    fn test_event_type_created_builder() {
        let ctx = ExecutionContext::create("user-123");

        let event = EventTypeCreated::builder()
            .from_context(&ctx)
            .event_type_id("0HZXEQ5Y8JY5Z")
            .code("orders:fulfillment:shipment:shipped")
            .name("Shipment Shipped")
            .description("Emitted when a shipment leaves")
            .application("orders")
            .subdomain("fulfillment")
            .aggregate("shipment")
            .event_name("shipped")
            .build();

        assert_eq!(event.event_type(), "platform:control-plane:eventtype:created");
        assert_eq!(event.event_type_id, "0HZXEQ5Y8JY5Z");
        assert_eq!(event.code, "orders:fulfillment:shipment:shipped");
        assert_eq!(event.principal_id(), "user-123");
    }

    #[test]
    fn test_event_type_created_new() {
        let ctx = ExecutionContext::create("user-456");

        let event = EventTypeCreated::new(
            &ctx,
            "0HZXEQ5Y8JY5Z",
            "orders:fulfillment:shipment:shipped",
            "Shipment Shipped",
            Some("Emitted when shipped"),
            "orders",
            "fulfillment",
            "shipment",
            "shipped",
            None,
        );

        assert_eq!(event.subject(), "platform.eventtype.0HZXEQ5Y8JY5Z");
        assert_eq!(event.message_group(), "platform:eventtype:0HZXEQ5Y8JY5Z");
    }

    #[test]
    fn test_event_type_updated() {
        let ctx = ExecutionContext::create("user-123");
        let event = EventTypeUpdated::new(
            &ctx,
            "et-123",
            Some("New Name"),
            Some("New Description"),
        );

        assert_eq!(event.event_type(), "platform:control-plane:eventtype:updated");
        assert_eq!(event.event_type_id, "et-123");
        assert_eq!(event.name, Some("New Name".to_string()));
    }

    #[test]
    fn test_event_type_archived() {
        let ctx = ExecutionContext::create("user-123");
        let event = EventTypeArchived::new(&ctx, "et-123", "orders:fulfillment:order:created");

        assert_eq!(event.event_type(), "platform:control-plane:eventtype:archived");
        assert_eq!(event.code, "orders:fulfillment:order:created");
    }
}
