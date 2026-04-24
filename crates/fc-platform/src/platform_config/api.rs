//! Platform Config Admin API

use axum::{
    extract::{State, Path, Query},
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use utoipa::ToSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::collections::HashMap;

use super::entity::PlatformConfig;
use super::repository::PlatformConfigRepository;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigQuery {
    pub scope: Option<String>,
    pub client_id: Option<String>,
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
    pub set_property_use_case: Arc<
        super::operations::SetPlatformConfigPropertyUseCase<crate::usecase::PgUnitOfWork>,
    >,
}

/// List all configs for an application
#[utoipa::path(
    get,
    path = "/{app_code}",
    tag = "platform-config",
    operation_id = "getApiAdminConfigByAppCode",
    params(
        ("app_code" = String, Path, description = "Application code"),
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
    _auth: Authenticated,
    Path(app_code): Path<String>,
    Query(query): Query<ConfigQuery>,
) -> Result<Json<ConfigListResponse>, PlatformError> {
    let items = state.config_repo.find_by_application(
        &app_code,
        query.scope.as_deref(),
        query.client_id.as_deref(),
    ).await?;
    Ok(Json(ConfigListResponse {
        items: items.into_iter().map(ConfigResponse::from_config).collect(),
    }))
}

/// Get config section for an application
#[utoipa::path(
    get,
    path = "/{app_code}/{section}",
    tag = "platform-config",
    operation_id = "getApiAdminConfigByAppCodeBySection",
    params(
        ("app_code" = String, Path, description = "Application code"),
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
    _auth: Authenticated,
    Path((app_code, section)): Path<(String, String)>,
    Query(query): Query<ConfigQuery>,
) -> Result<Json<ConfigSectionResponse>, PlatformError> {
    let scope_str = query.scope.as_deref().unwrap_or("GLOBAL");
    let items = state.config_repo.find_by_section(
        &app_code,
        &section,
        Some(scope_str),
        query.client_id.as_deref(),
    ).await?;
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
    path = "/{app_code}/{section}/{property}",
    tag = "platform-config",
    operation_id = "getApiAdminConfigByAppCodeBySectionByProperty",
    params(
        ("app_code" = String, Path, description = "Application code"),
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
    _auth: Authenticated,
    Path((app_code, section, property)): Path<(String, String, String)>,
    Query(query): Query<ConfigQuery>,
) -> Result<Json<ConfigValueResponse>, PlatformError> {
    let scope_str = query.scope.as_deref().unwrap_or("GLOBAL");
    let config = state.config_repo.find_by_key(
        &app_code, &section, &property, scope_str, query.client_id.as_deref(),
    ).await?
        .ok_or_else(|| PlatformError::not_found("PlatformConfig", &format!("{}/{}/{}", app_code, section, property)))?;

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
    path = "/{app_code}/{section}/{property}",
    tag = "platform-config",
    operation_id = "putApiAdminConfigByAppCodeBySectionByProperty",
    params(
        ("app_code" = String, Path, description = "Application code"),
        ("section" = String, Path, description = "Config section"),
        ("property" = String, Path, description = "Config property"),
        ("scope" = Option<String>, Query, description = "Config scope filter"),
        ("client_id" = Option<String>, Query, description = "Client ID filter")
    ),
    request_body = SetConfigRequest,
    responses(
        (status = 200, description = "Config updated", body = ConfigResponse),
        (status = 201, description = "Config created", body = ConfigResponse)
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

    crate::checks::require_anchor(&auth.0)?;
    let scope_str = query.scope.as_deref().unwrap_or("GLOBAL").to_string();

    let cmd = SetPlatformConfigPropertyCommand {
        application_code: app_code.clone(),
        section: section.clone(),
        property: property.clone(),
        value: req.value,
        scope: scope_str.clone(),
        client_id: query.client_id.clone(),
        value_type: req.value_type,
        description: req.description,
    };
    let ctx = ExecutionContext::create(&auth.0.principal_id);
    let event = state.set_property_use_case.run(cmd, ctx).await.into_result()?;

    let config = state.config_repo.find_by_key(
        &app_code, &section, &property, &scope_str, query.client_id.as_deref(),
    ).await?
        .ok_or_else(|| PlatformError::internal("Config set committed but row not found"))?;

    let status = if event.was_created {
        axum::http::StatusCode::CREATED
    } else {
        axum::http::StatusCode::OK
    };
    Ok((status, Json(ConfigResponse::from_config(config))))
}

/// Delete a config property
#[utoipa::path(
    delete,
    path = "/{app_code}/{section}/{property}",
    tag = "platform-config",
    operation_id = "deleteApiAdminConfigByAppCodeBySectionByProperty",
    params(
        ("app_code" = String, Path, description = "Application code"),
        ("section" = String, Path, description = "Config section"),
        ("property" = String, Path, description = "Config property"),
        ("scope" = Option<String>, Query, description = "Config scope filter"),
        ("client_id" = Option<String>, Query, description = "Client ID filter")
    ),
    responses(
        (status = 204, description = "Config deleted"),
        (status = 404, description = "Config not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_property(
    State(state): State<PlatformConfigState>,
    auth: Authenticated,
    Path((app_code, section, property)): Path<(String, String, String)>,
    Query(query): Query<ConfigQuery>,
) -> Result<axum::http::StatusCode, PlatformError> {
    crate::checks::require_anchor(&auth.0)?;
    let scope_str = query.scope.as_deref().unwrap_or("GLOBAL");
    let deleted = state.config_repo.delete_by_key(
        &app_code, &section, &property, scope_str, query.client_id.as_deref(),
    ).await?;
    if deleted {
        Ok(axum::http::StatusCode::NO_CONTENT)
    } else {
        Err(PlatformError::not_found("PlatformConfig", &format!("{}/{}/{}", app_code, section, property)))
    }
}

pub fn admin_platform_config_router(state: PlatformConfigState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(list_configs))
        .routes(routes!(get_section))
        .routes(routes!(get_property, set_property, delete_property))
        .with_state(state)
}
