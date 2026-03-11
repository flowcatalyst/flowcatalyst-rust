//! /api/me Routes — User self-service

use axum::{
    routing::get,
    extract::{State, Path},
    Json, Router,
};
use utoipa::ToSchema;
use serde::Serialize;
use std::sync::Arc;

use crate::client::repository::ClientRepository;
use crate::application::repository::ApplicationRepository;
use crate::application::client_config_repository::ApplicationClientConfigRepository;
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
    pub description: Option<String>,
    pub icon_url: Option<String>,
    pub base_url: Option<String>,
    pub website: Option<String>,
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

    let accessible: Vec<_> = all_clients.into_iter()
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

    Ok(Json(MyClientsListResponse { clients: accessible, total }))
}

/// Get a specific client by ID for the authenticated user
#[utoipa::path(
    get,
    path = "/clients/{client_id}",
    tag = "me",
    operation_id = "getApiMeClientsByClientId",
    params(
        ("client_id" = String, Path, description = "Client ID")
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
    let client = state.client_repo.find_by_id(&client_id).await?
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
    path = "/clients/{client_id}/applications",
    tag = "me",
    operation_id = "getApiMeClientsByClientIdApplications",
    params(
        ("client_id" = String, Path, description = "Client ID")
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
    let _client = state.client_repo.find_by_id(&client_id).await?
        .ok_or_else(|| PlatformError::not_found("Client", &client_id))?;

    if !auth.0.is_anchor() && !auth.0.can_access_client(&client_id) {
        return Err(PlatformError::forbidden("No access to this client"));
    }

    // Get enabled app configs for this client
    let configs = state.app_client_config_repo.find_by_client(&client_id).await?;
    let enabled_app_ids: Vec<&str> = configs.iter()
        .filter(|c| c.enabled)
        .map(|c| c.application_id.as_str())
        .collect();

    // Fetch applications
    let all_apps = state.application_repo.find_all().await?;
    let apps: Vec<MyApplicationResponse> = all_apps.into_iter()
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

pub fn me_router(state: MeState) -> Router {
    Router::new()
        .route("/clients", get(list_my_clients))
        .route("/clients/{client_id}", get(get_my_client))
        .route("/clients/{client_id}/applications", get(list_my_client_applications))
        .with_state(state)
}
