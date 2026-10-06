//! Application Domain Events
//!
//! Type, source, subject, message group and `data` are Go's
//! (`internal/platform/application/operations/events.go`): type
//! `platform:iam:application:*`, source `platform:iam`, subject
//! `platform.application.{id}`, group `platform:application:{id}`, and each
//! payload carries exactly Go's `ToDataJSON` fields.

use super::update_client_config::UpdateApplicationClientConfigCommand;
use crate::application::client_config::ApplicationClientConfig;
use fc_platform_core::impl_domain_event;
use fc_platform_core::shared::id::AppClientConfigId;
use fc_platform_core::shared::id::ApplicationId;
use fc_platform_core::shared::id::ClientId;
use fc_platform_core::usecase::domain_event::{null_if_empty, EventMetadata};
use fc_platform_core::usecase::ExecutionContext;
use serde::{Deserialize, Serialize};

const SPEC_VERSION: &str = "1.0";
const SOURCE: &str = "platform:iam";

fn metadata(
    ctx: &ExecutionContext,
    event_type: &str,
    application_id: &ApplicationId,
) -> EventMetadata {
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
    pub application_id: ApplicationId,
    pub code: String,
    pub name: String,
}

impl_domain_event!(ApplicationCreated);

impl ApplicationCreated {
    pub const EVENT_TYPE: &'static str = "platform:iam:application:created";

    pub fn new(
        ctx: &ExecutionContext,
        application_id: &ApplicationId,
        code: &str,
        name: &str,
    ) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, application_id),
            application_id: application_id.clone(),
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
    pub application_id: ApplicationId,
    pub name: String,
}

impl_domain_event!(ApplicationUpdated);

impl ApplicationUpdated {
    pub const EVENT_TYPE: &'static str = "platform:iam:application:updated";

    pub fn new(ctx: &ExecutionContext, application_id: &ApplicationId, name: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, application_id),
            application_id: application_id.clone(),
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
    pub application_id: ApplicationId,
}

impl_domain_event!(ApplicationActivated);

impl ApplicationActivated {
    pub const EVENT_TYPE: &'static str = "platform:iam:application:activated";

    pub fn new(ctx: &ExecutionContext, application_id: &ApplicationId) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, application_id),
            application_id: application_id.clone(),
        }
    }
}

/// `{applicationId}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationDeactivated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub application_id: ApplicationId,
}

impl_domain_event!(ApplicationDeactivated);

impl ApplicationDeactivated {
    pub const EVENT_TYPE: &'static str = "platform:iam:application:deactivated";

    pub fn new(ctx: &ExecutionContext, application_id: &ApplicationId) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, application_id),
            application_id: application_id.clone(),
        }
    }
}

/// `{applicationId, applicationCode, serviceAccountId, serviceAccountCode}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationServiceAccountProvisioned {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub application_id: ApplicationId,
    pub application_code: String,
    pub service_account_id: String,
    pub service_account_code: String,
}

impl_domain_event!(ApplicationServiceAccountProvisioned);

impl ApplicationServiceAccountProvisioned {
    pub const EVENT_TYPE: &'static str = "platform:iam:application:service-account-provisioned";

    pub fn new(
        ctx: &ExecutionContext,
        application_id: &ApplicationId,
        application_code: &str,
        service_account_id: &str,
        service_account_code: &str,
    ) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, application_id),
            application_id: application_id.clone(),
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
    pub application_id: ApplicationId,
    pub code: String,
}

impl_domain_event!(ApplicationDeleted);

impl ApplicationDeleted {
    pub const EVENT_TYPE: &'static str = "platform:iam:application:deleted";

    pub fn new(ctx: &ExecutionContext, application_id: &ApplicationId, code: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, application_id),
            application_id: application_id.clone(),
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
    pub application_id: ApplicationId,
    pub client_id: ClientId,
    pub config_id: AppClientConfigId,
}

impl_domain_event!(ApplicationEnabledForClient);

impl ApplicationEnabledForClient {
    pub const EVENT_TYPE: &'static str = "platform:iam:application:enabled-for-client";

