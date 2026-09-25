//! Platform-config routes as Go serves them (`platformconfig/api/api.go:32-40`):
//!
//! - `GET|PUT|DELETE /api/config/{appCode}/{section}/{property}` (`?clientId=`)
//! - `GET    /api/platform-config/{app}`          → `{items}`
//! - `GET    /api/platform-config/{app}/access`   → `{items}`
//! - `POST   /api/platform-config/{app}/access`   → 201 `{id}`
//! - `DELETE /api/platform-config/access/{id}`    → 204
//!
//! The property routes follow Go's gate: an anchor passes, anyone else needs
//! a platform-config access grant (read or write) on one of its roles for
//! the application. The application need not be registered, as in Go; a
//! registered one must be in the caller's application scope (Rust's rule,
//! 404 otherwise). Scope is derived as Go's: `CLIENT` when `clientId` is given,
//! otherwise `GLOBAL`. A `SECRET` value reads as `***` to a non-anchor.
//! Rust keeps encrypting secrets at rest; an anchor read decrypts them (a
//! Go-written plaintext row reads as stored).
//!
//! The access-grant routes are Go's `anchorWith(platform:admin:config:*)`.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::access_api::{AccessListResponse, AccessResponse};
use super::access_repository::PlatformConfigAccessRepository;
use super::entity::{ConfigScope, ConfigValueType, PlatformConfig};
use super::operations::{
    GrantPlatformConfigAccessCommand, GrantPlatformConfigAccessUseCase,
    RevokePlatformConfigAccessCommand, RevokePlatformConfigAccessUseCase,
    SetPlatformConfigPropertyCommand, SetPlatformConfigPropertyUseCase,
};
use super::repository::PlatformConfigRepository;
use crate::shared::api_common::CreatedResponse;
use crate::shared::authorization_service::checks;
use crate::shared::encryption_service::{is_encrypted_ref, EncryptionService};
use crate::shared::enum_str::parse_opt;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::usecase::{ExecutionContext, PgUnitOfWork, UseCase};
use crate::AuthContext;

#[derive(Clone)]
pub struct GoPlatformConfigState {
    pub config_repo: Arc<PlatformConfigRepository>,
    pub access_repo: Arc<PlatformConfigAccessRepository>,
    pub encryption: Option<Arc<EncryptionService>>,
    pub set_property_use_case: Arc<SetPlatformConfigPropertyUseCase<PgUnitOfWork>>,
    pub grant_access_use_case: Arc<GrantPlatformConfigAccessUseCase<PgUnitOfWork>>,
    pub revoke_access_use_case: Arc<RevokePlatformConfigAccessUseCase<PgUnitOfWork>>,
    pub application_repo: Arc<crate::ApplicationRepository>,
    pub app_access: Arc<crate::shared::authorization_service::ApplicationAccessService>,
}

/// Go `ConfigResponse`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GoConfigResponse {
    pub id: String,
    pub application_code: String,
    pub section: String,
    pub property: String,
    pub scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    pub value_type: String,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GoConfigListResponse {
    pub items: Vec<GoConfigResponse>,
}

/// Go `SetPropertyRequest`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GoSetPropertyRequest {
    pub value: String,
    pub value_type: Option<String>,
    pub description: Option<String>,
    pub client_id: Option<String>,
}

/// Go `GrantAccessRequest`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GoGrantAccessRequest {
    pub role_code: String,
    pub can_write: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientIdQuery {
    pub client_id: Option<String>,
}

/// Go's coordinate: `CLIENT` with a client id, `GLOBAL` without.
fn coordinate(client_id: Option<&str>) -> (ConfigScope, Option<&str>) {
    match client_id.filter(|c| !c.is_empty()) {
        Some(c) => (ConfigScope::Client, Some(c)),
        None => (ConfigScope::Global, None),
    }
}

/// Go's property-route gate: anchor, or a read/write grant for the app on
/// one of the caller's roles.
async fn require_property_access(
    state: &GoPlatformConfigState,
    ctx: &AuthContext,
    app: &str,
    write: bool,
) -> Result<(), PlatformError> {
    if ctx.is_anchor() {
        return Ok(());
    }
    let granted = !ctx.roles.is_empty()
        && state
            .access_repo
            .find_by_role_codes(app, &ctx.roles)
            .await?
            .iter()
            .any(|a| if write { a.can_write } else { a.can_read });
    if granted {
        return Ok(());
    }
    let kind = if write { "write" } else { "read" };
    Err(PlatformError::forbidden(format!(
        "No {kind} access to platform config for {app}"
    )))
}

