//! Application Domain Events
//!
//! Type, source, subject, message group and `data` are Go's
//! (`internal/platform/application/operations/events.go`): type
//! `platform:iam:application:*`, source `platform:iam`, subject
//! `platform.application.{id}`, group `platform:application:{id}`, and each
//! payload carries exactly Go's `ToDataJSON` fields.

use crate::impl_domain_event;
use crate::usecase::domain_event::{null_if_empty, EventMetadata};
use crate::usecase::ExecutionContext;
use serde::{Deserialize, Serialize};

const SPEC_VERSION: &str = "1.0";
const SOURCE: &str = "platform:iam";

fn metadata(ctx: &ExecutionContext, event_type: &str, application_id: &str) -> EventMetadata {
    EventMetadata::from_ctx(
        ctx,
        event_type,
        SPEC_VERSION,
        SOURCE,
        format!("platform.application.{}", application_id),
        format!("platform:application:{}", application_id),
    )
}

/// `{applicationId, code, name}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationCreated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub application_id: String,
    pub code: String,
    pub name: String,
}

impl_domain_event!(ApplicationCreated);

impl ApplicationCreated {
    pub const EVENT_TYPE: &'static str = "platform:iam:application:created";

    pub fn new(ctx: &ExecutionContext, application_id: &str, code: &str, name: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, application_id),
            application_id: application_id.to_string(),
            code: code.to_string(),
            name: name.to_string(),
        }
    }
}

/// `{applicationId, name}`: the application's name after the update.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationUpdated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub application_id: String,
    pub name: String,
}

impl_domain_event!(ApplicationUpdated);

impl ApplicationUpdated {
    pub const EVENT_TYPE: &'static str = "platform:iam:application:updated";

    pub fn new(ctx: &ExecutionContext, application_id: &str, name: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, application_id),
            application_id: application_id.to_string(),
            name: name.to_string(),
        }
    }
}

/// `{applicationId}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationActivated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub application_id: String,
}

impl_domain_event!(ApplicationActivated);

impl ApplicationActivated {
    pub const EVENT_TYPE: &'static str = "platform:iam:application:activated";

    pub fn new(ctx: &ExecutionContext, application_id: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, application_id),
            application_id: application_id.to_string(),
        }
    }
}

/// `{applicationId}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationDeactivated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub application_id: String,
}

impl_domain_event!(ApplicationDeactivated);

impl ApplicationDeactivated {
    pub const EVENT_TYPE: &'static str = "platform:iam:application:deactivated";

    pub fn new(ctx: &ExecutionContext, application_id: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, application_id),
            application_id: application_id.to_string(),
        }
    }
}

/// `{applicationId, applicationCode, serviceAccountId, serviceAccountCode}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationServiceAccountProvisioned {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub application_id: String,
    pub application_code: String,
    pub service_account_id: String,
    pub service_account_code: String,
}

impl_domain_event!(ApplicationServiceAccountProvisioned);

impl ApplicationServiceAccountProvisioned {
    pub const EVENT_TYPE: &'static str = "platform:iam:application:service-account-provisioned";

    pub fn new(
        ctx: &ExecutionContext,
        application_id: &str,
        application_code: &str,
        service_account_id: &str,
        service_account_code: &str,
    ) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, application_id),
            application_id: application_id.to_string(),
            application_code: application_code.to_string(),
            service_account_id: service_account_id.to_string(),
            service_account_code: service_account_code.to_string(),
        }
    }
}

/// `{applicationId, code}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationDeleted {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub application_id: String,
    pub code: String,
}

impl_domain_event!(ApplicationDeleted);

impl ApplicationDeleted {
    pub const EVENT_TYPE: &'static str = "platform:iam:application:deleted";

    pub fn new(ctx: &ExecutionContext, application_id: &str, code: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, application_id),
            application_id: application_id.to_string(),
            code: code.to_string(),
        }
    }
}

/// `{applicationId, clientId, configId}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationEnabledForClient {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub application_id: String,
    pub client_id: String,
    pub config_id: String,
}

impl_domain_event!(ApplicationEnabledForClient);

