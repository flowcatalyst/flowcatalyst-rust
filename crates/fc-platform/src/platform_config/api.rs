//! Platform Config Admin API

use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::entity::{ConfigScope, PlatformConfig};
use super::repository::PlatformConfigRepository;
use crate::shared::authorization_service::ApplicationAccessService;
use crate::shared::enum_str::parse_opt;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;

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
        Arc<super::operations::SetPlatformConfigPropertyUseCase<crate::usecase::PgUnitOfWork>>,
}

/// Go's property-route rule (platformconfig/api/api.go:47-57, 90-100,
/// 147-157, operations/set_property.go:60-73): an anchor caller passes; any
/// other caller needs a platform-config access grant on one of its roles
/// for the application, with read or write as asked, else 403.
async fn require_config_access(
    state: &PlatformConfigState,
    ctx: &crate::AuthContext,
    app_code: &str,
    write: bool,
) -> Result<(), PlatformError> {
    if ctx.is_anchor() {
        return Ok(());
    }
    let granted = if ctx.roles.is_empty() {
        false
    } else {
        state
            .access_repo
            .find_by_role_codes(app_code, &ctx.roles)
            .await?
            .iter()
            .any(|a| if write { a.can_write } else { a.can_read })
    };
    if granted {
        Ok(())
    } else if write {
        Err(PlatformError::forbidden(format!(
            "No write access to platform config for {}",
            app_code
        )))
    } else {
        Err(PlatformError::forbidden(format!(
            "No read access to platform config for {}",
            app_code
        )))
    }
}

/// Read a config property: anchor, or a read grant (Go).
async fn can_read_config(
    state: &PlatformConfigState,
    ctx: &crate::AuthContext,
    app_code: &str,
) -> Result<(), PlatformError> {
    require_config_access(state, ctx, app_code, false).await
}

/// Write a config property: anchor, or a write grant (Go).
async fn can_write_config(
    state: &PlatformConfigState,
    ctx: &crate::AuthContext,
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
            query.client_id.as_deref(),
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
            &app_code,
            &section,
            Some(scope_str),
            query.client_id.as_deref(),
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
            &app_code,
            &section,
            &property,
            scope_str,
            query.client_id.as_deref(),
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
        client_id: config.client_id,
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
) -> Result<(axum::http::StatusCode, Json<ConfigResponse>), PlatformError> {
    use crate::platform_config::operations::SetPlatformConfigPropertyCommand;
    use crate::usecase::{ExecutionContext, UseCase};

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
        client_id: query.client_id.clone(),
        value_type: parse_opt(req.value_type.as_deref())?,
        description: req.description,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    state
        .set_property_use_case
        .run(cmd, ctx)
        .await
        .into_result()?;

    let config = state
        .config_repo
        .find_by_key(
            &app_code,
            &section,
            &property,
            scope.as_str(),
            query.client_id.as_deref(),
        )
        .await?
        .ok_or_else(|| PlatformError::internal("Config set committed but row not found"))?;

    // 200 whether the property was created or updated, as Go
    // (platformconfig/api/api.go:36).
    Ok((
        axum::http::StatusCode::OK,
        Json(ConfigResponse::from_config(config)),
    ))
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
) -> Result<axum::http::StatusCode, PlatformError> {
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
            &app_code,
            &section,
            &property,
            scope_str,
            query.client_id.as_deref(),
        )
        .await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub fn admin_platform_config_router(state: PlatformConfigState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(list_configs))
        .routes(routes!(get_section))
        .routes(routes!(get_property, set_property, delete_property))
        .with_state(state)
}