    pub fn new(
        ctx: &ExecutionContext,
        application_id: &ApplicationId,
        client_id: &ClientId,
        config_id: &AppClientConfigId,
    ) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, application_id),
            application_id: application_id.clone(),
            client_id: client_id.clone(),
            config_id: config_id.clone(),
        }
    }
}

/// `{applicationId, clientId, configId}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationDisabledForClient {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub application_id: ApplicationId,
    pub client_id: ClientId,
    pub config_id: AppClientConfigId,
}

impl_domain_event!(ApplicationDisabledForClient);

impl ApplicationDisabledForClient {
    pub const EVENT_TYPE: &'static str = "platform:iam:application:disabled-for-client";

    pub fn new(
        ctx: &ExecutionContext,
        application_id: &ApplicationId,
        client_id: &ClientId,
        config_id: &AppClientConfigId,
    ) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, application_id),
            application_id: application_id.clone(),
            client_id: client_id.clone(),
            config_id: config_id.clone(),
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
    pub application_id: ApplicationId,
    pub client_id: ClientId,
    pub config_id: AppClientConfigId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url_override: Option<String>,
    pub config_changed: bool,
}

impl_domain_event!(ApplicationClientConfigUpdated);

impl ApplicationClientConfigUpdated {
    pub const EVENT_TYPE: &'static str = "platform:iam:application:client-config-updated";

    /// The event for `config` as `command` updated it inside `ctx`: the
    /// fields as requested, and whether a new config document was sent.
    pub fn new(
        ctx: &ExecutionContext,
        command: &UpdateApplicationClientConfigCommand,
        config: &ApplicationClientConfig,
    ) -> Self {
        Self {
            metadata: Self::metadata_for(ctx, &command.application_id),
            application_id: command.application_id.clone(),
            client_id: command.client_id.clone(),
            config_id: config.id.clone(),
            enabled: command.enabled,
            base_url_override: command.base_url_override.clone(),
            config_changed: command.config.is_some(),
        }
    }

    /// Metadata for this event, raised inside `ctx`.
    pub fn metadata_for(ctx: &ExecutionContext, application_id: &ApplicationId) -> EventMetadata {
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
    pub client_id: ClientId,
    /// Final, authoritative set of enabled applications after the update.
    #[serde(serialize_with = "null_if_empty")]
    pub enabled_application_ids: Vec<ApplicationId>,
    /// Applications that became enabled in this operation.
    #[serde(serialize_with = "null_if_empty")]
    pub enabled_added: Vec<ApplicationId>,
    /// Applications that became disabled in this operation.
    #[serde(serialize_with = "null_if_empty")]
    pub disabled_removed: Vec<ApplicationId>,
}

impl_domain_event!(ClientApplicationsUpdated);

impl ClientApplicationsUpdated {
    pub const EVENT_TYPE: &'static str = "platform:iam:client:applications-updated";

    pub fn new(
        ctx: &ExecutionContext,
        client_id: &ClientId,
        enabled_application_ids: Vec<ApplicationId>,
        enabled_added: Vec<ApplicationId>,
        disabled_removed: Vec<ApplicationId>,
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
            client_id: client_id.clone(),
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
        let ctx = ExecutionContext::system("admin-123");
        let event = ApplicationCreated::new(
            &ctx,
            &ApplicationId::parse("app_1").unwrap(),
            "orders",
            "Orders Application",
        );

        assert_eq!(
            event.metadata.event_type,
            "platform:iam:application:created"
        );
        assert_eq!(event.metadata.source, "platform:iam");
        assert_eq!(event.application_id.as_str(), "app_1");
        assert_eq!(event.code, "orders");
    }

    #[test]
    fn test_application_service_account_provisioned_event() {
        let ctx = ExecutionContext::system("admin-123");
        let event = ApplicationServiceAccountProvisioned::new(
            &ctx,
            &ApplicationId::parse("app_1").unwrap(),
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
        let ctx = ExecutionContext::system("admin-123");
        let event = ClientApplicationsUpdated::new(
            &ctx,
            &ClientId::parse("clt_1").unwrap(),
            vec![],
            vec![],
            vec![],
        );
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
