//! Connections Admin API

use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::entity::{Connection, ConnectionStatus};
use super::repository::ConnectionRepository;
use crate::shared::error::{NotFoundExt, PlatformError};
use crate::shared::middleware::Authenticated;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateConnectionRequest {
    pub code: String,
    /// The owning application (omitted: shared, usable from any application)
    #[serde(default)]
    pub application_code: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub external_id: Option<String>,
    pub service_account_id: String,
    pub client_id: Option<String>,
}

/// Go `UpdateConnectionRequest`: the name is required; description and
/// externalId are replaced as sent; applicationCode and status are
/// set-if-provided.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateConnectionRequest {
    /// Required, as Go's (a missing one is a 400 VALIDATION).
    pub name: String,
    #[serde(default)]
    pub application_code: Option<String>,
    pub description: Option<String>,
    pub external_id: Option<String>,
    pub status: Option<String>,
}

/// Go `ConnectionResponse`: optional members are omitted when unset.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionResponse {
    pub id: String,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_code: Option<String>,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    pub status: String,
    pub service_account_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_identifier: Option<String>,
    pub source: String,
    pub created_at: String,
    pub updated_at: String,
}

impl From<Connection> for ConnectionResponse {
    fn from(c: Connection) -> Self {
        Self {
            id: c.id,
            code: c.code,
            application_code: c.application_code,
            name: c.name,
            description: c.description,
            external_id: c.external_id,
            status: c.status.as_str().to_string(),
            service_account_id: c.service_account_id,
            client_id: c.client_id,
            client_identifier: c.client_identifier,
            source: c.source,
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
    /// Resolves an `applicationCode` within the caller's application scope.
    pub app_access: Arc<crate::shared::authorization_service::ApplicationAccessService>,
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
        (status = 201, description = "Connection created", body = ConnectionResponse),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Duplicate code")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_connection(
    State(state): State<ConnectionsState>,
    auth: Authenticated,
    Json(req): Json<CreateConnectionRequest>,
) -> Result<(axum::http::StatusCode, Json<ConnectionResponse>), PlatformError> {
    use crate::connection::operations::CreateConnectionCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::checks::can_create_connections(&auth.0)?;

    // An application the caller cannot act on is the same 404 as a
    // missing one (owner ruling).
    let application_code = match req.application_code.as_deref().map(str::trim) {
        Some(code) if !code.is_empty() => Some(
            state
                .app_access
                .require_application_access(&auth.0, code)
                .await?
                .code,
        ),
        _ => None,
    };

    // The use case checks the input (400), then the caller's reach into
    // the requested client (403 SCOPE_FORBIDDEN), as Go.
    let cmd = CreateConnectionCommand {
        code: req.code,
        name: req.name,
        description: req.description,
        service_account_id: req.service_account_id,
        external_id: req.external_id,
        client_id: req.client_id,
        application_code,
        caller: Some(auth.0.clone()),
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    let event = state.create_use_case.run(cmd, ctx).await.into_result()?;
    // Go answers with the stored connection, not `{id}`: the SPA puts it
    // straight into a select.
    let conn = state
        .connection_repo
        .find_by_id(&event.connection_id)
        .await?
        .or_not_found("Connection", &event.connection_id)?;
    Ok((axum::http::StatusCode::CREATED, Json(conn.into())))
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
    auth: Authenticated,
    Query(query): Query<ConnectionsQuery>,
) -> Result<Json<ConnectionsListResponse>, PlatformError> {
    crate::checks::can_read_connections(&auth.0)?;

    let connections = state
        .connection_repo
        .find_with_filters(
            query.client_id.as_deref(),
            crate::shared::enum_str::parse_opt(query.status.as_deref())?,
            query.service_account_id.as_deref(),
        )
        .await?;
    // Go `FilterClientScoped`: platform connections to every holder of the
    // read permission, a client's only to callers reaching that client.
    let connections: Vec<_> = connections
        .into_iter()
        .filter(|c| super::access::is_visible(&auth.0, c))
        .collect();
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
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<ConnectionResponse>, PlatformError> {
    crate::checks::can_read_connections(&auth.0)?;

    let conn = state
        .connection_repo
        .find_by_id(&id)
        .await?
        .or_not_found("Connection", &id)?;
    super::access::ensure_visible(&auth.0, &conn)?;
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

    crate::checks::can_update_connections(&auth.0)?;

    let application_code = match req.application_code.as_deref().map(str::trim) {
        Some(code) if !code.is_empty() => Some(
            state
                .app_access
                .require_application_access(&auth.0, code)
                .await?
                .code,
        ),
        _ => None,
    };
    let status = match req.status.as_deref().map(str::trim) {
        None => None,
        Some("ACTIVE") => Some(ConnectionStatus::Active),
        Some("PAUSED") => Some(ConnectionStatus::Paused),
        Some(_) => {
            return Err(PlatformError::bad_request_code(
                "INVALID_STATUS",
                "status must be ACTIVE or PAUSED",
            ))
        }
    };
    // The use case validates, loads (404) and checks the caller's scope on
    // the row (403 SCOPE_FORBIDDEN).
    let cmd = UpdateConnectionCommand {
        connection_id: id,
        name: Some(req.name),
        description: req.description,
        external_id: req.external_id,
        status,
        service_account_id: None,
        application_code,
        replace_details: true,
        caller: Some(auth.0.clone()),
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

    crate::checks::can_delete_connections(&auth.0)?;

    // Go: 404 for a missing connection, then the caller's scope on it.
    let conn = state
        .connection_repo
        .find_by_id(&id)
        .await?
        .or_not_found("Connection", &id)?;
    crate::shared::caller_reach::require_scope_access(&auth.0, conn.client_id.as_deref())?;

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

    crate::checks::can_update_connections(&auth.0)?;

    // Unconditional (a repeat is a no-op write), with Go's per-row scope.
    let cmd = UpdateConnectionCommand {
        connection_id: id.clone(),
        name: None,
        description: None,
        external_id: None,
        status: Some(ConnectionStatus::Paused),
        service_account_id: None,
        application_code: None,
        replace_details: false,
        caller: Some(auth.0.clone()),
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

    crate::checks::can_update_connections(&auth.0)?;

    // Unconditional (a repeat is a no-op write), with Go's per-row scope.
    let cmd = UpdateConnectionCommand {
        connection_id: id.clone(),
        name: None,
        description: None,
        external_id: None,
        status: Some(ConnectionStatus::Active),
        service_account_id: None,
        application_code: None,
        replace_details: false,
        caller: Some(auth.0.clone()),
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
