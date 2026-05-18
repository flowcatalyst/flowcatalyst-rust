//! Connections Admin API

use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::entity::Connection;
use super::repository::ConnectionRepository;
use crate::shared::error::{NotFoundExt, PlatformError};
use crate::shared::middleware::Authenticated;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateConnectionRequest {
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub external_id: Option<String>,
    pub service_account_id: String,
    pub client_id: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateConnectionRequest {
    pub name: Option<String>,
    pub description: Option<String>,
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
    pub create_use_case:
        Arc<crate::connection::operations::CreateConnectionUseCase<crate::usecase::PgUnitOfWork>>,
    pub update_use_case:
        Arc<crate::connection::operations::UpdateConnectionUseCase<crate::usecase::PgUnitOfWork>>,
    pub delete_use_case:
        Arc<crate::connection::operations::DeleteConnectionUseCase<crate::usecase::PgUnitOfWork>>,
}

/// Create a new connection
#[utoipa::path(
    post,
    path = "",
    tag = "connections",
    operation_id = "postApiConnections",
    request_body = CreateConnectionRequest,
    responses(
        (status = 201, description = "Connection created", body = crate::shared::api_common::CreatedResponse),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Duplicate code")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_connection(
    State(state): State<ConnectionsState>,
    auth: Authenticated,
    Json(req): Json<CreateConnectionRequest>,
) -> Result<
    (
        axum::http::StatusCode,
        Json<crate::shared::api_common::CreatedResponse>,
    ),
    PlatformError,
> {
    use crate::connection::operations::CreateConnectionCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::require_anchor(&auth.0)?;

    let cmd = CreateConnectionCommand {
        code: req.code,
        name: req.name,
        description: req.description,
        service_account_id: req.service_account_id,
        external_id: req.external_id,
        client_id: req.client_id,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    let event = state.create_use_case.run(cmd, ctx).await.into_result()?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(crate::shared::api_common::CreatedResponse::new(
            event.connection_id,
        )),
    ))
}

/// List connections
#[utoipa::path(
    get,
    path = "",
    tag = "connections",
    operation_id = "getApiConnections",
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
    let connections = state
        .connection_repo
        .find_with_filters(
            query.client_id.as_deref(),
            query.status.as_deref(),
            query.service_account_id.as_deref(),
        )
        .await?;
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
    operation_id = "getApiConnectionsById",
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
    let conn = state
        .connection_repo
        .find_by_id(&id)
        .await?
        .or_not_found("Connection", &id)?;
    Ok(Json(conn.into()))
}

/// Update connection by ID
#[utoipa::path(
    put,
    path = "/{id}",
    tag = "connections",
    operation_id = "putApiConnectionsById",
    params(
        ("id" = String, Path, description = "Connection ID")
    ),
    request_body = UpdateConnectionRequest,
    responses(
        (status = 204, description = "Connection updated"),
        (status = 404, description = "Connection not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_connection(
    State(state): State<ConnectionsState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateConnectionRequest>,
) -> Result<axum::http::StatusCode, PlatformError> {
    use crate::connection::operations::UpdateConnectionCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::require_anchor(&auth.0)?;

    let cmd = UpdateConnectionCommand {
        connection_id: id,
        name: req.name,
        description: req.description,
        external_id: req.external_id,
        status: req.status,
        service_account_id: None,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.update_use_case.run(cmd, ctx).await.into_result()?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Delete connection by ID
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "connections",
    operation_id = "deleteApiConnectionsById",
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
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode, PlatformError> {
    use crate::connection::operations::DeleteConnectionCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::require_anchor(&auth.0)?;

    let cmd = DeleteConnectionCommand { connection_id: id };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.delete_use_case.run(cmd, ctx).await.into_result()?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Pause a connection
#[utoipa::path(
    post,
    path = "/{id}/pause",
    tag = "connections",
    operation_id = "postApiConnectionsByIdPause",
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
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<ConnectionResponse>, PlatformError> {
    use crate::connection::operations::UpdateConnectionCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::require_anchor(&auth.0)?;

    let cmd = UpdateConnectionCommand {
        connection_id: id.clone(),
        name: None,
        description: None,
        external_id: None,
        status: Some("PAUSED".to_string()),
        service_account_id: None,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.update_use_case.run(cmd, ctx).await.into_result()?;
    let conn = state
        .connection_repo
        .find_by_id(&id)
        .await?
        .or_not_found("Connection", &id)?;
    Ok(Json(conn.into()))
}

/// Activate a connection
#[utoipa::path(
    post,
    path = "/{id}/activate",
    tag = "connections",
    operation_id = "postApiConnectionsByIdActivate",
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
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<ConnectionResponse>, PlatformError> {
    use crate::connection::operations::UpdateConnectionCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::require_anchor(&auth.0)?;

    let cmd = UpdateConnectionCommand {
        connection_id: id.clone(),
        name: None,
        description: None,
        external_id: None,
        status: Some("ACTIVE".to_string()),
        service_account_id: None,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.update_use_case.run(cmd, ctx).await.into_result()?;
    let conn = state
        .connection_repo
        .find_by_id(&id)
        .await?
        .or_not_found("Connection", &id)?;
    Ok(Json(conn.into()))
}

/// Create connections router
pub fn connections_router(state: ConnectionsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(create_connection, list_connections))
        .routes(routes!(
            get_connection,
            update_connection,
            delete_connection
        ))
        .routes(routes!(pause_connection))
        .routes(routes!(activate_connection))
        .with_state(state)
}
