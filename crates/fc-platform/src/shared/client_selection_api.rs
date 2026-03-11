//! Client Selection API
//!
//! REST endpoints for client context switching in multi-tenant environment.
//! Available only in embedded auth mode.

use axum::{
    routing::{get, post},
    extract::State,
    Json, Router,
};
use utoipa::ToSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::collections::HashSet;

use crate::{Principal, UserScope, ClientStatus};
use crate::{PrincipalRepository, ClientRepository, RoleRepository, ClientAccessGrantRepository};
use crate::AuthService;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;

/// Client info response
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    /// Client ID (TSID)
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Unique identifier/slug
    pub identifier: String,
}

/// Accessible clients response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AccessibleClientsResponse {
    /// List of accessible clients
    pub clients: Vec<ClientInfo>,
    /// Current client ID (if set)
    pub current_client_id: Option<String>,
    /// Whether user has global access (anchor scope)
    pub global_access: bool,
}

/// Switch client request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SwitchClientRequest {
    /// Client ID to switch to
    pub client_id: String,
}

/// Switch client response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SwitchClientResponse {
    /// New session token with client context
    pub token: String,
    /// Client info
    pub client: ClientInfo,
    /// User's roles
    pub roles: Vec<String>,
    /// Resolved permissions
    pub permissions: Vec<String>,
}

/// Current client response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CurrentClientResponse {
    /// Current client info (if set)
    pub client: Option<ClientInfo>,
    /// Whether user has no client context
    pub no_client_context: bool,
}

/// Client selection service state
#[derive(Clone)]
pub struct ClientSelectionState {
    pub principal_repo: Arc<PrincipalRepository>,
    pub client_repo: Arc<ClientRepository>,
    pub role_repo: Arc<RoleRepository>,
    pub grant_repo: Arc<ClientAccessGrantRepository>,
    pub auth_service: Arc<AuthService>,
}

impl ClientSelectionState {
    /// Add active grants to client IDs list
    async fn add_active_grants(&self, client_ids: &mut Vec<String>, principal_id: &str) -> Result<(), PlatformError> {
        let grants = self.grant_repo.find_by_principal(principal_id).await?;
        for grant in grants {
            if !client_ids.contains(&grant.client_id) {
                client_ids.push(grant.client_id);
            }
        }
        Ok(())
    }

    /// Get accessible client IDs for a principal
    async fn get_accessible_client_ids(&self, principal: &Principal) -> Result<Vec<String>, PlatformError> {
        match principal.scope {
            UserScope::Anchor => {
                // Anchor users have access to all active clients
                let clients = self.client_repo.find_active().await?;
                Ok(clients.into_iter().map(|c| c.id).collect())
            }
            UserScope::Client => {
                // Client users have access to their home client + explicit grants
                let mut client_ids = Vec::new();
                if let Some(ref home_client) = principal.client_id {
                    client_ids.push(home_client.clone());
                }

                // Add non-expired explicit grants
                self.add_active_grants(&mut client_ids, &principal.id).await?;

                Ok(client_ids)
            }
            UserScope::Partner => {
                // Partner users have access via assigned clients + explicit grants
                let mut client_ids = principal.assigned_clients.clone();

                // Add non-expired explicit grants
                self.add_active_grants(&mut client_ids, &principal.id).await?;

                Ok(client_ids)
            }
        }
    }

    /// Check if principal can access a specific client
    async fn can_access_client(&self, principal: &Principal, client_id: &str) -> Result<bool, PlatformError> {
        if principal.scope == UserScope::Anchor {
            return Ok(true);
        }

        let accessible = self.get_accessible_client_ids(principal).await?;
        Ok(accessible.contains(&client_id.to_string()))
    }

    /// Resolve permissions for a set of roles
    async fn resolve_permissions(&self, role_codes: &[String]) -> Result<HashSet<String>, PlatformError> {
        if role_codes.is_empty() {
            return Ok(HashSet::new());
        }

        let roles = self.role_repo.find_by_codes(role_codes).await?;
        let mut permissions = HashSet::new();

        for role in roles {
            permissions.extend(role.permissions);
        }

        Ok(permissions)
    }
}

