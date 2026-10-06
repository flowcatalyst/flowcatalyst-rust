//! Platform Config Admin API

use fc_platform_core::shared::id::ClientId;
use fc_platform_core::shared::id::PlatformConfigAccessId;
use std::collections::HashMap;
use std::sync::Arc;

use axum::http::StatusCode;
use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::access_api::{AccessListResponse, AccessResponse};
use super::access_repository::PlatformConfigAccessRepository;
use super::entity::ConfigValueType;
use super::entity::{ConfigScope, PlatformConfig};
use super::operations::{
    GrantPlatformConfigAccessCommand, GrantPlatformConfigAccessUseCase,
    RevokePlatformConfigAccessCommand, RevokePlatformConfigAccessUseCase,
    SetPlatformConfigPropertyCommand, SetPlatformConfigPropertyUseCase,
};
use super::repository::{PlatformConfigRepository, PropertyKey, SectionKey};
use crate::application::repository::ApplicationRepository;
use crate::shared::authorization_service::ApplicationAccessService;
use fc_platform_core::shared::api_common::CreatedResponse;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::shared::authorization_service::AuthContext;
use fc_platform_core::shared::encryption_service::{is_encrypted_ref, EncryptionService};
use fc_platform_core::shared::enum_str::parse_opt;
use fc_platform_core::shared::error::PlatformError;
use fc_platform_core::shared::middleware::Authenticated;
use fc_platform_core::usecase::{ExecutionContext, PgUnitOfWork, UseCase};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigQuery {
    pub scope: Option<String>,
    pub client_id: Option<String>,
}

impl ConfigQuery {
    /// The requested scope, if any; a value that isn't an exact scope is a 400.
    fn scope_filter(&self) -> Result<Option<ConfigScope>, PlatformError> {
        parse_opt(self.scope.as_deref())
    }

