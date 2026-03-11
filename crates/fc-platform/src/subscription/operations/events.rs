//! Subscription Domain Events

use serde::{Deserialize, Serialize};
use crate::usecase::ExecutionContext;
use crate::usecase::domain_event::EventMetadata;
use crate::TsidGenerator;
use crate::EntityType;
use crate::impl_domain_event;

/// Event emitted when a new subscription is created.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionCreated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub subscription_id: String,
    pub code: String,
    pub name: String,
    pub connection_id: String,
    pub event_types: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

impl_domain_event!(SubscriptionCreated);

impl SubscriptionCreated {
    const EVENT_TYPE: &'static str = "platform:subscription:created";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:subscription";

    pub fn new(
        ctx: &ExecutionContext,
        subscription_id: &str,
        code: &str,
        name: &str,
        connection_id: &str,
        event_types: Vec<String>,
        client_id: Option<&str>,
    ) -> Self {
        let event_id = TsidGenerator::generate(EntityType::Event);
        let subject = format!("platform.subscription.{}", subscription_id);
        let message_group = format!("platform:subscription:{}", subscription_id);

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
            subscription_id: subscription_id.to_string(),
            code: code.to_string(),
            name: name.to_string(),
            connection_id: connection_id.to_string(),
            event_types,
            client_id: client_id.map(String::from),
        }
    }
}

/// Event emitted when a subscription is updated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionUpdated {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub subscription_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub event_types_added: Vec<String>,
    pub event_types_removed: Vec<String>,
}

impl_domain_event!(SubscriptionUpdated);

impl SubscriptionUpdated {
    const EVENT_TYPE: &'static str = "platform:subscription:updated";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:subscription";

    pub fn new(
        ctx: &ExecutionContext,
        subscription_id: &str,
        name: Option<&str>,
        event_types_added: Vec<String>,
        event_types_removed: Vec<String>,
    ) -> Self {
        let event_id = TsidGenerator::generate(EntityType::Event);
        let subject = format!("platform.subscription.{}", subscription_id);
        let message_group = format!("platform:subscription:{}", subscription_id);

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
            subscription_id: subscription_id.to_string(),
            name: name.map(String::from),
            event_types_added,
            event_types_removed,
        }
    }
}

/// Event emitted when a subscription is paused.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionPaused {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub subscription_id: String,
    pub code: String,
}

impl_domain_event!(SubscriptionPaused);

impl SubscriptionPaused {
    const EVENT_TYPE: &'static str = "platform:subscription:paused";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:subscription";

    pub fn new(ctx: &ExecutionContext, subscription_id: &str, code: &str) -> Self {
        let event_id = TsidGenerator::generate(EntityType::Event);
        let subject = format!("platform.subscription.{}", subscription_id);
        let message_group = format!("platform:subscription:{}", subscription_id);

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
            subscription_id: subscription_id.to_string(),
            code: code.to_string(),
        }
    }
}

/// Event emitted when a subscription is resumed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionResumed {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub subscription_id: String,
    pub code: String,
}

impl_domain_event!(SubscriptionResumed);

impl SubscriptionResumed {
    const EVENT_TYPE: &'static str = "platform:subscription:resumed";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:subscription";

    pub fn new(ctx: &ExecutionContext, subscription_id: &str, code: &str) -> Self {
        let event_id = TsidGenerator::generate(EntityType::Event);
        let subject = format!("platform.subscription.{}", subscription_id);
        let message_group = format!("platform:subscription:{}", subscription_id);

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
            subscription_id: subscription_id.to_string(),
            code: code.to_string(),
        }
    }
}

/// Event emitted when a subscription is deleted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionDeleted {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub subscription_id: String,
    pub code: String,
}

impl_domain_event!(SubscriptionDeleted);

impl SubscriptionDeleted {
    const EVENT_TYPE: &'static str = "platform:subscription:deleted";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:subscription";

    pub fn new(ctx: &ExecutionContext, subscription_id: &str, code: &str) -> Self {
        let event_id = TsidGenerator::generate(EntityType::Event);
        let subject = format!("platform.subscription.{}", subscription_id);
        let message_group = format!("platform:subscription:{}", subscription_id);

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
            subscription_id: subscription_id.to_string(),
            code: code.to_string(),
        }
    }
}

/// Event emitted when subscriptions are synced from an application SDK.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionsSynced {
    #[serde(flatten)]
    pub metadata: EventMetadata,

    pub application_code: String,
    pub created: u32,
    pub updated: u32,
    pub deleted: u32,
    pub synced_codes: Vec<String>,
}

impl_domain_event!(SubscriptionsSynced);

impl SubscriptionsSynced {
    const EVENT_TYPE: &'static str = "platform:subscription:synced";
    const SPEC_VERSION: &'static str = "1.0";
    const SOURCE: &'static str = "platform:subscription";

    pub fn new(
        ctx: &ExecutionContext,
        application_code: &str,
        created: u32,
        updated: u32,
        deleted: u32,
        synced_codes: Vec<String>,
    ) -> Self {
        let event_id = TsidGenerator::generate(EntityType::Event);
        let subject = format!("platform.application.{}", application_code);
        let message_group = format!("platform:application:{}", application_code);

        Self {
            metadata: EventMetadata::new(
                event_id, Self::EVENT_TYPE, Self::SPEC_VERSION, Self::SOURCE,
                subject, message_group,
                ctx.execution_id.clone(), ctx.correlation_id.clone(),
                ctx.causation_id.clone(), ctx.principal_id.clone(),
            ),
            application_code: application_code.to_string(),
            created,
            updated,
            deleted,
            synced_codes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usecase::DomainEvent;

    #[test]
    fn test_subscription_created_event() {
        let ctx = ExecutionContext::create("admin-123");
        let event = SubscriptionCreated::new(
            &ctx,
            "sub-1",
            "order-webhook",
            "Order Webhook",
            "https://example.com/webhook",
            vec!["orders:*:*:*".to_string()],
            Some("client-1"),
        );

        assert_eq!(event.event_type(), "platform:subscription:created");
        assert_eq!(event.subscription_id, "sub-1");
        assert_eq!(event.code, "order-webhook");
    }

    #[test]
    fn test_subscription_paused_event() {
        let ctx = ExecutionContext::create("admin-123");
        let event = SubscriptionPaused::new(&ctx, "sub-1", "order-webhook");

        assert_eq!(event.event_type(), "platform:subscription:paused");
        assert_eq!(event.code, "order-webhook");
    }

    #[test]
    fn test_subscription_deleted_event() {
        let ctx = ExecutionContext::create("admin-123");
        let event = SubscriptionDeleted::new(&ctx, "sub-1", "order-webhook");

        assert_eq!(event.event_type(), "platform:subscription:deleted");
        assert_eq!(event.code, "order-webhook");
    }
}
