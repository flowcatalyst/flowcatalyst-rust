//! Service Account Domain Events
//!
//! Type, source, subject, message group and `data` are Go's
//! (`internal/platform/serviceaccount/operations/events.go`): type
//! `platform:iam:serviceaccount:*` (no hyphen in the aggregate), source
//! `platform:iam`, subject `platform.serviceaccount.{id}`, group
//! `platform:serviceaccount:{id}`, and each payload carries exactly Go's
//! `ToDataJSON` fields. No payload ever carries a token or secret.
//!
//! `{id}` and `serviceAccountId` are the account's own id (`sac_…`,
//! [`ServiceAccount::account_id`]), never its SERVICE principal's (`prn_…`):
//! Go builds every one of these from `sa.ID`. The constructors take the
//! account so no caller can hand them the principal's id; the audit row's
//! `entityId` follows the subject.

use crate::impl_domain_event;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::ExecutionContext;
use crate::ServiceAccount;
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

    pub fn new(ctx: &ExecutionContext, account: &ServiceAccount) -> Self {
        let id = account.account_id();
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, id),
            service_account_id: id.to_string(),
            code: account.code.clone(),
            name: account.name.clone(),
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

    pub fn new(ctx: &ExecutionContext, account: &ServiceAccount) -> Self {
        let id = account.account_id();
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, id),
            service_account_id: id.to_string(),
            name: account.name.clone(),
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

    pub fn new(ctx: &ExecutionContext, account: &ServiceAccount) -> Self {
        let id = account.account_id();
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, id),
            service_account_id: id.to_string(),
            code: account.code.clone(),
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

    pub fn new(ctx: &ExecutionContext, account: &ServiceAccount) -> Self {
        let id = account.account_id();
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, id),
            service_account_id: id.to_string(),
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
        account: &ServiceAccount,
        roles_added: Vec<String>,
        roles_removed: Vec<String>,
    ) -> Self {
        let id = account.account_id();
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, id),
            service_account_id: id.to_string(),
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

    pub fn new(ctx: &ExecutionContext, account: &ServiceAccount) -> Self {
        let id = account.account_id();
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, id),
            service_account_id: id.to_string(),
            code: account.code.clone(),
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

    pub fn new(ctx: &ExecutionContext, account: &ServiceAccount) -> Self {
        let id = account.account_id();
        Self {
            metadata: metadata(ctx, Self::EVENT_TYPE, id),
            service_account_id: id.to_string(),
            code: account.code.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::principal::entity::UserScope;

    /// An account whose SERVICE principal (`prn_1`) and own row (`sac_1`)
    /// have different ids, as every account does.
    fn account() -> ServiceAccount {
        let mut sa = ServiceAccount::new("my-service", "My Service", UserScope::Anchor);
        sa.id = "prn_1".to_string();
        sa.service_account_table_id = Some("sac_1".to_string());
        sa
    }

    #[test]
    fn every_event_carries_the_account_id_not_the_principal_id() {
        let ctx = ExecutionContext::system("admin-123");
        let sa = account();
        let subject = "platform.serviceaccount.sac_1";
        let group = "platform:serviceaccount:sac_1";
        let metas = [
            (
                ServiceAccountCreated::new(&ctx, &sa).metadata,
                ServiceAccountCreated::new(&ctx, &sa).service_account_id,
            ),
            (
                ServiceAccountUpdated::new(&ctx, &sa).metadata,
                ServiceAccountUpdated::new(&ctx, &sa).service_account_id,
            ),
            (
                ServiceAccountDeactivated::new(&ctx, &sa).metadata,
                ServiceAccountDeactivated::new(&ctx, &sa).service_account_id,
            ),
            (
                ServiceAccountDeleted::new(&ctx, &sa).metadata,
                ServiceAccountDeleted::new(&ctx, &sa).service_account_id,
            ),
            (
                ServiceAccountRolesAssigned::new(&ctx, &sa, vec![], vec![]).metadata,
                ServiceAccountRolesAssigned::new(&ctx, &sa, vec![], vec![]).service_account_id,
            ),
            (
                ServiceAccountTokenRegenerated::new(&ctx, &sa).metadata,
                ServiceAccountTokenRegenerated::new(&ctx, &sa).service_account_id,
            ),
            (
                ServiceAccountSecretRegenerated::new(&ctx, &sa).metadata,
                ServiceAccountSecretRegenerated::new(&ctx, &sa).service_account_id,
            ),
        ];
        for (meta, id) in metas {
            assert_eq!(meta.subject, subject, "{}", meta.event_type);
            assert_eq!(meta.message_group, group, "{}", meta.event_type);
            assert_eq!(id, "sac_1", "{}", meta.event_type);
        }
    }

    #[test]
    fn a_legacy_principal_without_an_account_row_uses_its_own_id() {
        let mut sa = account();
        sa.service_account_table_id = None;
        let e = ServiceAccountCreated::new(&ExecutionContext::system("admin-123"), &sa);
        assert_eq!(e.service_account_id, "prn_1");
    }

    #[test]
    fn test_service_account_created_event() {
        let ctx = ExecutionContext::system("admin-123");
        let event = ServiceAccountCreated::new(&ctx, &account());

        assert_eq!(
            event.metadata.event_type,
            "platform:iam:serviceaccount:created"
        );
        assert_eq!(event.metadata.source, "platform:iam");
        assert_eq!(event.code, "my-service");
        assert_eq!(event.name, "My Service");
    }

    #[test]
    fn test_service_account_roles_assigned_event() {
        let ctx = ExecutionContext::system("admin-123");
        let event = ServiceAccountRolesAssigned::new(
            &ctx,
            &account(),
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
