//! Subscription Domain Events
//!
//! Type, source, subject, message group and `data` are Go's
//! (`internal/platform/subscription/operations/events.go`): type
//! `platform:admin:subscription:*`, source `platform:admin`, subject
//! `platform.subscription.{id}`, group `platform:subscription:{id}`, and
//! each payload carries exactly Go's `ToDataJSON` fields.

use crate::impl_domain_event;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::ExecutionContext;
use serde::{Deserialize, Serialize};

const SPEC_VERSION: &str = "1.0";
const SOURCE: &str = "platform:admin";

fn metadata(ctx: &ExecutionContext, event_type: &str, subscription_id: &str) -> EventMetadata {
    EventMetadata::from_ctx(
        ctx,
        event_type,
        SPEC_VERSION,
        SOURCE,
        format!("platform.subscription.{}", subscription_id),
        format!("platform:subscription:{}", subscription_id),
    )
}

/// `{subscriptionId, code, name}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionCreated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub subscription_id: String,
    pub code: String,
    pub name: String,
}

impl_domain_event!(SubscriptionCreated);

impl SubscriptionCreated {
    pub const EVENT_TYPE: &'static str = "platform:admin:subscription:created";

    pub fn new(ctx: &ExecutionContext, subscription_id: &str, code: &str, name: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, subscription_id),
            subscription_id: subscription_id.to_string(),
            code: code.to_string(),
            name: name.to_string(),
        }
    }
}

/// `{subscriptionId, name}`: the subscription's name after the update.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionUpdated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub subscription_id: String,
    pub name: String,
}

impl_domain_event!(SubscriptionUpdated);

impl SubscriptionUpdated {
    pub const EVENT_TYPE: &'static str = "platform:admin:subscription:updated";

    pub fn new(ctx: &ExecutionContext, subscription_id: &str, name: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, subscription_id),
            subscription_id: subscription_id.to_string(),
            name: name.to_string(),
        }
    }
}

macro_rules! id_only_event {
    ($(#[$doc:meta])* $name:ident, $event_type:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct $name {
            #[serde(skip)]
            pub metadata: EventMetadata,
            pub subscription_id: String,
        }

        impl_domain_event!($name);

        impl $name {
            pub const EVENT_TYPE: &'static str = $event_type;

            pub fn new(ctx: &ExecutionContext, subscription_id: &str) -> Self {
                Self {
                    metadata: metadata(ctx, Self::EVENT_TYPE, subscription_id),
                    subscription_id: subscription_id.to_string(),
                }
            }
        }
    };
}

id_only_event!(
    /// `{subscriptionId}`.
    SubscriptionPaused,
    "platform:admin:subscription:paused"
);
id_only_event!(
    /// `{subscriptionId}`.
    SubscriptionResumed,
    "platform:admin:subscription:resumed"
);

/// `{subscriptionId, code}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionDeleted {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub subscription_id: String,
    pub code: String,
}

impl_domain_event!(SubscriptionDeleted);

impl SubscriptionDeleted {
    pub const EVENT_TYPE: &'static str = "platform:admin:subscription:deleted";

    pub fn new(ctx: &ExecutionContext, subscription_id: &str, code: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, subscription_id),
            subscription_id: subscription_id.to_string(),
            code: code.to_string(),
        }
    }
}

/// The rollup of an SDK subscription sync:
/// `{applicationCode, clientId?, created, updated, deleted, syncedCodes}` on
/// subject `platform.subscriptions.{applicationCode}` and group
/// `platform:subscriptions:{applicationCode}`. `clientId` is omitted for a
/// sync of the application's client-less subscriptions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionsSynced {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub application_code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub created: u32,
    pub updated: u32,
    pub deleted: u32,
    pub synced_codes: Vec<String>,
}

impl_domain_event!(SubscriptionsSynced);

impl SubscriptionsSynced {
    pub const EVENT_TYPE: &'static str = "platform:admin:subscription:synced";

    /// Metadata for this event, raised inside `ctx` for a sync of
    /// `application_code`.
    pub fn metadata_for(ctx: &ExecutionContext, application_code: &str) -> EventMetadata {
        let group = if application_code.is_empty() {
            "platform:subscriptions".to_string()
        } else {
            format!("platform:subscriptions:{}", application_code)
        };
        EventMetadata::from_ctx(
            ctx,
            Self::EVENT_TYPE,
            SPEC_VERSION,
            SOURCE,
            format!("platform.subscriptions.{}", application_code),
            group,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscription_events_are_go_shaped() {
        let ctx = ExecutionContext::create("prn_1");
        let e = SubscriptionPaused::new(&ctx, "sub-1");
        assert_eq!(e.metadata.event_type, "platform:admin:subscription:paused");
        assert_eq!(e.metadata.message_group, "platform:subscription:sub-1");
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            serde_json::json!({"subscriptionId": "sub-1"})
        );
        let meta = SubscriptionsSynced::metadata_for(&ctx, "orders");
        assert_eq!(meta.subject, "platform.subscriptions.orders");
        assert_eq!(meta.message_group, "platform:subscriptions:orders");
    }
}