/// List accessible clients for the current user
#[utoipa::path(
    get,
    path = "/accessible",
    tag = "client-selection",
    responses(
        (status = 200, description = "List of accessible clients", body = AccessibleClientsResponse),
        (status = 401, description = "Not authenticated")
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_accessible_clients(
    State(state): State<ClientSelectionState>,
    auth: Authenticated,
) -> Result<Json<AccessibleClientsResponse>, PlatformError> {
    let principal = state.principal_repo.find_by_id(&auth.0.principal_id).await?
        .ok_or_else(|| PlatformError::not_found("Principal", &auth.0.principal_id))?;

    let global_access = principal.scope == UserScope::Anchor;

    // Get accessible client IDs
    let client_ids = state.get_accessible_client_ids(&principal).await?;

    // Load client details
    let mut clients = Vec::new();
    for id in &client_ids {
        if let Some(client) = state.client_repo.find_by_id(id).await? {
            // Only include active clients
            if client.status == ClientStatus::Active {
                clients.push(ClientInfo {
                    id: client.id,
                    name: client.name,
                    identifier: client.identifier,
                });
            }
        }
    }

    // Sort by name
    clients.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(Json(AccessibleClientsResponse {
        clients,
        current_client_id: principal.client_id.clone(),
        global_access,
    }))
}

/// Switch to a different client context
#[utoipa::path(
    post,
    path = "/switch",
    tag = "client-selection",
    request_body = SwitchClientRequest,
    responses(
        (status = 200, description = "Client switched, new token issued", body = SwitchClientResponse),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "Access denied to client"),
        (status = 404, description = "Client not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn switch_client(
    State(state): State<ClientSelectionState>,
    auth: Authenticated,
    Json(req): Json<SwitchClientRequest>,
) -> Result<Json<SwitchClientResponse>, PlatformError> {
    let principal = state.principal_repo.find_by_id(&auth.0.principal_id).await?
        .ok_or_else(|| PlatformError::not_found("Principal", &auth.0.principal_id))?;

    // Check if user can access the requested client
    if !state.can_access_client(&principal, &req.client_id).await? {
        return Err(PlatformError::forbidden(format!(
            "Access denied to client: {}",
            req.client_id
        )));
    }

    // Load the client
    let client = state.client_repo.find_by_id(&req.client_id).await?
        .ok_or_else(|| PlatformError::not_found("Client", &req.client_id))?;

    // Check client is active
    if client.status != ClientStatus::Active {
        return Err(PlatformError::forbidden(format!(
            "Client is not active: {}",
            client.name
        )));
    }

    // Generate new token with client context
    let token = state.auth_service.generate_access_token(&principal)?;

    // Get roles and permissions
    let role_codes: Vec<String> = principal.roles.iter().map(|r| r.role.clone()).collect();
    let permissions = state.resolve_permissions(&role_codes).await?;

    Ok(Json(SwitchClientResponse {
        token,
        client: ClientInfo {
            id: client.id,
            name: client.name,
            identifier: client.identifier,
        },
        roles: role_codes,
        permissions: permissions.into_iter().collect(),
    }))
}

/// Get current client context
#[utoipa::path(
    get,
    path = "/current",
    tag = "client-selection",
    responses(
        (status = 200, description = "Current client context", body = CurrentClientResponse),
        (status = 401, description = "Not authenticated")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_current_client(
    State(state): State<ClientSelectionState>,
    auth: Authenticated,
) -> Result<Json<CurrentClientResponse>, PlatformError> {
    // Check if user has a home client
    let principal = state.principal_repo.find_by_id(&auth.0.principal_id).await?
        .ok_or_else(|| PlatformError::not_found("Principal", &auth.0.principal_id))?;

    let client = if let Some(ref client_id) = principal.client_id {
        if let Some(c) = state.client_repo.find_by_id(client_id).await? {
            Some(ClientInfo {
                id: c.id,
                name: c.name,
                identifier: c.identifier,
            })
        } else {
            None
        }
    } else {
        None
    };

    Ok(Json(CurrentClientResponse {
        no_client_context: client.is_none(),
        client,
    }))
}

/// Create client selection router
pub fn client_selection_router(state: ClientSelectionState) -> Router {
    Router::new()
        .route("/accessible", get(list_accessible_clients))
        .route("/switch", post(switch_client))
        .route("/current", get(get_current_client))
        .with_state(state)
}
