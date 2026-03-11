//! Connections Admin API

use axum::{
    extract::{State, Path, Query},
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use utoipa::ToSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::entity::{Connection, ConnectionStatus};
use super::repository::ConnectionRepository;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateConnectionRequest {
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub endpoint: String,
    pub external_id: Option<String>,
    pub service_account_id: String,
    pub client_id: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateConnectionRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub endpoint: Option<String>,
    pub external_id: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionResponse {
    pub id: String,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub endpoint: String,
    pub external_id: Option<String>,
    pub status: String,
    pub service_account_id: String,
    pub client_id: Option<String>,
    pub client_identifier: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<Connection> for ConnectionResponse {
    fn from(c: Connection) -> Self {
        Self {
            id: c.id,
            code: c.code,
            name: c.name,
            description: c.description,
            endpoint: c.endpoint,
            external_id: c.external_id,
            status: c.status.as_str().to_string(),
            service_account_id: c.service_account_id,
            client_id: c.client_id,
            client_identifier: c.client_identifier,
            created_at: c.created_at.to_rfc3339(),
            updated_at: c.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionsListResponse {
    pub connections: Vec<ConnectionResponse>,
    pub total: usize,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionsQuery {
    pub client_id: Option<String>,
    pub status: Option<String>,
    pub service_account_id: Option<String>,
}

#[derive(Clone)]
pub struct ConnectionsState {
    pub connection_repo: Arc<ConnectionRepository>,
}

/// Create a new connection
#[utoipa::path(
    post,
    path = "",
    tag = "connections",
    operation_id = "postApiAdminConnections",
    request_body = CreateConnectionRequest,
    responses(
        (status = 201, description = "Connection created", body = ConnectionResponse),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Duplicate code")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_connection(
    State(state): State<ConnectionsState>,
    _auth: Authenticated,
    Json(req): Json<CreateConnectionRequest>,
) -> Result<(axum::http::StatusCode, Json<ConnectionResponse>), PlatformError> {
    if let Some(existing) = state.connection_repo.find_by_code_and_client(&req.code, req.client_id.as_deref()).await? {
        return Err(PlatformError::duplicate("Connection", "code", &existing.code));
    }

    let mut conn = Connection::new(&req.code, &req.name, &req.endpoint, &req.service_account_id);
    if let Some(desc) = req.description { conn = conn.with_description(desc); }
    if let Some(ext) = req.external_id { conn = conn.with_external_id(ext); }
    if let Some(cid) = req.client_id { conn = conn.with_client_id(cid); }

    state.connection_repo.insert(&conn).await?;
    Ok((axum::http::StatusCode::CREATED, Json(conn.into())))
}

/// List connections
#[utoipa::path(
    get,
    path = "",
    tag = "connections",
    operation_id = "getApiAdminConnections",
    params(
        ("clientId" = Option<String>, Query, description = "Filter by client ID"),
        ("status" = Option<String>, Query, description = "Filter by status"),
        ("serviceAccountId" = Option<String>, Query, description = "Filter by service account ID")
    ),
    responses(
        (status = 200, description = "List of connections", body = ConnectionsListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_connections(
    State(state): State<ConnectionsState>,
    _auth: Authenticated,
    Query(query): Query<ConnectionsQuery>,
) -> Result<Json<ConnectionsListResponse>, PlatformError> {
    let connections = state.connection_repo.find_with_filters(
        query.client_id.as_deref(),
        query.status.as_deref(),
        query.service_account_id.as_deref(),
    ).await?;
    let total = connections.len();
    Ok(Json(ConnectionsListResponse {
        connections: connections.into_iter().map(|c| c.into()).collect(),
        total,
    }))
}

/// Get connection by ID
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "connections",
    operation_id = "getApiAdminConnectionsById",
    params(
        ("id" = String, Path, description = "Connection ID")
    ),
    responses(
        (status = 200, description = "Connection found", body = ConnectionResponse),
        (status = 404, description = "Connection not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_connection(
    State(state): State<ConnectionsState>,
    _auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<ConnectionResponse>, PlatformError> {
    let conn = state.connection_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("Connection", &id))?;
    Ok(Json(conn.into()))
}

/// Update connection by ID
#[utoipa::path(
    put,
    path = "/{id}",
    tag = "connections",
    operation_id = "putApiAdminConnectionsById",
    params(
        ("id" = String, Path, description = "Connection ID")
    ),
    request_body = UpdateConnectionRequest,
    responses(
        (status = 200, description = "Connection updated", body = ConnectionResponse),
        (status = 404, description = "Connection not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_connection(
    State(state): State<ConnectionsState>,
    _auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateConnectionRequest>,
) -> Result<Json<ConnectionResponse>, PlatformError> {
    let mut conn = state.connection_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("Connection", &id))?;

    if let Some(name) = req.name { conn.name = name; }
    if let Some(desc) = req.description { conn.description = Some(desc); }
    if let Some(ep) = req.endpoint { conn.endpoint = ep; }
    if let Some(ext) = req.external_id { conn.external_id = Some(ext); }
    if let Some(status) = req.status {
        conn.status = ConnectionStatus::from_str(&status);
    }
    conn.updated_at = chrono::Utc::now();

    state.connection_repo.update(&conn).await?;
    Ok(Json(conn.into()))
}

/// Delete connection by ID
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "connections",
    operation_id = "deleteApiAdminConnectionsById",
    params(
        ("id" = String, Path, description = "Connection ID")
    ),
    responses(
        (status = 204, description = "Connection deleted"),
        (status = 404, description = "Connection not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_connection(
    State(state): State<ConnectionsState>,
    _auth: Authenticated,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, PlatformError> {
    let _ = state.connection_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("Connection", &id))?;
    state.connection_repo.delete(&id).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Pause a connection
#[utoipa::path(
    post,
    path = "/{id}/pause",
    tag = "connections",
    operation_id = "postApiAdminConnectionsByIdPause",
    params(
        ("id" = String, Path, description = "Connection ID")
    ),
    responses(
        (status = 200, description = "Connection paused", body = ConnectionResponse),
        (status = 404, description = "Connection not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn pause_connection(
    State(state): State<ConnectionsState>,
    _auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<ConnectionResponse>, PlatformError> {
    let mut conn = state.connection_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("Connection", &id))?;
    conn.pause();
    state.connection_repo.update(&conn).await?;
    Ok(Json(conn.into()))
}

/// Activate a connection
#[utoipa::path(
    post,
    path = "/{id}/activate",
    tag = "connections",
    operation_id = "postApiAdminConnectionsByIdActivate",
    params(
        ("id" = String, Path, description = "Connection ID")
    ),
    responses(
        (status = 200, description = "Connection activated", body = ConnectionResponse),
        (status = 404, description = "Connection not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn activate_connection(
    State(state): State<ConnectionsState>,
    _auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<ConnectionResponse>, PlatformError> {
    let mut conn = state.connection_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("Connection", &id))?;
    conn.activate();
    state.connection_repo.update(&conn).await?;
    Ok(Json(conn.into()))
}

/// Create connections router
pub fn connections_router(state: ConnectionsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(create_connection, list_connections))
        .routes(routes!(get_connection, update_connection, delete_connection))
        .routes(routes!(pause_connection))
        .routes(routes!(activate_connection))
        .with_state(state)
}
