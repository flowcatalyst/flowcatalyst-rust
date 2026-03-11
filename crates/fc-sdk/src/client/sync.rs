//! Sync operations for bulk reconciliation.
//!
//! Sync endpoints reconcile application-scoped resources with a declarative manifest.
//! They create, update, and optionally delete resources to match the provided list.

use serde::{Deserialize, Serialize};
use super::{FlowCatalystClient, ClientError};
use super::event_types::CreateEventTypeRequest;
use super::subscriptions::CreateSubscriptionRequest;

/// Result of a sync operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    pub created: u32,
    pub updated: u32,
    pub deleted: u32,
}

/// Request to sync roles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRolesRequest {
    pub roles: Vec<SyncRoleItem>,
}

/// A role item for sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRoleItem {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
}

/// Request to sync event types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncEventTypesRequest {
    pub event_types: Vec<CreateEventTypeRequest>,
}

/// Request to sync subscriptions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncSubscriptionsRequest {
    pub subscriptions: Vec<CreateSubscriptionRequest>,
}

/// Request to sync dispatch pools.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncDispatchPoolsRequest {
    pub dispatch_pools: Vec<SyncDispatchPoolItem>,
}

/// A dispatch pool item for sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncDispatchPoolItem {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit_per_minute: Option<u32>,
}

impl FlowCatalystClient {
    /// Sync roles for an application.
    ///
    /// `remove_unlisted`: if true, roles not in the request will be deleted.
    pub async fn sync_roles(
        &self,
        app_code: &str,
        req: &SyncRolesRequest,
        remove_unlisted: bool,
    ) -> Result<SyncResult, ClientError> {
        let query = if remove_unlisted {
            "?removeUnlisted=true"
        } else {
            ""
        };
        self.post(
            &format!("/api/applications/{}/sync/roles{}", app_code, query),
            req,
        )
        .await
    }

    /// Sync event types for an application.
    pub async fn sync_event_types(
        &self,
        app_code: &str,
        req: &SyncEventTypesRequest,
        remove_unlisted: bool,
    ) -> Result<SyncResult, ClientError> {
        let query = if remove_unlisted {
            "?removeUnlisted=true"
        } else {
            ""
        };
        self.post(
            &format!(
                "/api/applications/{}/sync/event-types{}",
                app_code, query
            ),
            req,
        )
        .await
    }

    /// Sync subscriptions for an application.
    pub async fn sync_subscriptions(
        &self,
        app_code: &str,
        req: &SyncSubscriptionsRequest,
        remove_unlisted: bool,
    ) -> Result<SyncResult, ClientError> {
        let query = if remove_unlisted {
            "?removeUnlisted=true"
        } else {
            ""
        };
        self.post(
            &format!(
                "/api/applications/{}/sync/subscriptions{}",
                app_code, query
            ),
            req,
        )
        .await
    }

    /// Sync dispatch pools for an application.
    pub async fn sync_dispatch_pools(
        &self,
        app_code: &str,
        req: &SyncDispatchPoolsRequest,
        remove_unlisted: bool,
    ) -> Result<SyncResult, ClientError> {
        let query = if remove_unlisted {
            "?removeUnlisted=true"
        } else {
            ""
        };
        self.post(
            &format!(
                "/api/applications/{}/sync/dispatch-pools{}",
                app_code, query
            ),
            req,
        )
        .await
    }
}
