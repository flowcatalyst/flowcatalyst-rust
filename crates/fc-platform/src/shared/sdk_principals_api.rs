//! SDK Principals API
//!
//! REST endpoints for external SDK access to principal management via Bearer tokens.

use axum::{
    extract::{State, Path, Query},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::principal::api::{
    PrincipalResponse, PrincipalListResponse, CreateUserRequest, UpdatePrincipalRequest,
    BatchAssignRolesRequest, BatchAssignRolesResponse, RoleAssignmentDto,
    ClientAccessGrantResponse, ClientAccessListResponse, StatusChangeResponse,
};
use crate::principal::entity::{Principal, UserScope};
use crate::principal::repository::PrincipalRepository;
use crate::shared::api_common::CreatedResponse;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;

/// SDK Principals service state
#[derive(Clone)]
pub struct SdkPrincipalsState {
    pub principal_repo: Arc<PrincipalRepository>,
}

/// Query parameters for SDK principals list
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SdkPrincipalsQuery {
    pub client_id: Option<String>,
    #[serde(rename = "type")]
    pub principal_type: Option<String>,
    pub active: Option<String>,
    pub email: Option<String>,
}

/// List principals (SDK)
#[utoipa::path(
    get,
    path = "",
    tag = "sdk-principals",
    operation_id = "getApiSdkPrincipals",
    params(
        ("clientId" = Option<String>, Query, description = "Filter by client ID"),
        ("type" = Option<String>, Query, description = "Filter by type (USER, SERVICE)"),
        ("active" = Option<String>, Query, description = "Filter by active status (true/false)"),
        ("email" = Option<String>, Query, description = "Filter by email address")
    ),
    responses(
        (status = 200, description = "List of principals", body = PrincipalListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_sdk_principals(
    State(state): State<SdkPrincipalsState>,
    _auth: Authenticated,
    Query(query): Query<SdkPrincipalsQuery>,
) -> Result<Json<PrincipalListResponse>, PlatformError> {
    let principals = if let Some(ref email) = query.email {
        match state.principal_repo.find_by_email(email).await? {
            Some(p) => vec![p],
            None => vec![],
        }
    } else {
        state.principal_repo.find_all().await?
    };

    // Filter in memory by type and active if provided
    let filtered: Vec<PrincipalResponse> = principals
        .into_iter()
        .filter(|p| {
            if let Some(ref t) = query.principal_type {
                let type_str = format!("{:?}", p.principal_type).to_uppercase();
                if type_str != t.to_uppercase() {
                    return false;
                }
            }
            if let Some(ref active_str) = query.active {
                match active_str.to_lowercase().as_str() {
                    "true" => {
                        if !p.active {
                            return false;
                        }
                    }
                    "false" => {
                        if p.active {
                            return false;
                        }
                    }
                    _ => {}
                }
            }
            if let Some(ref client_id) = query.client_id {
                if p.client_id.as_deref() != Some(client_id.as_str()) {
                    return false;
                }
            }
            true
        })
        .map(|p| p.into())
        .collect();

    let total = filtered.len();
    Ok(Json(PrincipalListResponse {
        principals: filtered,
        total,
    }))
}

/// Get principal by ID (SDK)
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "sdk-principals",
    operation_id = "getApiSdkPrincipalsById",
    params(
        ("id" = String, Path, description = "Principal ID")
    ),
    responses(
        (status = 200, description = "Principal found", body = PrincipalResponse),
        (status = 404, description = "Principal not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_sdk_principal(
    State(state): State<SdkPrincipalsState>,
    _auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<PrincipalResponse>, PlatformError> {
    let principal = state
        .principal_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Principal", &id))?;

    Ok(Json(principal.into()))
}

/// Create a new user principal (SDK)
#[utoipa::path(
    post,
    path = "/user",
    tag = "sdk-principals",
    operation_id = "postApiSdkPrincipalsUser",
    request_body = CreateUserRequest,
    responses(
        (status = 201, description = "User created", body = CreatedResponse),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Duplicate email")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_sdk_user(
    State(state): State<SdkPrincipalsState>,
    _auth: Authenticated,
    Json(req): Json<CreateUserRequest>,
) -> Result<Json<CreatedResponse>, PlatformError> {
    // Check for duplicate email
    if state.principal_repo.find_by_email(&req.email).await?.is_some() {
        return Err(PlatformError::duplicate("Principal", "email", &req.email));
    }

    // Determine scope based on client_id
    let scope = if req.client_id.is_some() {
        UserScope::Client
    } else {
        UserScope::Anchor
    };

    let mut principal = Principal::new_user(&req.email, scope);
    principal.name = req.name.clone();

    if let Some(cid) = req.client_id.clone() {
        principal = principal.with_client_id(cid);
    }

    let id = principal.id.clone();
    state.principal_repo.insert(&principal).await?;

    Ok(Json(CreatedResponse::new(id)))
}

/// Update principal (SDK)
#[utoipa::path(
    put,
    path = "/{id}",
    tag = "sdk-principals",
    operation_id = "putApiSdkPrincipalsById",
    params(
        ("id" = String, Path, description = "Principal ID")
    ),
    request_body = UpdatePrincipalRequest,
    responses(
        (status = 200, description = "Principal updated", body = PrincipalResponse),
        (status = 404, description = "Principal not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_sdk_principal(
    State(state): State<SdkPrincipalsState>,
    _auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdatePrincipalRequest>,
) -> Result<Json<PrincipalResponse>, PlatformError> {
    let mut principal = state
        .principal_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Principal", &id))?;

    if let Some(name) = req.name {
        principal.name = name;
    }

    principal.updated_at = chrono::Utc::now();
    state.principal_repo.update(&principal).await?;

    Ok(Json(principal.into()))
}

/// Activate a principal (SDK)
#[utoipa::path(
    post,
    path = "/{id}/activate",
    tag = "sdk-principals",
    operation_id = "postApiSdkPrincipalsByIdActivate",
    params(
        ("id" = String, Path, description = "Principal ID")
    ),
    responses(
        (status = 200, description = "Principal activated", body = StatusChangeResponse),
        (status = 404, description = "Principal not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn activate_sdk_principal(
    State(state): State<SdkPrincipalsState>,
    _auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<StatusChangeResponse>, PlatformError> {
    let mut principal = state
        .principal_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Principal", &id))?;

    principal.activate();
    state.principal_repo.update(&principal).await?;

    Ok(Json(StatusChangeResponse {
        message: "Principal activated".to_string(),
    }))
}

/// Deactivate a principal (SDK)
#[utoipa::path(
    post,
    path = "/{id}/deactivate",
    tag = "sdk-principals",
    operation_id = "postApiSdkPrincipalsByIdDeactivate",
    params(
        ("id" = String, Path, description = "Principal ID")
    ),
    responses(
        (status = 200, description = "Principal deactivated", body = StatusChangeResponse),
        (status = 404, description = "Principal not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn deactivate_sdk_principal(
    State(state): State<SdkPrincipalsState>,
    _auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<StatusChangeResponse>, PlatformError> {
    let mut principal = state
        .principal_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Principal", &id))?;

    principal.deactivate();
    state.principal_repo.update(&principal).await?;

    Ok(Json(StatusChangeResponse {
        message: "Principal deactivated".to_string(),
    }))
}

/// Get roles assigned to a principal (SDK)
#[utoipa::path(
    get,
    path = "/{id}/roles",
    tag = "sdk-principals",
    operation_id = "getApiSdkPrincipalsByIdRoles",
    params(
        ("id" = String, Path, description = "Principal ID")
    ),
    responses(
        (status = 200, description = "List of roles", body = Vec<RoleAssignmentDto>),
        (status = 404, description = "Principal not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_sdk_principal_roles(
    State(state): State<SdkPrincipalsState>,
    _auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<Vec<RoleAssignmentDto>>, PlatformError> {
    let principal = state
        .principal_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Principal", &id))?;

    let roles: Vec<RoleAssignmentDto> = principal
        .roles
        .iter()
        .enumerate()
        .map(|(i, r)| RoleAssignmentDto {
            id: format!("{}-role-{}", id, i),
            role_name: r.role.clone(),
            assignment_source: "SDK".to_string(),
            assigned_at: r.assigned_at.to_rfc3339(),
        })
        .collect();

    Ok(Json(roles))
}

/// Assign roles to a principal (SDK) — declarative, replaces all roles
#[utoipa::path(
    put,
    path = "/{id}/roles",
    tag = "sdk-principals",
    operation_id = "putApiSdkPrincipalsByIdRoles",
    params(
        ("id" = String, Path, description = "Principal ID")
    ),
    request_body = BatchAssignRolesRequest,
    responses(
        (status = 200, description = "Roles updated", body = BatchAssignRolesResponse),
        (status = 404, description = "Principal not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn assign_sdk_principal_roles(
    State(state): State<SdkPrincipalsState>,
    _auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<BatchAssignRolesRequest>,
) -> Result<Json<BatchAssignRolesResponse>, PlatformError> {
    let mut principal = state
        .principal_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Principal", &id))?;

    // Track what was added/removed
    let old_roles: std::collections::HashSet<String> =
        principal.roles.iter().map(|r| r.role.clone()).collect();
    let new_roles: std::collections::HashSet<String> = req.roles.iter().cloned().collect();

    let added: Vec<String> = new_roles.difference(&old_roles).cloned().collect();
    let removed: Vec<String> = old_roles.difference(&new_roles).cloned().collect();

    // Clear existing roles and assign new ones
    principal.roles.clear();
    for role in req.roles {
        principal.assign_role(role);
    }
    principal.updated_at = chrono::Utc::now();

    state.principal_repo.update(&principal).await?;

    // Build response with role DTOs
    let roles: Vec<RoleAssignmentDto> = principal
        .roles
        .iter()
        .enumerate()
        .map(|(i, r)| RoleAssignmentDto {
            id: format!("{}-role-{}", id, i),
            role_name: r.role.clone(),
            assignment_source: "SDK".to_string(),
            assigned_at: r.assigned_at.to_rfc3339(),
        })
        .collect();

    Ok(Json(BatchAssignRolesResponse {
        roles,
        added,
        removed,
    }))
}

/// Get client access grants for a principal (SDK)
#[utoipa::path(
    get,
    path = "/{id}/clients",
    tag = "sdk-principals",
    operation_id = "getApiSdkPrincipalsByIdClients",
    params(
        ("id" = String, Path, description = "Principal ID")
    ),
    responses(
        (status = 200, description = "Client access grants", body = ClientAccessListResponse),
        (status = 404, description = "Principal not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_sdk_principal_clients(
    State(state): State<SdkPrincipalsState>,
    _auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<ClientAccessListResponse>, PlatformError> {
    let principal = state
        .principal_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Principal", &id))?;

    let grants: Vec<ClientAccessGrantResponse> = principal
        .assigned_clients
        .iter()
        .enumerate()
        .map(|(i, client_id)| ClientAccessGrantResponse {
            id: format!("{}-client-{}", id, i),
            client_id: client_id.clone(),
            granted_at: principal.created_at.to_rfc3339(),
            expires_at: None,
        })
        .collect();

    Ok(Json(ClientAccessListResponse { grants }))
}

/// Grant client access to a principal (SDK)
#[utoipa::path(
    post,
    path = "/{id}/clients/{client_id}",
    tag = "sdk-principals",
    operation_id = "postApiSdkPrincipalsByIdClientsByClientId",
    params(
        ("id" = String, Path, description = "Principal ID"),
        ("client_id" = String, Path, description = "Client ID to grant access to")
    ),
    responses(
        (status = 200, description = "Client access granted", body = ClientAccessGrantResponse),
        (status = 404, description = "Principal not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn grant_sdk_client_access(
    State(state): State<SdkPrincipalsState>,
    _auth: Authenticated,
    Path((id, client_id)): Path<(String, String)>,
) -> Result<Json<ClientAccessGrantResponse>, PlatformError> {
    let mut principal = state
        .principal_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Principal", &id))?;

    let granted_at = chrono::Utc::now();
    principal.grant_client_access(client_id.clone());
    state.principal_repo.update(&principal).await?;

    Ok(Json(ClientAccessGrantResponse {
        id: format!("{}-client-{}", id, principal.assigned_clients.len() - 1),
        client_id,
        granted_at: granted_at.to_rfc3339(),
        expires_at: None,
    }))
}

/// Revoke client access from a principal (SDK)
#[utoipa::path(
    delete,
    path = "/{id}/clients/{client_id}",
    tag = "sdk-principals",
    operation_id = "deleteApiSdkPrincipalsByIdClientsByClientId",
    params(
        ("id" = String, Path, description = "Principal ID"),
        ("client_id" = String, Path, description = "Client ID to revoke")
    ),
    responses(
        (status = 204, description = "Client access revoked"),
        (status = 404, description = "Principal not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn revoke_sdk_client_access(
    State(state): State<SdkPrincipalsState>,
    _auth: Authenticated,
    Path((id, client_id)): Path<(String, String)>,
) -> Result<StatusCode, PlatformError> {
    let mut principal = state
        .principal_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Principal", &id))?;

    principal.revoke_client_access(&client_id);
    state.principal_repo.update(&principal).await?;

    Ok(StatusCode::NO_CONTENT)
}

/// Create the SDK principals router
pub fn sdk_principals_router(state: SdkPrincipalsState) -> Router {
    Router::new()
        .route("/", get(list_sdk_principals))
        .route("/user", post(create_sdk_user))
        .route("/{id}", get(get_sdk_principal).put(update_sdk_principal))
        .route("/{id}/activate", post(activate_sdk_principal))
        .route("/{id}/deactivate", post(deactivate_sdk_principal))
        .route(
            "/{id}/roles",
            get(get_sdk_principal_roles).put(assign_sdk_principal_roles),
        )
        .route(
            "/{id}/clients",
            get(get_sdk_principal_clients),
        )
        .route(
            "/{id}/clients/{client_id}",
            post(grant_sdk_client_access).delete(revoke_sdk_client_access),
        )
        .with_state(state)
}