/// Rust's application confinement on top of Go's gate: a code naming a
/// registered application must be one the caller reaches (404 otherwise,
/// the owner's rule for an out-of-scope application), so an anchor service
/// account of one application cannot rewrite another's configuration. A
/// code no application has is not confined, as in Go.
async fn require_application_access_if_registered(
    state: &GoPlatformConfigState,
    ctx: &AuthContext,
    app: &str,
) -> Result<(), PlatformError> {
    if state.application_repo.find_by_code(app).await?.is_some() {
        state
            .app_access
            .require_application_access(ctx, app)
            .await?;
    }
    Ok(())
}

async fn can_read_config_property(
    state: &GoPlatformConfigState,
    ctx: &AuthContext,
    app: &str,
) -> Result<(), PlatformError> {
    require_property_access(state, ctx, app, false).await
}

async fn can_write_config_property(
    state: &GoPlatformConfigState,
    ctx: &AuthContext,
    app: &str,
) -> Result<(), PlatformError> {
    require_property_access(state, ctx, app, true).await
}

impl GoPlatformConfigState {
    /// The response: a secret is `***` unless `reveal` (an anchor reading,
    /// or the writer's own answer), when it reads decrypted.
    fn response(&self, c: PlatformConfig, reveal: bool) -> GoConfigResponse {
        let value = if c.value_type == ConfigValueType::Secret {
            if !reveal {
                c.masked_value().to_string()
            } else if is_encrypted_ref(&c.value) {
                self.encryption
                    .as_deref()
                    .and_then(|e| e.decrypt_ref(&c.value).ok())
                    .unwrap_or_else(|| c.masked_value().to_string())
            } else {
                c.value.clone()
            }
        } else {
            c.value.clone()
        };
        GoConfigResponse {
            id: c.id,
            application_code: c.application_code,
            section: c.section,
            property: c.property,
            scope: c.scope.as_str().to_string(),
            client_id: c.client_id,
            value_type: c.value_type.as_str().to_string(),
            value,
            description: c.description,
            created_at: c.created_at.to_rfc3339(),
            updated_at: c.updated_at.to_rfc3339(),
        }
    }
}

/// Every property of an application (Go `listPlatformConfig`).
#[utoipa::path(
    get,
    path = "/api/platform-config/{app}",
    tag = "platform-config",
    operation_id = "listPlatformConfig",
    params(("app" = String, Path, description = "Application code")),
    responses((status = 200, description = "Properties", body = GoConfigListResponse)),
    security(("bearer_auth" = []))
)]
pub async fn list_platform_config(
    State(state): State<GoPlatformConfigState>,
    auth: Authenticated,
    Path(app): Path<String>,
) -> Result<Json<GoConfigListResponse>, PlatformError> {
    can_read_config_property(&state, &auth.0, &app).await?;
    require_application_access_if_registered(&state, &auth.0, &app).await?;
    let rows = state
        .config_repo
        .find_by_application(&app, None, None)
        .await?;
    Ok(Json(GoConfigListResponse {
        items: rows
            .into_iter()
            .map(|c| state.response(c, auth.0.is_anchor()))
            .collect(),
    }))
}

