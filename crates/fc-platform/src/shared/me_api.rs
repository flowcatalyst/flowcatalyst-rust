//! /api/me Routes — User self-service

use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use serde::Serialize;
use std::sync::Arc;
use utoipa::ToSchema;

use crate::application::client_config_repository::ApplicationClientConfigRepository;
use crate::application::repository::ApplicationRepository;
use crate::client::repository::ClientRepository;
use crate::principal::repository::PrincipalRepository;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MyClientResponse {
    pub id: String,
    pub name: String,
    pub identifier: String,
    pub status: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MyClientsListResponse {
    pub clients: Vec<MyClientResponse>,
    pub total: usize,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MyApplicationResponse {
    pub id: String,
    pub code: String,
    pub name: String,
    // Go `myApplicationResponse`: each optional member absent when unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo_mime_type: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MyApplicationsListResponse {
    pub applications: Vec<MyApplicationResponse>,
    pub total: usize,
    pub client_id: String,
}

#[derive(Clone)]
pub struct MeState {
    pub client_repo: Arc<ClientRepository>,
    pub application_repo: Arc<ApplicationRepository>,
    pub app_client_config_repo: Arc<ApplicationClientConfigRepository>,
    /// Used to resolve the caller's application-access list, which the
    /// JWT-derived `AuthContext` does not carry.
    pub principal_repo: Arc<PrincipalRepository>,
    /// Orders the caller's permissions as Go does (role by role).
    pub role_repo: Arc<crate::role::repository::RoleRepository>,
    /// Reads a bearer's own `clients` / `applications` claims for
    /// `/api/me`, which reports the credential's reach, not the row's.
    pub auth_service: Arc<crate::AuthService>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WhoamiResponse {
    pub principal_id: String,
    /// "USER" or "SERVICE".
    pub principal_type: String,
    /// "ANCHOR", "TENANT", "SERVICE", etc.
    pub scope: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    pub active: bool,
    /// Role codes assigned to this principal (resolved at request time).
    pub roles: Vec<String>,
    /// The caller's effective permissions, in Go's order.
    pub permissions: Vec<String>,
    /// Client IDs this principal can act inside: a bearer's `clients` claim
    /// as minted (`*`, or `id:identifier` pairs); for a session, the
    /// principal's home and granted clients.
    pub accessible_client_ids: Vec<String>,
    /// Application IDs this principal can act against (the credential's
    /// `applications`, codes stripped, `*` dropped).
    pub accessible_application_ids: Vec<String>,
    /// Whether the credential reaches every application; the list is then
    /// no restriction.
    pub all_applications: bool,
}

/// Go `ParseApplicationsClaim` (shared/auth/application_scope.go:23-41):
/// the ids of an `applications` claim (`id:code` → `id`) and whether it
/// holds the `*` sentinel.
fn parse_applications_claim(entries: &[String]) -> (Vec<String>, bool) {
    let mut all = false;
    let mut ids = Vec::with_capacity(entries.len());
    for entry in entries {
        if entry == "*" {
            all = true;
            continue;
        }
        let id = entry.split_once(':').map_or(entry.as_str(), |(id, _)| id);
        if !id.is_empty() {
            ids.push(id.to_string());
        }
    }
    (ids, all)
}

/// List clients accessible to the authenticated user
#[utoipa::path(
    get,
    path = "/clients",
    tag = "me",
    operation_id = "getApiMeClients",
    responses(
        (status = 200, description = "List of accessible clients", body = MyClientsListResponse)
    ),
    security(("bearer_auth" = []))
)]
async fn list_my_clients(
    State(state): State<MeState>,
    auth: Authenticated,
) -> Result<Json<MyClientsListResponse>, PlatformError> {
    let all_clients = state.client_repo.find_all().await?;

    let accessible: Vec<_> = all_clients
        .into_iter()
        .filter(|c| auth.0.is_anchor() || auth.0.can_access_client(&c.id))
        .map(|c| MyClientResponse {
            id: c.id,
            name: c.name,
            identifier: c.identifier,
            status: Some(c.status.as_str().to_string()),
            created_at: c.created_at.to_rfc3339(),
            updated_at: c.updated_at.to_rfc3339(),
        })
        .collect();
    let total = accessible.len();

    Ok(Json(MyClientsListResponse {
        clients: accessible,
        total,
    }))
}

/// Get a specific client by ID for the authenticated user
#[utoipa::path(
    get,
    path = "/clients/{clientId}",
    tag = "me",
    operation_id = "getApiMeClientsByClientId",
    params(
        ("clientId" = String, Path, description = "Client ID")
    ),
    responses(
        (status = 200, description = "Client found", body = MyClientResponse),
        (status = 403, description = "No access to this client"),
        (status = 404, description = "Client not found")
    ),
    security(("bearer_auth" = []))
)]
async fn get_my_client(
    State(state): State<MeState>,
    auth: Authenticated,
    Path(client_id): Path<String>,
) -> Result<Json<MyClientResponse>, PlatformError> {
    let client = state
        .client_repo
        .find_by_id(&client_id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Client", &client_id))?;

    if !auth.0.is_anchor() && !auth.0.can_access_client(&client.id) {
        return Err(PlatformError::forbidden("No access to this client"));
    }

    Ok(Json(MyClientResponse {
        id: client.id,
        name: client.name,
        identifier: client.identifier,
        status: Some(client.status.as_str().to_string()),
        created_at: client.created_at.to_rfc3339(),
        updated_at: client.updated_at.to_rfc3339(),
    }))
}

/// List applications enabled for a specific client
#[utoipa::path(
    get,
    path = "/clients/{clientId}/applications",
    tag = "me",
    operation_id = "getApiMeClientsByClientIdApplications",
    params(
        ("clientId" = String, Path, description = "Client ID")
    ),
    responses(
        (status = 200, description = "Client applications", body = MyApplicationsListResponse),
        (status = 403, description = "No access to this client"),
        (status = 404, description = "Client not found")
    ),
    security(("bearer_auth" = []))
)]
async fn list_my_client_applications(
    State(state): State<MeState>,
    auth: Authenticated,
    Path(client_id): Path<String>,
) -> Result<Json<MyApplicationsListResponse>, PlatformError> {
    // Check access
    let _client = state
        .client_repo
        .find_by_id(&client_id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Client", &client_id))?;

    if !auth.0.is_anchor() && !auth.0.can_access_client(&client_id) {
        return Err(PlatformError::forbidden("No access to this client"));
    }

    // Get enabled app configs for this client
    let configs = state
        .app_client_config_repo
        .find_by_client(&client_id)
        .await?;
    let enabled_app_ids: Vec<&str> = configs
        .iter()
        .filter(|c| c.enabled)
        .map(|c| c.application_id.as_str())
        .collect();

    // Fetch applications
    let all_apps = state.application_repo.find_all().await?;
    let apps: Vec<MyApplicationResponse> = all_apps
        .into_iter()
        .filter(|a| enabled_app_ids.contains(&a.id.as_str()))
        .map(|a| MyApplicationResponse {
            id: a.id,
            code: a.code,
            name: a.name,
            description: a.description,
            icon_url: a.icon_url,
            base_url: a.default_base_url,
            website: a.website,
            logo_mime_type: a.logo_mime_type,
        })
        .collect();
    let total = apps.len();

    Ok(Json(MyApplicationsListResponse {
        applications: apps,
        total,
        client_id,
    }))
}

/// Return the calling principal's id, type, scope, roles, and accessible
/// clients/applications. The canonical "who am I" lookup used by agents,
/// SDKs, and the UI.
#[utoipa::path(
    get,
    path = "",
    tag = "me",
    operation_id = "getApiMeWhoami",
    responses(
        (status = 200, description = "Calling principal", body = WhoamiResponse)
    ),
    security(("bearer_auth" = []))
)]
async fn whoami(
    State(state): State<MeState>,
    headers: axum::http::HeaderMap,
    auth: Authenticated,
) -> Result<Json<WhoamiResponse>, PlatformError> {
    let ctx = &auth.0;
    // Go `whoami` (shared/me/me.go:57-110): roles, permissions, clients and
    // applications are the CREDENTIAL's, so a token narrowed to one
    // application reports that application only; the row supplies the
    // identity fields. A session is reloaded from the row on every request
    // (Go `BuildClaims`), so its reach is the row's.
    let principal = state.principal_repo.find_by_id(&ctx.principal_id).await?;
    let bearer_claims = match ctx.credential {
        crate::shared::authorization_service::Credential::BearerToken => headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(crate::auth::auth_service::extract_bearer_token)
            .and_then(|token| state.auth_service.validate_token(token).ok()),
        crate::shared::authorization_service::Credential::SessionCookie => None,
    };
    let (accessible_client_ids, (accessible_application_ids, all_applications)) =
        match (&bearer_claims, &principal) {
            (Some(claims), _) => {
                let (ids, all) = parse_applications_claim(&claims.applications);
                (
                    claims.clients.clone(),
                    (ids, all || claims.all_applications),
                )
            }
            (None, Some(p)) => {
                let mut clients = p.assigned_clients.clone();
                if let Some(home) = p.client_id.as_ref().filter(|c| !c.is_empty()) {
                    clients.push(home.clone());
                }
                let (ids, all) =
                    parse_applications_claim(&crate::auth::auth_service::applications_claim(p));
                (clients, (ids, all || p.all_applications))
            }
            (None, None) => (Vec::new(), (Vec::new(), false)),
        };
    // Go's order (its `flattenPermissions`: role by role, each role's
    // permissions sorted); anything the roles do not explain follows,
    // sorted.
    let flattened = state
        .role_repo
        .flatten_permissions(&ctx.roles)
        .await
        .unwrap_or_default();
    let mut permissions: Vec<String> = flattened
        .into_iter()
        .filter(|p| ctx.permissions.contains(p))
        .collect();
    let mut rest: Vec<String> = ctx
        .permissions
        .iter()
        .filter(|p| !permissions.contains(p))
        .cloned()
        .collect();
    rest.sort();
    permissions.extend(rest);

    Ok(Json(WhoamiResponse {
        principal_id: ctx.principal_id.clone(),
        principal_type: principal
            .as_ref()
            .map_or(ctx.principal_type.as_str(), |p| p.principal_type.as_str())
            .to_string(),
        scope: ctx.scope.as_str().to_string(),
        name: principal
            .as_ref()
            .map_or_else(|| ctx.name.clone(), |p| p.name.clone()),
        email: ctx.email.clone().filter(|e| !e.is_empty()),
        active: true,
        roles: ctx.roles.clone(),
        permissions,
        accessible_client_ids,
        accessible_application_ids,
        all_applications,
    }))
}