impl ApplicationEnabledForClient {
    pub const EVENT_TYPE: &'static str = "platform:iam:application:enabled-for-client";

    pub fn new(
        ctx: &ExecutionContext,
        application_id: &str,
        client_id: &str,
        config_id: &str,
    ) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, application_id),
            application_id: application_id.to_string(),
            client_id: client_id.to_string(),
            config_id: config_id.to_string(),
        }
    }
}

/// `{applicationId, clientId, configId}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationDisabledForClient {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub application_id: String,
    pub client_id: String,
    pub config_id: String,
}

impl_domain_event!(ApplicationDisabledForClient);

impl ApplicationDisabledForClient {
    pub const EVENT_TYPE: &'static str = "platform:iam:application:disabled-for-client";

    pub fn new(
        ctx: &ExecutionContext,
        application_id: &str,
        client_id: &str,
        config_id: &str,
    ) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, application_id),
            application_id: application_id.to_string(),
            client_id: client_id.to_string(),
            config_id: config_id.to_string(),
        }
    }
}

/// A client's per-application config was updated (base URL override,
/// config json, enabled flag). Go has no such operation; the event keeps its
/// Rust shape under the application family's Go source and subject.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationClientConfigUpdated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub application_id: String,
    pub client_id: String,
    pub config_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url_override: Option<String>,
    pub config_changed: bool,
}

impl_domain_event!(ApplicationClientConfigUpdated);

impl ApplicationClientConfigUpdated {
    pub const EVENT_TYPE: &'static str = "platform:iam:application:client-config-updated";

    /// Metadata for this event, raised inside `ctx`.
    pub fn metadata_for(ctx: &ExecutionContext, application_id: &str) -> EventMetadata {
        metadata(ctx, Self::EVENT_TYPE, application_id)
    }
}

/// `{clientId, enabledApplicationIds, enabledAdded, disabledRemoved}`, on the
/// client's subject and group. Go builds all three lists by appending to a
/// nil slice, so an empty one is `null`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientApplicationsUpdated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub client_id: String,
    /// Final, authoritative set of enabled applications after the update.
    #[serde(serialize_with = "null_if_empty")]
    pub enabled_application_ids: Vec<String>,
    /// Applications that became enabled in this operation.
    #[serde(serialize_with = "null_if_empty")]
    pub enabled_added: Vec<String>,
    /// Applications that became disabled in this operation.
    #[serde(serialize_with = "null_if_empty")]
    pub disabled_removed: Vec<String>,
}

impl_domain_event!(ClientApplicationsUpdated);

impl ClientApplicationsUpdated {
    pub const EVENT_TYPE: &'static str = "platform:iam:client:applications-updated";

    pub fn new(
        ctx: &ExecutionContext,
        client_id: &str,
        enabled_application_ids: Vec<String>,
        enabled_added: Vec<String>,
        disabled_removed: Vec<String>,
    ) -> Self {
        Self {
            metadata: EventMetadata::from_ctx(
                ctx,
                Self::EVENT_TYPE,
                SPEC_VERSION,
                SOURCE,
                format!("platform.client.{}", client_id),
                format!("platform:client:{}", client_id),
            ),
            client_id: client_id.to_string(),
            enabled_application_ids,
            enabled_added,
            disabled_removed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_application_created_event() {
        let ctx = ExecutionContext::create("admin-123");
        let event = ApplicationCreated::new(&ctx, "app-1", "orders", "Orders Application");

        assert_eq!(
            event.metadata.event_type,
            "platform:iam:application:created"
        );
        assert_eq!(event.metadata.source, "platform:iam");
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

        assert_eq!(
            event.metadata.event_type,
            "platform:iam:application:service-account-provisioned"
        );
        assert_eq!(event.service_account_id, "sa-1");
    }

    #[test]
    fn client_applications_updated_empty_lists_are_null() {
        let ctx = ExecutionContext::create("admin-123");
        let event = ClientApplicationsUpdated::new(&ctx, "clt_1", vec![], vec![], vec![]);
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            serde_json::json!({
                "clientId": "clt_1",
                "enabledApplicationIds": null,
                "enabledAdded": null,
                "disabledRemoved": null
            })
        );
    }
}