/// One property (Go `getConfigProperty`).
#[utoipa::path(
    get,
    path = "/api/config/{appCode}/{section}/{property}",
    tag = "platform-config",
    operation_id = "getConfigProperty",
    params(
        ("appCode" = String, Path, description = "Application code"),
        ("section" = String, Path, description = "Section"),
        ("property" = String, Path, description = "Property"),
        ("clientId" = Option<String>, Query, description = "Client for a CLIENT-scoped property")
    ),
    responses(
        (status = 200, description = "The property", body = GoConfigResponse),
        (status = 404, description = "Not set")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_config_property(
    State(state): State<GoPlatformConfigState>,
    auth: Authenticated,
    Path((app, section, property)): Path<(String, String, String)>,
    Query(q): Query<ClientIdQuery>,
) -> Result<Json<GoConfigResponse>, PlatformError> {
    can_read_config_property(&state, &auth.0, &app).await?;
    require_application_access_if_registered(&state, &auth.0, &app).await?;
    let (scope, client_id) = coordinate(q.client_id.as_deref());
    let config = state
        .config_repo
        .find_by_key(&app, &section, &property, scope.as_str(), client_id)
        .await?
        .ok_or_else(|| {
            PlatformError::not_found_code("Config", format!("{app}/{section}/{property}"))
        })?;
    Ok(Json(state.response(config, auth.0.is_anchor())))
}

/// Create or update a property (Go `setConfigProperty`): 200 with the row.
#[utoipa::path(
    put,
    path = "/api/config/{appCode}/{section}/{property}",
    tag = "platform-config",
    operation_id = "setConfigProperty",
    params(
        ("appCode" = String, Path, description = "Application code"),
        ("section" = String, Path, description = "Section"),
        ("property" = String, Path, description = "Property"),
        ("clientId" = Option<String>, Query, description = "Client for a CLIENT-scoped property")
    ),
    request_body = GoSetPropertyRequest,
    responses(
        (status = 200, description = "The property", body = GoConfigResponse),
        (status = 400, description = "Invalid value type")
    ),
    security(("bearer_auth" = []))
)]
pub async fn set_config_property(
    State(state): State<GoPlatformConfigState>,
    auth: Authenticated,
    Path((app, section, property)): Path<(String, String, String)>,
    Query(q): Query<ClientIdQuery>,
    Json(req): Json<GoSetPropertyRequest>,
) -> Result<Json<GoConfigResponse>, PlatformError> {
    can_write_config_property(&state, &auth.0, &app).await?;
    require_application_access_if_registered(&state, &auth.0, &app).await?;
    // The query's clientId wins over the body's (Go).
    let client_id = q.client_id.or(req.client_id);
    let (scope, client_id) = coordinate(client_id.as_deref());
    let value_type = parse_opt::<ConfigValueType>(req.value_type.as_deref()).map_err(|_| {
        PlatformError::bad_request_code("INVALID_VALUE_TYPE", "valueType must be PLAIN or SECRET")
    })?;
    let cmd = SetPlatformConfigPropertyCommand {
        application_code: app.clone(),
        section: section.clone(),
        property: property.clone(),
        value: req.value,
        scope,
        client_id: client_id.map(str::to_string),
        value_type,
        description: req.description,
    };
    state
        .set_property_use_case
        .run(cmd, ExecutionContext::from_auth(&auth.0))
        .await
        .into_result()?;
    let config = state
        .config_repo
        .find_by_key(&app, &section, &property, scope.as_str(), client_id)
        .await?
        .ok_or_else(|| PlatformError::internal("config missing after set"))?;
    // Go answers the writer with the stored row, unmasked.
    Ok(Json(state.response(config, true)))
}

/// Delete a property (Go `deleteConfigProperty`): 204, also when absent.
/// Go writes no event or audit row for it; neither does Rust's existing
/// property delete, which this mirrors.
#[utoipa::path(
    delete,
    path = "/api/config/{appCode}/{section}/{property}",
    tag = "platform-config",
    operation_id = "deleteConfigProperty",
    params(
        ("appCode" = String, Path, description = "Application code"),
        ("section" = String, Path, description = "Section"),
        ("property" = String, Path, description = "Property"),
        ("clientId" = Option<String>, Query, description = "Client for a CLIENT-scoped property")
    ),
    responses((status = 204, description = "Deleted, or already absent")),
    security(("bearer_auth" = []))
)]
pub async fn delete_config_property(
    State(state): State<GoPlatformConfigState>,
    auth: Authenticated,
    Path((app, section, property)): Path<(String, String, String)>,
    Query(q): Query<ClientIdQuery>,
) -> Result<StatusCode, PlatformError> {
    can_write_config_property(&state, &auth.0, &app).await?;
    require_application_access_if_registered(&state, &auth.0, &app).await?;
    let (scope, client_id) = coordinate(q.client_id.as_deref());
    state
        .config_repo
        .delete_by_key(&app, &section, &property, scope.as_str(), client_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The access grants for an application (Go `listPlatformConfigAccess`).
#[utoipa::path(
    get,
    path = "/api/platform-config/{app}/access",
    tag = "platform-config",
    operation_id = "listPlatformConfigAccess",
    params(("app" = String, Path, description = "Application code")),
    responses((status = 200, description = "Grants", body = AccessListResponse)),
    security(("bearer_auth" = []))
)]
pub async fn list_platform_config_access(
    State(state): State<GoPlatformConfigState>,
    auth: Authenticated,
    Path(app): Path<String>,
) -> Result<Json<AccessListResponse>, PlatformError> {
    checks::can_read_platform_config(&auth.0)?;
    require_application_access_if_registered(&state, &auth.0, &app).await?;
    let items = state.access_repo.find_by_application(&app).await?;
    Ok(Json(AccessListResponse {
        items: items.into_iter().map(AccessResponse::from).collect(),
    }))
}

