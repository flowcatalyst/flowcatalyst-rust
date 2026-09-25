//! Service Account Domain Events
//!
//! Type, source, subject, message group and `data` are Go's
//! (`internal/platform/serviceaccount/operations/events.go`): type
//! `platform:iam:serviceaccount:*` (no hyphen in the aggregate), source
//! `platform:iam`, subject `platform.serviceaccount.{id}`, group
//! `platform:serviceaccount:{id}`, and each payload carries exactly Go's
//! `ToDataJSON` fields. No payload ever carries a token or secret.

use crate::impl_domain_event;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::ExecutionContext;
use serde::{Deserialize, Serialize};

const SPEC_VERSION: &str = "1.0";
const SOURCE: &str = "platform:iam";

fn metadata(ctx: &ExecutionContext, event_type: &str, service_account_id: &str) -> EventMetadata {
    EventMetadata::from_ctx(
        ctx,
        event_type,
        SPEC_VERSION,
        SOURCE,
        format!("platform.serviceaccount.{}", service_account_id),
        format!("platform:serviceaccount:{}", service_account_id),
    )
}

/// `{serviceAccountId, code, name}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccountCreated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub service_account_id: String,
    pub code: String,
    pub name: String,
}

impl_domain_event!(ServiceAccountCreated);

impl ServiceAccountCreated {
    pub const EVENT_TYPE: &'static str = "platform:iam:serviceaccount:created";

    pub fn new(ctx: &ExecutionContext, service_account_id: &str, code: &str, name: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, service_account_id),
            service_account_id: service_account_id.to_string(),
            code: code.to_string(),
            name: name.to_string(),
        }
    }
}

/// `{serviceAccountId, name}`: the account's name after the update.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccountUpdated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub service_account_id: String,
    pub name: String,
}

impl_domain_event!(ServiceAccountUpdated);

impl ServiceAccountUpdated {
    pub const EVENT_TYPE: &'static str = "platform:iam:serviceaccount:updated";

    pub fn new(ctx: &ExecutionContext, service_account_id: &str, name: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, service_account_id),
            service_account_id: service_account_id.to_string(),
            name: name.to_string(),
        }
    }
}

/// `{serviceAccountId, code}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccountDeleted {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub service_account_id: String,
    pub code: String,
}

impl_domain_event!(ServiceAccountDeleted);

impl ServiceAccountDeleted {
    pub const EVENT_TYPE: &'static str = "platform:iam:serviceaccount:deleted";

    pub fn new(ctx: &ExecutionContext, service_account_id: &str, code: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, service_account_id),
            service_account_id: service_account_id.to_string(),
            code: code.to_string(),
        }
    }
}

/// `{serviceAccountId}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccountDeactivated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub service_account_id: String,
}

impl_domain_event!(ServiceAccountDeactivated);

impl ServiceAccountDeactivated {
    pub const EVENT_TYPE: &'static str = "platform:iam:serviceaccount:deactivated";

    pub fn new(ctx: &ExecutionContext, service_account_id: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, service_account_id),
            service_account_id: service_account_id.to_string(),
        }
    }
}

/// `{serviceAccountId, rolesAdded, rolesRemoved}`: empty lists are `[]`
/// (Go's `defaultEmpty`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccountRolesAssigned {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub service_account_id: String,
    pub roles_added: Vec<String>,
    pub roles_removed: Vec<String>,
}

impl_domain_event!(ServiceAccountRolesAssigned);

impl ServiceAccountRolesAssigned {
    pub const EVENT_TYPE: &'static str = "platform:iam:serviceaccount:roles-assigned";

    pub fn new(
        ctx: &ExecutionContext,
        service_account_id: &str,
        roles_added: Vec<String>,
        roles_removed: Vec<String>,
    ) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, service_account_id),
            service_account_id: service_account_id.to_string(),
            roles_added,
            roles_removed,
        }
    }
}

/// `{serviceAccountId, code}`. The new token goes back to the caller only.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccountTokenRegenerated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub service_account_id: String,
    pub code: String,
}

impl_domain_event!(ServiceAccountTokenRegenerated);

impl ServiceAccountTokenRegenerated {
    pub const EVENT_TYPE: &'static str = "platform:iam:serviceaccount:token-regenerated";

    pub fn new(ctx: &ExecutionContext, service_account_id: &str, code: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, service_account_id),
            service_account_id: service_account_id.to_string(),
            code: code.to_string(),
        }
    }
}

/// `{serviceAccountId, code}`. The new secret goes back to the caller only.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccountSecretRegenerated {
    #[serde(skip)]
    pub metadata: EventMetadata,
    pub service_account_id: String,
    pub code: String,
}

impl_domain_event!(ServiceAccountSecretRegenerated);

impl ServiceAccountSecretRegenerated {
    pub const EVENT_TYPE: &'static str = "platform:iam:serviceaccount:secret-regenerated";

    pub fn new(ctx: &ExecutionContext, service_account_id: &str, code: &str) -> Self {
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, service_account_id),
            service_account_id: service_account_id.to_string(),
            code: code.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_account_created_event() {
        let ctx = ExecutionContext::create("admin-123");
        let event = ServiceAccountCreated::new(&ctx, "sa-1", "my-service", "My Service");

        assert_eq!(
            event.metadata.event_type,
            "platform:iam:serviceaccount:created"
        );
        assert_eq!(event.metadata.source, "platform:iam");
        assert_eq!(event.service_account_id, "sa-1");
        assert_eq!(event.code, "my-service");
    }

    #[test]
    fn test_service_account_deleted_event() {
        let ctx = ExecutionContext::create("admin-123");
        let event = ServiceAccountDeleted::new(&ctx, "sa-1", "my-service");

        assert_eq!(
            event.metadata.event_type,
            "platform:iam:serviceaccount:deleted"
        );
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

        assert_eq!(
            event.metadata.event_type,
            "platform:iam:serviceaccount:roles-assigned"
        );
        assert_eq!(event.roles_added, vec!["ADMIN".to_string()]);
        assert_eq!(event.roles_removed, vec!["VIEWER".to_string()]);
    }
}