    /// The requested scope, defaulting to GLOBAL.
    fn scope_or_global(&self) -> Result<ConfigScope, PlatformError> {
        Ok(self.scope_filter()?.unwrap_or(ConfigScope::Global))
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetConfigRequest {
    pub value: String,
    pub value_type: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigResponse {
    pub id: String,
    pub application_code: String,
    pub section: String,
    pub property: String,
    pub scope: String,
    pub client_id: Option<String>,
    pub value_type: String,
    pub value: String,
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl ConfigResponse {
    fn from_config(c: PlatformConfig) -> Self {
        let value = c.masked_value().to_string();
        Self {
            id: c.id.into_string(),
            application_code: c.application_code,
            section: c.section,
            property: c.property,
            scope: c.scope.as_str().to_string(),
            client_id: c.client_id.map(ClientId::into_string),
            value_type: c.value_type.as_str().to_string(),
            value,
            description: c.description,
            created_at: c.created_at.to_rfc3339(),
            updated_at: c.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigListResponse {
    pub items: Vec<ConfigResponse>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSectionResponse {
    pub application_code: String,
    pub section: String,
    pub scope: String,
    pub client_id: Option<String>,
    pub values: HashMap<String, String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigValueResponse {
    pub application_code: String,
    pub section: String,
    pub property: String,
    pub scope: String,
    pub client_id: Option<String>,
    pub value: String,
}

#[derive(Clone)]
pub struct PlatformConfigState {
    pub config_repo: Arc<PlatformConfigRepository>,
    /// Role-based access grants, for non-anchor callers.
    pub access_repo: Arc<super::access_repository::PlatformConfigAccessRepository>,
    /// Resolves `{appCode}` and confines the caller to its applications.
    pub app_access: Arc<ApplicationAccessService>,
    pub set_property_use_case:
        Arc<super::operations::SetPlatformConfigPropertyUseCase<PgUnitOfWork>>,
}

/// Go's property-route rule ([`super::access::require_config_access`]).
async fn require_config_access(
    state: &PlatformConfigState,
    ctx: &AuthContext,
    app_code: &str,
    write: bool,
) -> Result<(), PlatformError> {
    super::access::require_config_access(
        &state.access_repo,
        ctx.is_anchor(),
        &ctx.roles,
        app_code,
        write,
    )
    .await
}

/// Read a config property: anchor, or a read grant (Go).
async fn can_read_config(
    state: &PlatformConfigState,
    ctx: &AuthContext,
    app_code: &str,
) -> Result<(), PlatformError> {
    require_config_access(state, ctx, app_code, false).await
}

/// Write a config property: anchor, or a write grant (Go).
async fn can_write_config(
    state: &PlatformConfigState,
    ctx: &AuthContext,
    app_code: &str,
) -> Result<(), PlatformError> {
    require_config_access(state, ctx, app_code, true).await
}

/// List all configs for an application
#[utoipa::path(
    get,
    path = "/{appCode}",
    tag = "platform-config",
    operation_id = "getApiConfigByAppCode",
    params(
        ("appCode" = String, Path, description = "Application code"),
        ("scope" = Option<String>, Query, description = "Config scope filter"),
        ("client_id" = Option<String>, Query, description = "Client ID filter")
    ),
    responses(
        (status = 200, description = "Config list", body = ConfigListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_configs(
    State(state): State<PlatformConfigState>,
    auth: Authenticated,
    Path(app_code): Path<String>,
    Query(query): Query<ConfigQuery>,
) -> Result<Json<ConfigListResponse>, PlatformError> {
    can_read_config(&state, &auth.0, &app_code).await?;
    state
        .app_access
        .require_application_access(&auth.0, &app_code)
        .await?;
    let items = state
        .config_repo
        .find_by_application(
            &app_code,
            query.scope_filter()?.map(|s| s.as_str()),
            query.client_id.as_deref().map(ClientId::from_wire).as_ref(),
        )
        .await?;
    Ok(Json(ConfigListResponse {
        items: items.into_iter().map(ConfigResponse::from_config).collect(),
    }))
}

/// Get config section for an application
#[utoipa::path(
    get,
    path = "/{appCode}/{section}",
    tag = "platform-config",
    operation_id = "getApiConfigByAppCodeBySection",
    params(
        ("appCode" = String, Path, description = "Application code"),
        ("section" = String, Path, description = "Config section"),
        ("scope" = Option<String>, Query, description = "Config scope filter"),
        ("client_id" = Option<String>, Query, description = "Client ID filter")
    ),
    responses(
        (status = 200, description = "Config section", body = ConfigSectionResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_section(
    State(state): State<PlatformConfigState>,
    auth: Authenticated,
    Path((app_code, section)): Path<(String, String)>,
    Query(query): Query<ConfigQuery>,
) -> Result<Json<ConfigSectionResponse>, PlatformError> {
    can_read_config(&state, &auth.0, &app_code).await?;
    state
        .app_access
        .require_application_access(&auth.0, &app_code)
        .await?;
    let scope_str = query.scope_or_global()?.as_str();
    let items = state
        .config_repo
        .find_by_section(
            &SectionKey {
                app_code: &app_code,
                section: &section,
            },
            Some(scope_str),
            query.client_id.as_deref().map(ClientId::from_wire).as_ref(),
        )
        .await?;
    let mut values = HashMap::new();
    for item in &items {
        values.insert(item.property.clone(), item.masked_value().to_string());
    }
    Ok(Json(ConfigSectionResponse {
        application_code: app_code,
        section,
        scope: scope_str.to_string(),
        client_id: query.client_id,
        values,
    }))
}

/// Get a specific config property
#[utoipa::path(
    get,
    path = "/{appCode}/{section}/{property}",
    tag = "platform-config",
    operation_id = "getApiConfigByAppCodeBySectionByProperty",
    params(
        ("appCode" = String, Path, description = "Application code"),
        ("section" = String, Path, description = "Config section"),
        ("property" = String, Path, description = "Config property"),
        ("scope" = Option<String>, Query, description = "Config scope filter"),
        ("client_id" = Option<String>, Query, description = "Client ID filter")
    ),
    responses(
        (status = 200, description = "Config value", body = ConfigValueResponse),
        (status = 404, description = "Config not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_property(
    State(state): State<PlatformConfigState>,
    auth: Authenticated,
    Path((app_code, section, property)): Path<(String, String, String)>,
    Query(query): Query<ConfigQuery>,
) -> Result<Json<ConfigValueResponse>, PlatformError> {
    can_read_config(&state, &auth.0, &app_code).await?;
    state
        .app_access
        .require_application_access(&auth.0, &app_code)
        .await?;
    let scope_str = query.scope_or_global()?.as_str();
    let config = state
        .config_repo
        .find_by_key(
            &PropertyKey {
                app_code: &app_code,
                section: &section,
                property: &property,
                scope: scope_str,
            },
            query.client_id.as_deref().map(ClientId::from_wire).as_ref(),
        )
        .await?
        .ok_or_else(|| {
            // Go's httperror.NotFound("Config", …) (platformconfig/api/api.go:107).
            PlatformError::not_found_code(
                "Config",
                format!("{}/{}/{}", app_code, section, property),
            )
        })?;

    let value = config.masked_value().to_string();
    Ok(Json(ConfigValueResponse {
        application_code: config.application_code,
        section: config.section,
        property: config.property,
        scope: config.scope.as_str().to_string(),
        client_id: config.client_id.map(ClientId::into_string),
        value,
    }))
}

/// Set (create or update) a config property
#[utoipa::path(
    put,
    path = "/{appCode}/{section}/{property}",
    tag = "platform-config",
    operation_id = "putApiConfigByAppCodeBySectionByProperty",
    params(
        ("appCode" = String, Path, description = "Application code"),
        ("section" = String, Path, description = "Config section"),
        ("property" = String, Path, description = "Config property"),
        ("scope" = Option<String>, Query, description = "Config scope filter"),
        ("client_id" = Option<String>, Query, description = "Client ID filter")
    ),
    request_body = SetConfigRequest,
    responses(
        (status = 200, description = "Config created or updated", body = ConfigResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn set_property(
    State(state): State<PlatformConfigState>,
    auth: Authenticated,
    Path((app_code, section, property)): Path<(String, String, String)>,
    Query(query): Query<ConfigQuery>,
    Json(req): Json<SetConfigRequest>,
) -> Result<(StatusCode, Json<ConfigResponse>), PlatformError> {
    use crate::platform_config::operations::SetPlatformConfigPropertyCommand;
    use fc_platform_core::usecase::{ExecutionContext, UseCase};

    can_write_config(&state, &auth.0, &app_code).await?;
    state
        .app_access
        .require_application_access(&auth.0, &app_code)
        .await?;
    let scope = query.scope_or_global()?;

    let cmd = SetPlatformConfigPropertyCommand {
        application_code: app_code.clone(),
        section: section.clone(),
        property: property.clone(),
        value: req.value,
        scope,
        client_id: query.client_id.clone().map(ClientId::from_wire),
        value_type: parse_opt(req.value_type.as_deref())?,
        description: req.description,
    };
    let ctx = ExecutionContext::from_auth(&auth.0);
    state
        .set_property_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;

    let config = state
        .config_repo
        .find_by_key(
            &PropertyKey {
                app_code: &app_code,
                section: &section,
                property: &property,
                scope: scope.as_str(),
            },
            query.client_id.as_deref().map(ClientId::from_wire).as_ref(),
        )
        .await?
        .ok_or_else(|| PlatformError::internal("Config set committed but row not found"))?;

    // 200 whether the property was created or updated, as Go
    // (platformconfig/api/api.go:36).
    Ok((StatusCode::OK, Json(ConfigResponse::from_config(config))))
}

/// Delete a config property
#[utoipa::path(
    delete,
    path = "/{appCode}/{section}/{property}",
    tag = "platform-config",
    operation_id = "deleteApiConfigByAppCodeBySectionByProperty",
    params(
        ("appCode" = String, Path, description = "Application code"),
        ("section" = String, Path, description = "Config section"),
        ("property" = String, Path, description = "Config property"),
        ("scope" = Option<String>, Query, description = "Config scope filter"),
        ("client_id" = Option<String>, Query, description = "Client ID filter")
    ),
    responses(
        (status = 204, description = "Config deleted, or already absent")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_property(
    State(state): State<PlatformConfigState>,
    auth: Authenticated,
    Path((app_code, section, property)): Path<(String, String, String)>,
    Query(query): Query<ConfigQuery>,
) -> Result<StatusCode, PlatformError> {
    can_write_config(&state, &auth.0, &app_code).await?;
    state
        .app_access
        .require_application_access(&auth.0, &app_code)
        .await?;
    let scope_str = query.scope_or_global()?.as_str();
    // Idempotent: 204 whether or not the property existed, as Go
    // (platformconfig/api/api.go:163-165).
    state
        .config_repo
        .delete_by_key(
            &PropertyKey {
                app_code: &app_code,
                section: &section,
                property: &property,
                scope: scope_str,
            },
            query.client_id.as_deref().map(ClientId::from_wire).as_ref(),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── Go-parity routes (formerly go_api.rs) ────────────────────────────────────
//
// Platform-config routes as Go serves them (`platformconfig/api/api.go:32-40`):
//
// - `GET|PUT|DELETE /api/config/{app}/{section}/{property}` (`?clientId=`)
// - `GET    /api/platform-config/{app}`          → `{items}`
// - `GET    /api/platform-config/{app}/access`   → `{items}`
// - `POST   /api/platform-config/{app}/access`   → 201 `{id}`
// - `DELETE /api/platform-config/access/{id}`    → 204
//
// The property routes follow Go's gate: an anchor passes, anyone else needs
// a platform-config access grant (read or write) on one of its roles for
// the application. The application need not be registered, as in Go; a
// registered one must be in the caller's application scope (Rust's rule,
// 404 otherwise). Scope is derived as Go's: `CLIENT` when `clientId` is given,
// otherwise `GLOBAL`. A `SECRET` value reads as `***` to a non-anchor.
// Rust keeps encrypting secrets at rest; an anchor read decrypts them (a
// Go-written plaintext row reads as stored).
//
// The access-grant routes are Go's `anchorWith(platform:admin:config:*)`.

#[derive(Clone)]
pub struct GoPlatformConfigState {
    pub config_repo: Arc<PlatformConfigRepository>,
    pub access_repo: Arc<PlatformConfigAccessRepository>,
    pub encryption: Option<Arc<EncryptionService>>,
    pub set_property_use_case: Arc<SetPlatformConfigPropertyUseCase<PgUnitOfWork>>,
    pub grant_access_use_case: Arc<GrantPlatformConfigAccessUseCase<PgUnitOfWork>>,
    pub revoke_access_use_case: Arc<RevokePlatformConfigAccessUseCase<PgUnitOfWork>>,
    pub application_repo: Arc<ApplicationRepository>,
    pub app_access: Arc<ApplicationAccessService>,
}

/// Go `ConfigResponse`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = ConfigResponse)]
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
    #[schema(format = DateTime)]
    pub created_at: String,
    #[schema(format = DateTime)]
    pub updated_at: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = ConfigListResponse)]
pub struct GoConfigListResponse {
    pub items: Vec<GoConfigResponse>,
}

/// Go `SetPropertyRequest`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = SetPropertyRequest)]
pub struct GoSetPropertyRequest {
    pub value: String,
    pub value_type: Option<String>,
    pub description: Option<String>,
    pub client_id: Option<String>,
}

/// Go `GrantAccessRequest`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = GrantAccessRequest)]
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
fn coordinate(client_id: Option<&str>) -> (ConfigScope, Option<ClientId>) {
    match client_id.filter(|c| !c.is_empty()) {
        Some(c) => (ConfigScope::Client, Some(ClientId::from_wire(c))),
        None => (ConfigScope::Global, None),
    }
}

/// Go's property-route gate ([`super::access::require_config_access`]).
async fn require_property_access(
    state: &GoPlatformConfigState,
    ctx: &AuthContext,
    app: &str,
    write: bool,
) -> Result<(), PlatformError> {
    super::access::require_config_access(
        &state.access_repo,
        ctx.is_anchor(),
        &ctx.roles,
        app,
        write,
    )
    .await
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
            id: c.id.into_string(),
            application_code: c.application_code,
            section: c.section,
            property: c.property,
            scope: c.scope.as_str().to_string(),
            client_id: c.client_id.map(ClientId::into_string),
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
    operation_id = "listPlatformConfigProperties",
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
    path = "/api/config/{app}/{section}/{property}",
    tag = "platform-config",
    operation_id = "getPlatformConfigProperty",
    params(
        ("app" = String, Path, description = "Application code"),
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
        .find_by_key(
            &PropertyKey {
                app_code: &app,
                section: &section,
                property: &property,
                scope: scope.as_str(),
            },
            client_id.as_ref(),
        )
        .await?
        .ok_or_else(|| {
            PlatformError::not_found_code("Config", format!("{app}/{section}/{property}"))
        })?;
    Ok(Json(state.response(config, auth.0.is_anchor())))
}

/// Create or update a property (Go `setConfigProperty`): 200 with the row.
#[utoipa::path(
    put,
    path = "/api/config/{app}/{section}/{property}",
    tag = "platform-config",
    operation_id = "setPlatformConfigProperty",
    params(
        ("app" = String, Path, description = "Application code"),
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
        client_id: client_id.clone(),
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
        .find_by_key(
            &PropertyKey {
                app_code: &app,
                section: &section,
                property: &property,
                scope: scope.as_str(),
            },
            client_id.as_ref(),
        )
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
    path = "/api/config/{app}/{section}/{property}",
    tag = "platform-config",
    operation_id = "deletePlatformConfigProperty",
    params(
        ("app" = String, Path, description = "Application code"),
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
        .delete_by_key(
            &PropertyKey {
                app_code: &app,
                section: &section,
                property: &property,
                scope: scope.as_str(),
            },
            client_id.as_ref(),
        )
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
            id: event.access_id.into_string(),
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
    let id = PlatformConfigAccessId::from_wire(id);
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
