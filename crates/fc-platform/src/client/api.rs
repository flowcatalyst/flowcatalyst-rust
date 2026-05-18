//! Clients Admin API
//!
//! REST endpoints for client management.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::entity::Client;
use super::repository::ClientRepository;
use crate::shared::api_common::PaginationParams;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;

/// Create client request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateClientRequest {
    /// Unique identifier/slug (URL-safe)
    pub identifier: String,

    /// Human-readable name
    pub name: String,
}

/// Update client request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateClientRequest {
    /// Human-readable name
    pub name: Option<String>,
}

/// Status change request (for suspend/deactivate)
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatusChangeRequest {
    /// Reason for the status change
    pub reason: String,
}

/// Status change response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatusChangeResponse {
    pub message: String,
}

/// Client response DTO (matches Java ClientDto)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientResponse {
    pub id: String,
    pub name: String,
    pub identifier: String,
    pub status: String,
    pub status_reason: Option<String>,
    pub status_changed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<Client> for ClientResponse {
    fn from(c: Client) -> Self {
        Self {
            id: c.id,
            name: c.name,
            identifier: c.identifier,
            status: format!("{:?}", c.status).to_uppercase(),
            status_reason: c.status_reason,
            status_changed_at: c.status_changed_at.map(|t| t.to_rfc3339()),
            created_at: c.created_at.to_rfc3339(),
            updated_at: c.updated_at.to_rfc3339(),
        }
    }
}

/// Client list response (matches Java ClientListResponse)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientListResponse {
    pub clients: Vec<ClientResponse>,
    pub total: usize,
}

/// Query parameters for clients list
#[derive(Debug, Deserialize, Default, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientsQuery {
    #[serde(flatten)]
    pub pagination: PaginationParams,

    /// Filter by status
    pub status: Option<String>,
}

/// Search query parameters
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SearchQuery {
    /// Search term (matches name or identifier)
    pub q: Option<String>,
    /// Search term alternative
    pub query: Option<String>,
}

/// Add note request (matches Java AddNoteRequest)
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddNoteRequest {
    /// Category of the note
    pub category: String,
    /// Note content
    pub text: String,
}

/// Add note response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddNoteResponse {
    pub message: String,
}

/// Client application config response (matches Java ClientApplicationDto)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientApplicationResponse {
    /// Application ID
    pub id: String,
    /// Application code
    pub code: String,
    /// Application display name
    pub name: String,
    /// Application description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Application icon URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
    /// Whether the application itself is active globally
    pub active: bool,
    /// Whether this application is enabled for this specific client
    pub enabled_for_client: bool,
}

/// Client applications list response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientApplicationsResponse {
    pub applications: Vec<ClientApplicationResponse>,
    pub total: usize,
}

/// Update client applications request (matches Java)
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateClientApplicationsRequest {
    /// List of application IDs to enable
    pub enabled_application_ids: Vec<String>,
}

/// Clients service state
#[derive(Clone)]
pub struct ClientsState {
    pub client_repo: Arc<ClientRepository>,
    pub application_repo: Option<Arc<crate::application::repository::ApplicationRepository>>,
    pub application_client_config_repo:
        Option<Arc<crate::application::ApplicationClientConfigRepository>>,
    pub audit_service: Option<Arc<crate::audit::AuditService>>,
    pub create_use_case:
        Arc<crate::client::operations::CreateClientUseCase<crate::usecase::PgUnitOfWork>>,
    pub update_use_case:
        Arc<crate::client::operations::UpdateClientUseCase<crate::usecase::PgUnitOfWork>>,
    pub delete_use_case:
        Arc<crate::client::operations::DeleteClientUseCase<crate::usecase::PgUnitOfWork>>,
    pub activate_use_case:
        Arc<crate::client::operations::ActivateClientUseCase<crate::usecase::PgUnitOfWork>>,
    pub suspend_use_case:
        Arc<crate::client::operations::SuspendClientUseCase<crate::usecase::PgUnitOfWork>>,
    pub add_note_use_case:
        Arc<crate::client::operations::AddClientNoteUseCase<crate::usecase::PgUnitOfWork>>,
    pub update_applications_use_case: Option<
        Arc<
            crate::application::operations::UpdateClientApplicationsUseCase<
                crate::usecase::PgUnitOfWork,
            >,
        >,
    >,
    pub enable_application_use_case: Option<
        Arc<
            crate::application::operations::EnableApplicationForClientUseCase<
                crate::usecase::PgUnitOfWork,
            >,
        >,
    >,
    pub disable_application_use_case: Option<
        Arc<
            crate::application::operations::DisableApplicationForClientUseCase<
                crate::usecase::PgUnitOfWork,
            >,
        >,
    >,
}

