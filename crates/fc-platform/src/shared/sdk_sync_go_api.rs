//! SDK sync routes Go serves that Rust lacked (Go `sdksync/api.go:100,108`):
//!
//! - `POST /api/applications/{appCode}/connections/sync` (`?removeUnlisted=`):
//!   `{clientId?, connections: [{code, name, description?, externalId?}]}`
//! - `POST /api/processes/sync` (`?removeUnlisted=`): the app-scoped
//!   processes sync with `applicationCode` in the body
//!
//! Both answer Go's `SyncResultResponse`.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::Deserialize;
use std::sync::Arc;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::connection::operations::sync::{
    SyncConnectionInput, SyncConnectionsCommand, SyncConnectionsUseCase,
};
use crate::process::operations::{SyncProcessInput, SyncProcessesCommand, SyncProcessesUseCase};
use crate::shared::authorization_service::{checks, ApplicationAccessService, AuthContext};
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::shared::sdk_sync_api::{SyncProcessInputRequest, SyncQuery, SyncResultResponse};
use crate::usecase::{ExecutionContext, PgUnitOfWork, UseCase};
use crate::ClientRepository;

#[derive(Clone)]
pub struct SdkSyncGoState {
    pub app_access: Arc<ApplicationAccessService>,
    pub client_repo: Arc<ClientRepository>,
    pub sync_connections_use_case: Arc<SyncConnectionsUseCase<PgUnitOfWork>>,
    pub sync_processes_use_case: Arc<SyncProcessesUseCase<PgUnitOfWork>>,
}

/// Go `syncConnectionInputRequest`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncConnectionInputRequest {
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub external_id: Option<String>,
}

/// Go `syncConnectionsRequest`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncConnectionsRequest {
    /// The client scope, by id or identifier; absent or blank is shared.
    pub client_id: Option<String>,
    pub connections: Vec<SyncConnectionInputRequest>,
}

/// Go `syncProcessesByBodyRequest`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncProcessesByBodyRequest {
    pub application_code: String,
    pub processes: Vec<SyncProcessInputRequest>,
}

/// Go `CanSyncConnections`: any of the connection sync/manage permissions
/// or the application-service connection writes.
fn can_sync_connections(ctx: &AuthContext) -> Result<(), PlatformError> {
    let perms = [
        crate::permissions::admin::CONNECTION_SYNC,
        crate::permissions::admin::CONNECTION_MANAGE,
        crate::permissions::application_service::CONNECTION_CREATE,
        crate::permissions::application_service::CONNECTION_UPDATE,
        crate::permissions::application_service::CONNECTION_DELETE,
    ];
    if ctx.has_any_permission(&perms) {
        Ok(())
    } else {
        Err(PlatformError::forbidden_code(
            "PERMISSION_REQUIRED",
            format!("one of: {}", perms.join(", ")),
        ))
    }
}

/// Sync an application's connections (Go `syncConnections`).
#[utoipa::path(
    post,
    path = "/api/applications/{appCode}/connections/sync",
    tag = "sdk-sync",
    operation_id = "syncConnections",
    params(
        ("appCode" = String, Path, description = "Application code"),
        ("removeUnlisted" = Option<bool>, Query, description = "Delete the application's API connections not listed")
    ),
    request_body = SyncConnectionsRequest,
    responses(
        (status = 200, description = "Synced", body = SyncResultResponse),
        (status = 400, description = "Invalid input, or no service account"),
        (status = 404, description = "Unknown application or client"),
        (status = 409, description = "A connection to remove is still in use")
    ),
    security(("bearer_auth" = []))
)]
pub async fn sync_connections(
    State(state): State<SdkSyncGoState>,
    auth: Authenticated,
    Path(app_code): Path<String>,
    Query(query): Query<SyncQuery>,
    Json(req): Json<SyncConnectionsRequest>,
) -> Result<Json<SyncResultResponse>, PlatformError> {
    can_sync_connections(&auth.0)?;
    let app = state
        .app_access
        .require_application_access(&auth.0, &app_code)
        .await?;
    // Go `resolveClientRef`: the id first, then the identifier lower-cased.
    let client_id = match req.client_id.as_deref().map(str::trim) {
        Some(r) if !r.is_empty() => {
            let client = match state.client_repo.find_by_id(r).await? {
                Some(c) => Some(c),
                None => {
                    state
                        .client_repo
                        .find_by_identifier(&r.to_lowercase())
                        .await?
                }
            };
            let client = client.ok_or_else(|| PlatformError::not_found_code("Client", r))?;
            if !auth.0.can_access_client(&client.id) {
                return Err(PlatformError::forbidden(format!(
                    "No access to client: {}",
                    client.id
                )));
            }
            Some(client.id)
        }
        _ => None,
    };
    let event = state
        .sync_connections_use_case
        .run(
            SyncConnectionsCommand {
                application_id: app.id,
                application_code: app.code,
                client_id,
                connections: req
                    .connections
                    .into_iter()
                    .map(|c| SyncConnectionInput {
                        code: c.code,
                        name: c.name,
                        description: c.description,
                        external_id: c.external_id,
                    })
                    .collect(),
                remove_unlisted: query.remove_unlisted,
            },
            ExecutionContext::from_auth(&auth.0),
        )
        .await
        .into_result()?;
    Ok(Json(SyncResultResponse {
        application_code: event.application_code,
        created: event.created,
        updated: event.updated,
        deleted: event.deleted,
        synced_codes: event.synced_codes,
        password_hash_ignored: Vec::new(),
    }))
}

/// Sync processes naming the application in the body (Go `syncProcessesByBody`).
#[utoipa::path(
    post,
    path = "/api/processes/sync",
    tag = "sdk-sync",
    operation_id = "syncProcessesByBody",
    params(("removeUnlisted" = Option<bool>, Query, description = "Delete the application's API processes not listed")),
    request_body = SyncProcessesByBodyRequest,
    responses(
        (status = 200, description = "Synced", body = SyncResultResponse),
        (status = 404, description = "Unknown application")
    ),
    security(("bearer_auth" = []))
)]
pub async fn sync_processes_by_body(
    State(state): State<SdkSyncGoState>,
    auth: Authenticated,
    Query(query): Query<SyncQuery>,
    Json(req): Json<SyncProcessesByBodyRequest>,
) -> Result<Json<SyncResultResponse>, PlatformError> {
    checks::can_sync_processes(&auth.0)?;
    let app = state
        .app_access
        .require_application_access(&auth.0, &req.application_code)
        .await?;
    let event = state
        .sync_processes_use_case
        .run(
            SyncProcessesCommand {
                application_code: app.code,
                processes: req
                    .processes
                    .into_iter()
                    .map(|p| SyncProcessInput {
                        code: p.code,
                        name: p.name,
                        description: p.description,
                        body: p.body,
                        diagram_type: p.diagram_type,
                        tags: p.tags,
                    })
                    .collect(),
                remove_unlisted: query.remove_unlisted,
            },
            ExecutionContext::from_auth(&auth.0),
        )
        .await
        .into_result()?;
    Ok(Json(SyncResultResponse {
        application_code: event.application_code,
        created: event.created,
        updated: event.updated,
        deleted: event.deleted,
        synced_codes: event.synced_codes,
        password_hash_ignored: Vec::new(),
    }))
}

/// Full-path router; merged at the root.
pub fn sdk_sync_go_router(state: SdkSyncGoState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(sync_connections))
        .routes(routes!(sync_processes_by_body))
        .with_state(state)
}