/// List applications the calling principal has access to. Anchors see
/// every application; other principals see the apps explicitly granted
/// to them via `iam_principal_application_access`.
#[utoipa::path(
    get,
    path = "/applications",
    tag = "me",
    operation_id = "getApiMeApplications",
    responses(
        (status = 200, description = "Accessible applications", body = MyApplicationsListResponse)
    ),
    security(("bearer_auth" = []))
)]
async fn list_my_applications(
    State(state): State<MeState>,
    auth: Authenticated,
) -> Result<Json<MyApplicationsListResponse>, PlatformError> {
    let all_apps = state.application_repo.find_all().await?;
    // Same resolution as whoami: pull the hydrated app-access list from
    // the principal row. Anchors see everything regardless.
    let principal = state
        .principal_repo
        .find_by_id(&auth.0.principal_id)
        .await?;
    let accessible_ids: std::collections::HashSet<String> = principal
        .map(|p| p.accessible_application_ids.into_iter().collect())
        .unwrap_or_default();

    let apps: Vec<MyApplicationResponse> = all_apps
        .into_iter()
        .filter(|a| auth.0.is_anchor() || accessible_ids.contains(&a.id))
        .map(|a| MyApplicationResponse {
            id: a.id,
            code: a.code,
            name: a.name,
            description: a.description,
            icon_url: a.icon_url,
            base_url: a.default_base_url,
            website: a.website,
            logo_mime_type: a.logo_mime_type,
        })
        .collect();
    let total = apps.len();

    Ok(Json(MyApplicationsListResponse {
        applications: apps,
        total,
        // Not scoped to a single client; agents/service accounts span
        // their assigned clients. Field kept for response-shape
        // compatibility with the per-client variant.
        client_id: String::new(),
    }))
}

pub fn me_router(state: MeState) -> Router {
    Router::new()
        .route("/", get(whoami))
        .route("/applications", get(list_my_applications))
        .route("/clients", get(list_my_clients))
        .route("/clients/{clientId}", get(get_my_client))
        .route(
            "/clients/{clientId}/applications",
            get(list_my_client_applications),
        )
        .with_state(state)
}