/// Create a new client
#[utoipa::path(
    post,
    path = "",
    tag = "clients",
    operation_id = "postApiClients",
    request_body = CreateClientRequest,
    responses(
        (status = 201, description = "Client created", body = crate::shared::api_common::CreatedResponse),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Duplicate identifier")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_client(
    State(state): State<ClientsState>,
    auth: Authenticated,
    Json(req): Json<CreateClientRequest>,
) -> Result<(StatusCode, Json<crate::shared::api_common::CreatedResponse>), PlatformError> {
    use crate::client::operations::CreateClientCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::shared::authorization_service::checks::require_anchor(&auth.0)?;

    let cmd = CreateClientCommand {
        name: req.name,
        identifier: req.identifier,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    let event = state.create_use_case.run(cmd, ctx).await.into_result()?;

    Ok((
        StatusCode::CREATED,
        Json(crate::shared::api_common::CreatedResponse::new(
            event.client_id,
        )),
    ))
}

/// Get client by ID
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "clients",
    operation_id = "getApiClientsById",
    params(
        ("id" = String, Path, description = "Client ID")
    ),
    responses(
        (status = 200, description = "Client found", body = ClientResponse),
        (status = 404, description = "Client not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_client(
    State(state): State<ClientsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<ClientResponse>, PlatformError> {
    // Check access
    if !auth.0.is_anchor() && !auth.0.can_access_client(&id) {
        return Err(PlatformError::forbidden("No access to this client"));
    }

    let client = state
        .client_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Client", &id))?;

    Ok(Json(client.into()))
}

/// List clients
#[utoipa::path(
    get,
    path = "",
    tag = "clients",
    operation_id = "getApiClients",
    params(
        ("page" = Option<u32>, Query, description = "Page number"),
        ("limit" = Option<u32>, Query, description = "Items per page"),
        ("status" = Option<String>, Query, description = "Filter by status")
    ),
    responses(
        (status = 200, description = "List of clients", body = ClientListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_clients(
    State(state): State<ClientsState>,
    auth: Authenticated,
    Query(_query): Query<ClientsQuery>,
) -> Result<Json<ClientListResponse>, PlatformError> {
    let clients = state.client_repo.find_active().await?;

    // Filter by access
    let filtered: Vec<ClientResponse> = clients
        .into_iter()
        .filter(|c| auth.0.is_anchor() || auth.0.can_access_client(&c.id))
        .map(|c| c.into())
        .collect();

    let total = filtered.len();
    Ok(Json(ClientListResponse {
        clients: filtered,
        total,
    }))
}

/// Update client
#[utoipa::path(
    put,
    path = "/{id}",
    tag = "clients",
    operation_id = "putApiClientsById",
    params(
        ("id" = String, Path, description = "Client ID")
    ),
    request_body = UpdateClientRequest,
    responses(
        (status = 204, description = "Client updated"),
        (status = 404, description = "Client not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_client(
    State(state): State<ClientsState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateClientRequest>,
) -> Result<StatusCode, PlatformError> {
    use crate::client::operations::UpdateClientCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::shared::authorization_service::checks::require_anchor(&auth.0)?;

    let cmd = UpdateClientCommand {
        client_id: id,
        name: req.name,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.update_use_case.run(cmd, ctx).await.into_result()?;

    Ok(StatusCode::NO_CONTENT)
}

/// Delete client (soft delete)
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "clients",
    operation_id = "deleteApiClientsById",
    params(
        ("id" = String, Path, description = "Client ID")
    ),
    responses(
        (status = 204, description = "Client deleted"),
        (status = 404, description = "Client not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_client(
    State(state): State<ClientsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<StatusCode, PlatformError> {
    use crate::client::operations::DeleteClientCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::shared::authorization_service::checks::require_anchor(&auth.0)?;

    let cmd = DeleteClientCommand { client_id: id };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.delete_use_case.run(cmd, ctx).await.into_result()?;

    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Status Management Endpoints
// ============================================================================

/// Activate a client
///
/// Transitions a suspended or pending client to active status.
#[utoipa::path(
    post,
    path = "/{id}/activate",
    tag = "clients",
    operation_id = "postApiClientsByIdActivate",
    params(
        ("id" = String, Path, description = "Client ID")
    ),
    responses(
        (status = 200, description = "Client activated", body = StatusChangeResponse),
        (status = 404, description = "Client not found"),
        (status = 403, description = "Insufficient permissions")
    ),
    security(("bearer_auth" = []))
)]
pub async fn activate_client(
    State(state): State<ClientsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<StatusChangeResponse>, PlatformError> {
    use crate::client::operations::ActivateClientCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::shared::authorization_service::checks::require_anchor(&auth.0)?;

    let cmd = ActivateClientCommand {
        client_id: id.clone(),
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.activate_use_case.run(cmd, ctx).await.into_result()?;

    tracing::info!(client_id = %id, principal_id = %auth.0.principal_id, "Client activated");

    Ok(Json(StatusChangeResponse {
        message: "Client activated".to_string(),
    }))
}

/// Suspend a client
///
/// Suspends a client (e.g., for billing issues). Requires a reason.
#[utoipa::path(
    post,
    path = "/{id}/suspend",
    tag = "clients",
    operation_id = "postApiClientsByIdSuspend",
    params(
        ("id" = String, Path, description = "Client ID")
    ),
    request_body = StatusChangeRequest,
    responses(
        (status = 200, description = "Client suspended", body = StatusChangeResponse),
        (status = 404, description = "Client not found"),
        (status = 403, description = "Insufficient permissions")
    ),
    security(("bearer_auth" = []))
)]
pub async fn suspend_client(
    State(state): State<ClientsState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<StatusChangeRequest>,
) -> Result<Json<StatusChangeResponse>, PlatformError> {
    use crate::client::operations::SuspendClientCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::shared::authorization_service::checks::require_anchor(&auth.0)?;

    let reason_for_log = req.reason.clone();
    let cmd = SuspendClientCommand {
        client_id: id.clone(),
        reason: req.reason,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.suspend_use_case.run(cmd, ctx).await.into_result()?;

    tracing::info!(
        client_id = %id,
        principal_id = %auth.0.principal_id,
        reason = %reason_for_log,
        "Client suspended"
    );

    Ok(Json(StatusChangeResponse {
        message: "Client suspended".to_string(),
    }))
}

/// Deactivate a client (soft delete)
///
/// Deactivates/soft-deletes a client. Requires a reason.
#[utoipa::path(
    post,
    path = "/{id}/deactivate",
    tag = "clients",
    operation_id = "postApiClientsByIdDeactivate",
    params(
        ("id" = String, Path, description = "Client ID")
    ),
    request_body = StatusChangeRequest,
    responses(
        (status = 200, description = "Client deactivated", body = StatusChangeResponse),
        (status = 404, description = "Client not found"),
        (status = 403, description = "Insufficient permissions")
    ),
    security(("bearer_auth" = []))
)]
pub async fn deactivate_client(
    State(state): State<ClientsState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<StatusChangeRequest>,
) -> Result<Json<StatusChangeResponse>, PlatformError> {
    // Deactivation is a soft delete — `DeleteClientUseCase` handles it.
    // The reason string is retained in logs; the use case emits the
    // `ClientDeleted` domain event + audit record.
    use crate::client::operations::DeleteClientCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::shared::authorization_service::checks::require_anchor(&auth.0)?;

    let reason_for_log = req.reason.clone();
    let cmd = DeleteClientCommand {
        client_id: id.clone(),
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.delete_use_case.run(cmd, ctx).await.into_result()?;

    tracing::info!(
        client_id = %id,
        principal_id = %auth.0.principal_id,
        reason = %reason_for_log,
        "Client deactivated"
    );

    Ok(Json(StatusChangeResponse {
        message: "Client deactivated".to_string(),
    }))
}

/// Search clients
#[utoipa::path(
    get,
    path = "/search",
    tag = "clients",
    operation_id = "getApiClientsSearch",
    params(
        ("q" = Option<String>, Query, description = "Search term")
    ),
    responses(
        (status = 200, description = "Search results", body = ClientListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn search_clients(
    State(state): State<ClientsState>,
    auth: Authenticated,
    Query(query): Query<SearchQuery>,
) -> Result<Json<ClientListResponse>, PlatformError> {
    let search_term = query.q.or(query.query).unwrap_or_default();

    let clients = if search_term.is_empty() {
        state.client_repo.find_all().await?
    } else {
        state.client_repo.search(&search_term).await?
    };

    // Filter by access if not anchor
    let clients: Vec<Client> = if auth.0.is_anchor() {
        clients
    } else {
        clients
            .into_iter()
            .filter(|c| auth.0.can_access_client(&c.id))
            .collect()
    };

    let total = clients.len();
    let responses: Vec<ClientResponse> = clients.into_iter().map(|c| c.into()).collect();

    Ok(Json(ClientListResponse {
        clients: responses,
        total,
    }))
}

/// Get client by identifier
#[utoipa::path(
    get,
    path = "/by-identifier/{identifier}",
    tag = "clients",
    operation_id = "getApiClientsByIdentifierByIdentifier",
    params(
        ("identifier" = String, Path, description = "Client identifier/slug")
    ),
    responses(
        (status = 200, description = "Client found", body = ClientResponse),
        (status = 404, description = "Client not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_client_by_identifier(
    State(state): State<ClientsState>,
    auth: Authenticated,
    Path(identifier): Path<String>,
) -> Result<Json<ClientResponse>, PlatformError> {
    let client = state
        .client_repo
        .find_by_identifier(&identifier)
        .await?
        .ok_or_else(|| PlatformError::not_found("Client", &identifier))?;

    // Check access
    if !auth.0.is_anchor() && !auth.0.can_access_client(&client.id) {
        return Err(PlatformError::forbidden("No access to this client"));
    }

    Ok(Json(client.into()))
}

/// Add note to client
#[utoipa::path(
    post,
    path = "/{id}/notes",
    tag = "clients",
    operation_id = "postApiClientsByIdNotes",
    params(
        ("id" = String, Path, description = "Client ID")
    ),
    request_body = AddNoteRequest,
    responses(
        (status = 200, description = "Note added", body = AddNoteResponse),
        (status = 404, description = "Client not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn add_note(
    State(state): State<ClientsState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<AddNoteRequest>,
) -> Result<Json<AddNoteResponse>, PlatformError> {
    use crate::client::operations::AddClientNoteCommand;
    use crate::usecase::{ExecutionContext, UseCase};

    crate::shared::authorization_service::checks::require_anchor(&auth.0)?;

    let cmd = AddClientNoteCommand {
        client_id: id.clone(),
        category: req.category,
        text: req.text,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state.add_note_use_case.run(cmd, ctx).await.into_result()?;

    tracing::info!(
        client_id = %id,
        principal_id = %auth.0.principal_id,
        "Note added to client"
    );

    Ok(Json(AddNoteResponse {
        message: "Note added successfully".to_string(),
    }))
}

/// Get client applications
#[utoipa::path(
    get,
    path = "/{id}/applications",
    tag = "clients",
    operation_id = "getApiClientsByIdApplications",
    params(
        ("id" = String, Path, description = "Client ID")
    ),
    responses(
        (status = 200, description = "Client applications", body = ClientApplicationsResponse),
        (status = 404, description = "Client not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_client_applications(
    State(state): State<ClientsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<ClientApplicationsResponse>, PlatformError> {
    // Check access
    if !auth.0.is_anchor() && !auth.0.can_access_client(&id) {
        return Err(PlatformError::forbidden("No access to this client"));
    }

    // Verify client exists
    let _client = state
        .client_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Client", &id))?;

    // Get all applications and their configs for this client
    let mut applications = Vec::new();

    if let Some(ref app_repo) = state.application_repo {
        // Get ALL applications (not just active), same as Java
        let all_apps = app_repo.find_all().await?;

        if let Some(ref config_repo) = state.application_client_config_repo {
            let configs = config_repo.find_by_client(&id).await?;
            let enabled_app_ids: std::collections::HashSet<_> = configs
                .iter()
                .filter(|c| c.enabled)
                .map(|c| c.application_id.as_str())
                .collect();

            for app in all_apps {
                applications.push(ClientApplicationResponse {
                    id: app.id.clone(),
                    code: app.code.clone(),
                    name: app.name.clone(),
                    description: app.description.clone(),
                    icon_url: app.icon_url.clone(),
                    active: app.active,
                    enabled_for_client: enabled_app_ids.contains(app.id.as_str()),
                });
            }
        } else {
            // No config repo, return apps as all disabled
            for app in all_apps {
                applications.push(ClientApplicationResponse {
                    id: app.id.clone(),
                    code: app.code.clone(),
                    name: app.name.clone(),
                    description: app.description.clone(),
                    icon_url: app.icon_url.clone(),
                    active: app.active,
                    enabled_for_client: false,
                });
            }
        }
    }

    let total = applications.len();
    Ok(Json(ClientApplicationsResponse {
        applications,
        total,
    }))
}

/// Enable application for client
#[utoipa::path(
    post,
    path = "/{id}/applications/{application_id}/enable",
    tag = "clients",
    operation_id = "postApiClientsByIdApplicationsByAppIdEnable",
    params(
        ("id" = String, Path, description = "Client ID"),
        ("application_id" = String, Path, description = "Application ID")
    ),
    responses(
        (status = 204, description = "Application enabled"),
        (status = 404, description = "Client or application not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn enable_application(
    State(state): State<ClientsState>,
    auth: Authenticated,
    Path((id, application_id)): Path<(String, String)>,
) -> Result<StatusCode, PlatformError> {
    use crate::usecase::{ExecutionContext, UseCase};

    crate::shared::authorization_service::checks::require_anchor(&auth.0)?;

    let use_case = state
        .enable_application_use_case
        .as_ref()
        .ok_or_else(|| PlatformError::internal("Enable-application use case not configured"))?;

    let command = crate::application::operations::EnableApplicationForClientCommand {
        application_id,
        client_id: id,
    };
    let ctx = ExecutionContext::from_auth(&auth.0);
    use_case.run(command, ctx).await.into_result()?;

    Ok(StatusCode::NO_CONTENT)
}

/// Disable application for client
#[utoipa::path(
    post,
    path = "/{id}/applications/{application_id}/disable",
    tag = "clients",
    operation_id = "postApiClientsByIdApplicationsByAppIdDisable",
    params(
        ("id" = String, Path, description = "Client ID"),
        ("application_id" = String, Path, description = "Application ID")
    ),
    responses(
        (status = 204, description = "Application disabled"),
        (status = 404, description = "Client or application not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn disable_application(
    State(state): State<ClientsState>,
    auth: Authenticated,
    Path((id, application_id)): Path<(String, String)>,
) -> Result<StatusCode, PlatformError> {
    use crate::usecase::{ExecutionContext, UseCase};

    crate::shared::authorization_service::checks::require_anchor(&auth.0)?;

    let use_case = state
        .disable_application_use_case
        .as_ref()
        .ok_or_else(|| PlatformError::internal("Disable-application use case not configured"))?;

    let command = crate::application::operations::DisableApplicationForClientCommand {
        application_id,
        client_id: id,
    };
    let ctx = ExecutionContext::from_auth(&auth.0);
    use_case.run(command, ctx).await.into_result()?;

    Ok(StatusCode::NO_CONTENT)
}

/// Update client applications (bulk)
#[utoipa::path(
    put,
    path = "/{id}/applications",
    tag = "clients",
    operation_id = "putApiClientsByIdApplications",
    params(
        ("id" = String, Path, description = "Client ID")
    ),
    request_body = UpdateClientApplicationsRequest,
    responses(
        (status = 204, description = "Applications updated"),
        (status = 404, description = "Client not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_client_applications(
    State(state): State<ClientsState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateClientApplicationsRequest>,
) -> Result<StatusCode, PlatformError> {
    crate::shared::authorization_service::checks::require_anchor(&auth.0)?;

    use crate::usecase::{ExecutionContext, UseCase};

    let use_case = state
        .update_applications_use_case
        .as_ref()
        .ok_or_else(|| PlatformError::internal("Client applications use case not configured"))?;

    let command = crate::application::operations::UpdateClientApplicationsCommand {
        client_id: id,
        enabled_application_ids: req.enabled_application_ids,
    };
    let ctx = ExecutionContext::from_auth(&auth.0);
    use_case.run(command, ctx).await.into_result()?;

    Ok(StatusCode::NO_CONTENT)
}

/// Create clients router
pub fn clients_router(state: ClientsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(create_client, list_clients))
        .routes(routes!(search_clients))
        .routes(routes!(get_client_by_identifier))
        .routes(routes!(get_client, update_client, delete_client))
        .routes(routes!(activate_client))
        .routes(routes!(suspend_client))
        .routes(routes!(deactivate_client))
        .routes(routes!(add_note))
        .routes(routes!(get_client_applications, update_client_applications))
        .routes(routes!(enable_application))
        .routes(routes!(disable_application))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::entity::{Client, ClientStatus};
    use chrono::Utc;

    fn make_test_client() -> Client {
        let now = Utc::now();
        Client {
            id: "clt_ABCDEFGHIJKLM".to_string(),
            name: "Acme Corporation".to_string(),
            identifier: "acme-corp".to_string(),
            status: ClientStatus::Active,
            status_reason: None,
            status_changed_at: None,
            notes: vec![],
            created_at: now,
            updated_at: now,
        }
    }

    // --- ClientResponse serialization ---

    #[test]
    fn test_client_response_serialization() {
        let client = make_test_client();
        let response = ClientResponse::from(client);

        let json = serde_json::to_value(&response).unwrap();

        assert_eq!(json["id"], "clt_ABCDEFGHIJKLM");
        assert_eq!(json["name"], "Acme Corporation");
        assert_eq!(json["identifier"], "acme-corp");
        assert_eq!(json["status"], "ACTIVE");
        assert!(json["statusReason"].is_null());
        assert!(json["statusChangedAt"].is_null());
        // Verify camelCase field names
        assert!(json.get("createdAt").is_some());
        assert!(json.get("updatedAt").is_some());
        // Verify no snake_case leak
        assert!(json.get("status_reason").is_none());
        assert!(json.get("status_changed_at").is_none());
        assert!(json.get("created_at").is_none());
    }

    #[test]
    fn test_client_response_with_suspension() {
        let mut client = make_test_client();
        client.suspend("Payment overdue");

        let response = ClientResponse::from(client);
        let json = serde_json::to_value(&response).unwrap();

        assert_eq!(json["status"], "SUSPENDED");
        assert_eq!(json["statusReason"], "Payment overdue");
        assert!(
            json["statusChangedAt"].is_string(),
            "statusChangedAt should be ISO 8601 string"
        );
    }

    // --- CreateClientRequest deserialization ---

    #[test]
    fn test_create_client_request_deserialization() {
        let json = serde_json::json!({
            "identifier": "new-client",
            "name": "New Client"
        });

        let req: CreateClientRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.identifier, "new-client");
        assert_eq!(req.name, "New Client");
    }

    #[test]
    fn test_create_client_request_camel_case() {
        // Verify that camelCase deserialization works (not just exact match)
        let json = serde_json::json!({
            "identifier": "test-id",
            "name": "Test Name"
        });

        let req: CreateClientRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.identifier, "test-id");
    }

    #[test]
    fn test_create_client_request_missing_identifier() {
        let json = serde_json::json!({
            "name": "Test"
        });

        let result = serde_json::from_value::<CreateClientRequest>(json);
        assert!(result.is_err(), "Should fail without identifier");
    }

    #[test]
    fn test_create_client_request_missing_name() {
        let json = serde_json::json!({
            "identifier": "test"
        });

        let result = serde_json::from_value::<CreateClientRequest>(json);
        assert!(result.is_err(), "Should fail without name");
    }

    #[test]
    fn test_create_client_request_empty_json() {
        let json = serde_json::json!({});
        let result = serde_json::from_value::<CreateClientRequest>(json);
        assert!(result.is_err(), "Should fail with empty JSON");
    }

    // --- UpdateClientRequest ---

    #[test]
    fn test_update_client_request_deserialization() {
        let json = serde_json::json!({
            "name": "Updated Name"
        });

        let req: UpdateClientRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.name, Some("Updated Name".to_string()));
    }

    #[test]
    fn test_update_client_request_empty() {
        let json = serde_json::json!({});
        let req: UpdateClientRequest = serde_json::from_value(json).unwrap();
        assert!(req.name.is_none());
    }

    // --- StatusChangeRequest ---

    #[test]
    fn test_status_change_request_deserialization() {
        let json = serde_json::json!({
            "reason": "Payment issue"
        });

        let req: StatusChangeRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.reason, "Payment issue");
    }

    #[test]
    fn test_status_change_request_missing_reason() {
        let json = serde_json::json!({});
        let result = serde_json::from_value::<StatusChangeRequest>(json);
        assert!(result.is_err(), "Should fail without reason");
    }

    // --- ClientListResponse ---

    #[test]
    fn test_client_list_response_serialization() {
        let client = make_test_client();
        let list = ClientListResponse {
            clients: vec![ClientResponse::from(client)],
            total: 1,
        };

        let json = serde_json::to_value(&list).unwrap();
        assert!(json["clients"].is_array());
        assert_eq!(json["clients"].as_array().unwrap().len(), 1);
        assert_eq!(json["total"], 1);
    }

    // --- AddNoteRequest ---

    #[test]
    fn test_add_note_request_deserialization() {
        let json = serde_json::json!({
            "category": "billing",
            "text": "Payment received"
        });

        let req: AddNoteRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.category, "billing");
        assert_eq!(req.text, "Payment received");
    }

    #[test]
    fn test_add_note_request_missing_fields() {
        let json = serde_json::json!({ "category": "billing" });
        let result = serde_json::from_value::<AddNoteRequest>(json);
        assert!(result.is_err(), "Should fail without text");
    }
}