/// Grant a role access (Go `grantPlatformConfigAccess`): an existing grant
/// for the role is updated in place; 201 `{id}` either way.
#[utoipa::path(
    post,
    path = "/api/platform-config/{app}/access",
    tag = "platform-config",
    operation_id = "grantPlatformConfigAccess",
    params(("app" = String, Path, description = "Application code")),
    request_body = GoGrantAccessRequest,
    responses((status = 201, description = "Granted", body = CreatedResponse)),
    security(("bearer_auth" = []))
)]
pub async fn grant_platform_config_access(
    State(state): State<GoPlatformConfigState>,
    auth: Authenticated,
    Path(app): Path<String>,
    Json(req): Json<GoGrantAccessRequest>,
) -> Result<(StatusCode, Json<CreatedResponse>), PlatformError> {
    checks::can_update_platform_config(&auth.0)?;
    require_application_access_if_registered(&state, &auth.0, &app).await?;
    if app.trim().is_empty() {
        return Err(PlatformError::bad_request_code(
            "APPLICATION_REQUIRED",
            "applicationCode is required",
        ));
    }
    if req.role_code.trim().is_empty() {
        return Err(PlatformError::bad_request_code(
            "ROLE_REQUIRED",
            "roleCode is required",
        ));
    }
    let cmd = GrantPlatformConfigAccessCommand {
        application_code: app,
        role_code: req.role_code,
        can_read: Some(true),
        can_write: Some(req.can_write),
    };
    let event = state
        .grant_access_use_case
        .run(cmd, ExecutionContext::from_auth(&auth.0))
        .await
        .into_result()?;
    Ok((
        StatusCode::CREATED,
        Json(CreatedResponse {
            id: event.access_id,
        }),
    ))
}

/// Revoke a grant by id (Go `revokePlatformConfigAccess`).
#[utoipa::path(
    delete,
    path = "/api/platform-config/access/{id}",
    tag = "platform-config",
    operation_id = "revokePlatformConfigAccess",
    params(("id" = String, Path, description = "Grant id")),
    responses(
        (status = 204, description = "Revoked"),
        (status = 404, description = "No such grant")
    ),
    security(("bearer_auth" = []))
)]
pub async fn revoke_platform_config_access(
    State(state): State<GoPlatformConfigState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<StatusCode, PlatformError> {
    checks::can_update_platform_config(&auth.0)?;
    let access = state
        .access_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found_code("PlatformConfigAccess", &id))?;
    require_application_access_if_registered(&state, &auth.0, &access.application_code).await?;
    state
        .revoke_access_use_case
        .run(
            RevokePlatformConfigAccessCommand {
                application_code: access.application_code,
                role_code: access.role_code,
            },
            ExecutionContext::from_auth(&auth.0),
        )
        .await
        .into_result()?;
    Ok(StatusCode::NO_CONTENT)
}

/// Full-path router; merged at the root.
pub fn go_platform_config_router(state: GoPlatformConfigState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(
            get_config_property,
            set_config_property,
            delete_config_property
        ))
        .routes(routes!(list_platform_config))
        .routes(routes!(
            list_platform_config_access,
            grant_platform_config_access
        ))
        .routes(routes!(revoke_platform_config_access))
        .with_state(state)
}
