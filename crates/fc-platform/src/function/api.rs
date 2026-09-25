//! `/api/functions*` and `/api/function-pools` (Java
//! `function/api/FunctionApi.java`, `register` lines 178-206). Versions,
//! the manifest check and artifact upload are in [`super::version_api`];
//! aliases are P5.
//!
//! Each write handler is a permission check, a command built from the body,
//! a use case run, and a response. Reads go to the repositories and apply
//! reach themselves: a function out of reach is `404 Function_NOT_FOUND`,
//! never a 403 (the one [`Caller::can_reach`] the use cases use too).
//!
//! Every write handler calls `checks::require_permission` (Java
//! `Checks.require`: `403 PERMISSION_REQUIRED`).

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::domain_repository::FunctionDomainRepository;
use super::entity::{Function, FunctionStatus, FunctionVersion, SecretValue};
use super::host_repository::FunctionHostRepository;
use super::operations::{
    access::function_by_address, Caller, CreateCommand, DeleteCommand, DeleteSecretCommand,
    FunctionOperations, SetConfigCommand, SetSecretCommand, UpdateCommand,
};
use super::policy_repository::ClientPolicyRepository;
use super::repository::{FunctionListFilter, FunctionRepository};
use super::route_repository::FunctionRouteRepository;
use super::settings_repository::FunctionSettingsRepository;
use super::trigger_object_repository::TriggerObjectRepository;
use super::version_repository::FunctionVersionRepository;
use super::wire::{micros_ser, parse_body};
use super::{
    parse_address, FunctionAddress, FunctionAddressPattern, FunctionLimits, FunctionOwner,
};
use crate::permissions::function::{FUNCTION_MANAGE, FUNCTION_SECRET_MANAGE, FUNCTION_VIEW};
use crate::shared::api_common::PaginatedResponse;
use crate::shared::authorization_service::{checks, ApplicationAccessService, AuthContext};
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::usecase::{ExecutionContext, PgUnitOfWork, UseCase, UseCaseError};

// ── State ───────────────────────────────────────────────────────────────────

/// Everything the function, policy and domain routes read, plus the use
/// cases they run.
#[derive(Clone)]
pub struct FunctionsState {
    pub functions: Arc<FunctionRepository>,
    pub versions: Arc<FunctionVersionRepository>,
    pub hosts: Arc<FunctionHostRepository>,
    pub settings: Arc<FunctionSettingsRepository>,
    pub policies: Arc<ClientPolicyRepository>,
    pub domains: Arc<FunctionDomainRepository>,
    pub routes: Arc<FunctionRouteRepository>,
    pub trigger_objects: Arc<TriggerObjectRepository>,
    pub app_access: Arc<ApplicationAccessService>,
    /// The platform defaults a policy's absent ceilings resolve to.
    pub limits: FunctionLimits,
    pub ops: FunctionOperations<PgUnitOfWork>,
}

impl FunctionsState {
    /// The caller with its application scope resolved (cached per principal).
    pub(crate) async fn caller(&self, auth: &AuthContext) -> Result<Caller, PlatformError> {
        let applications = self.app_access.scope_for(&auth.principal_id).await?;
        Ok(Caller::new(auth.clone(), applications))
    }
}

// ── Shared handler helpers ──────────────────────────────────────────────────

/// A query string's parameters, in order; repeated names are all kept.
pub(crate) type QueryParams = Vec<(String, String)>;

/// Java `ctx.queryParam(name)` read as the function API does: the first
/// value, with an empty one treated as absent.
pub(crate) fn query_param<'a>(params: &'a QueryParams, name: &str) -> Option<&'a str> {
    params
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
        .filter(|v| !v.is_empty())
}

/// `{address}` is one path segment that contains dots; a malformed one is
/// `400 ADDRESS_INVALID`, never a 404.
pub(crate) fn address_from_path(raw: &str) -> Result<FunctionAddress, PlatformError> {
    Ok(parse_address(raw)?)
}

/// Load-or-404, out-of-reach-or-404, for a read.
pub(crate) async fn reachable_function(
    state: &FunctionsState,
    address: &FunctionAddress,
    caller: &Caller,
) -> Result<Function, PlatformError> {
    Ok(function_by_address(&state.functions, address, caller).await?)
}

/// Java's `503 ENCRYPTION_UNCONFIGURED` on every secret route when no app
/// key is configured: the one place a function route answers 503.
fn require_encryption(state: &FunctionsState) -> Result<(), PlatformError> {
    if state.settings.encryption_configured() {
        return Ok(());
    }
    Err(PlatformError::Coded {
        status: StatusCode::SERVICE_UNAVAILABLE,
        code: "ENCRYPTION_UNCONFIGURED".to_string(),
        message: "FLOWCATALYST_APP_KEY is not configured; function secrets are unavailable"
            .to_string(),
        details: Default::default(),
    })
}

/// `{v}` or `?version=`: a positive integer, else `400 VERSION_INVALID`.
pub(crate) fn parse_version_number(raw: &str) -> Result<i32, UseCaseError> {
    raw.parse::<i32>().ok().filter(|v| *v > 0).ok_or_else(|| {
        UseCaseError::validation("VERSION_INVALID", "version must be a positive integer")
    })
}

/// Java `PageQuery.from`: `page` (0-based) and the first positive of
/// `size`, `limit`, `pageSize`, `page_size` (capped at 1000, default 20).
/// A value that is not an integer is `400 VALIDATION`, every bad parameter
/// listed in `details.errors`.
fn page_query(params: &QueryParams) -> Result<(u32, u32), PlatformError> {
    const MAX_PAGE_SIZE: i32 = 1000;
    const DEFAULT_PAGE_SIZE: i32 = 20;
    let mut errors = Vec::new();
    let mut int = |name: &str| -> i32 {
        match query_param(params, name) {
            None => 0,
            Some(raw) => match raw.trim_matches(|c: char| c <= ' ').parse::<i32>() {
                Ok(v) => v,
                Err(_) => {
                    errors.push(serde_json::json!({
                        "message": "invalid integer",
                        "location": format!("query.{name}"),
                        "value": raw,
                    }));
                    0
                }
            },
        }
    };
    let page = int("page");
    let sizes = [int("size"), int("limit"), int("pageSize"), int("page_size")];
    if !errors.is_empty() {
        let mut details = std::collections::HashMap::new();
        details.insert("errors".to_string(), serde_json::Value::Array(errors));
        return Err(PlatformError::Coded {
            status: StatusCode::BAD_REQUEST,
            code: "VALIDATION".to_string(),
            message: "validation failed".to_string(),
            details,
        });
    }
    let size = sizes
        .into_iter()
        .find(|v| *v > 0)
        .map(|v| v.min(MAX_PAGE_SIZE))
        .unwrap_or(DEFAULT_PAGE_SIZE);
    Ok((page.max(0) as u32, size as u32))
}

// ── DTOs ────────────────────────────────────────────────────────────────────

/// Body of `POST /api/functions`.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateFunctionRequest {
    pub application_code: Option<String>,
    pub service_name: Option<String>,
    pub name: Option<String>,
    /// `jvm` or `wasm`, in any case.
    pub runtime: Option<String>,
    pub description: Option<String>,
    /// Absent or blank: a platform-owned function.
    pub client_id: Option<String>,
}

/// Body of `PUT /api/functions/{address}`: only these two fields exist.
/// `serviceName`, `name`, `applicationCode`, `clientId` and `runtime` are
/// refused with `400 FUNCTION_IMMUTABLE_FIELD`.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateFunctionRequest {
    pub description: Option<String>,
    /// `ACTIVE` or `DISABLED`.
    pub status: Option<String>,
}

/// The function response (Java `FunctionResponse`). `live` is absent until
/// a version is promoted.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FunctionResponse {
    pub id: String,
    pub address: String,
    pub application_code: String,
    pub service_name: String,
    pub name: String,
    pub application_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// `jvm` or `wasm`.
    pub runtime: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `ACTIVE` or `DISABLED`.
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live: Option<LiveResponse>,
    #[serde(serialize_with = "micros_ser")]
    pub created_at: DateTime<Utc>,
    #[serde(serialize_with = "micros_ser")]
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LiveResponse {
    pub version: i32,
    pub version_id: String,
}

impl FunctionResponse {
    pub fn from(f: &Function, live: Option<&FunctionVersion>) -> Self {
        Self {
            id: f.id.clone(),
            address: f.address.render(),
            application_code: f.address.application().to_string(),
            service_name: f.address.service().to_string(),
            name: f.address.name().to_string(),
            application_id: f.application_id.clone(),
            client_id: f.owner.client_id_or_none().map(str::to_string),
            runtime: f.runtime.wire_value().to_string(),
            description: f.description.clone(),
            status: f.status.as_str().to_string(),
            live: live.map(|v| LiveResponse {
                version: v.version,
                version_id: v.id.clone(),
            }),
            created_at: f.created_at,
            updated_at: f.updated_at,
        }
    }
}

/// `GET /api/functions` query (documentation; read by [`query_param`]).
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[allow(dead_code)]
pub struct FunctionListQuery {
    /// `app.service.name`, `app.service.*` or `app.*`.
    address: Option<String>,
    /// A client id, or `platform` for platform-owned functions.
    #[serde(rename = "clientId")]
    #[param(rename = "clientId")]
    client_id: Option<String>,
    /// `ACTIVE` or `DISABLED`.
    status: Option<String>,
    /// 0-based page index.
    page: Option<i32>,
    /// Page size (default 20, at most 1000).
    size: Option<i32>,
}

/// `GET /api/functions/{address}/status`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatusResponse {
    pub address: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live: Option<StatusLive>,
    pub versions: Vec<VersionSummary>,
    /// Only the hosts reporting this address, and only their entries for it.
    pub hosts: Vec<HostSummary>,
    pub wiring: Vec<WiringEntry>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct StatusLive {
    pub version: i32,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct VersionSummary {
    pub version: i32,
    pub state: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HostSummary {
    pub host_id: String,
    pub pool: String,
    pub state: String,
    #[serde(serialize_with = "micros_ser")]
    pub last_heartbeat: DateTime<Utc>,
    /// Not heartbeated within the last 45 s.
    pub stale: bool,
    pub loaded: Vec<LoadedSummary>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct LoadedSummary {
    pub version: i32,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// One `fn_trigger_objects` row; `present` is false when the linked object
/// was deleted by hand.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WiringEntry {
    pub kind: String,
    pub code: String,
    pub object_id: String,
    pub present: bool,
}

/// One entry of `GET /api/function-pools`.
#[derive(Debug, Serialize, ToSchema)]
pub struct PoolSummaryResponse {
    pub pool: String,
    pub hosts: i64,
}

/// One manifest that contributed to `declared`: that manifest's own full key
/// list.
#[derive(Debug, Serialize, ToSchema)]
pub struct DeclaredByEntry {
    pub version: i32,
    pub keys: Vec<String>,
}

/// `GET`/`PUT …/config`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigResponse {
    /// The whole map, keys in order.
    pub values: BTreeMap<String, String>,
    pub declared: Vec<String>,
    pub missing: Vec<String>,
    pub declared_by: Vec<DeclaredByEntry>,
}

/// Body of `PUT …/config`: a full replacement.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct SetConfigRequest {
    pub values: Option<BTreeMap<String, String>>,
}

/// `GET …/secrets`: keys and metadata, never a value.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SecretListResponse {
    pub keys: Vec<SecretKeyResponse>,
    pub declared: Vec<String>,
    pub missing: Vec<String>,
    pub declared_by: Vec<DeclaredByEntry>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SecretKeyResponse {
    pub key: String,
    #[serde(serialize_with = "micros_ser")]
    pub updated_at: DateTime<Utc>,
    pub updated_by: String,
}

/// Body of `PUT …/secrets/{key}`.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct SetSecretRequest {
    pub value: Option<String>,
}

/// `?version=` on the config and secrets reads.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[allow(dead_code)]
pub struct DeclaredQuery {
    /// Whose manifest `declared` is read from besides live's; absent means
    /// the newest non-retired version.
    version: Option<i32>,
}

// ── Handlers: functions ─────────────────────────────────────────────────────

/// List functions, reach-filtered in SQL.
#[utoipa::path(
    get, path = "/api/functions", tag = "functions",
    operation_id = "getApiFunctions",
    params(FunctionListQuery),
    responses(
        (status = 200, body = PaginatedResponse<FunctionResponse>),
        (status = 400, description = "VALIDATION, ADDRESS_PATTERN_INVALID or STATUS_INVALID"),
        (status = 403, description = "PERMISSION_REQUIRED"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_functions(
    State(state): State<FunctionsState>,
    auth: Authenticated,
    Query(params): Query<QueryParams>,
) -> Result<Json<PaginatedResponse<FunctionResponse>>, PlatformError> {
    checks::require_permission(&auth.0, FUNCTION_VIEW)?;
    let (page, size) = page_query(&params)?;
    let pattern = query_param(&params, "address")
        .map(FunctionAddressPattern::parse)
        .transpose()?;
    let owner = query_param(&params, "clientId")
        .map(|raw| {
            FunctionOwner::from_wire(raw)
                .map_err(|e| PlatformError::bad_request_code("CLIENT_ID_INVALID", e.to_string()))
        })
        .transpose()?;
    let status = query_param(&params, "status")
        .map(|raw| {
            raw.parse::<FunctionStatus>().map_err(|_| {
                UseCaseError::validation("STATUS_INVALID", "status must be ACTIVE or DISABLED")
            })
        })
        .transpose()?;
    let caller = state.caller(&auth.0).await?;
    let filter = FunctionListFilter {
        pattern,
        owner,
        status,
        owners: caller.owner_reach(),
        applications: caller.application_reach(),
    };
    let offset = page as i64 * size as i64;
    let (rows, total) = tokio::try_join!(
        state
            .functions
            .find_with_filters(&filter, size as i64, offset),
        state.functions.count_with_filters(&filter),
    )?;
    // One batch read for every row's live version; a corrupt one is simply
    // absent, never a failed list.
    let live_ids: Vec<String> = rows
        .iter()
        .filter_map(|f| f.live_version_id().map(str::to_string))
        .collect();
    let live = state.versions.find_by_ids(&live_ids).await?;
    let data = rows
        .iter()
        .map(|f| FunctionResponse::from(f, f.live_version_id().and_then(|id| live.get(id))))
        .collect();
    Ok(Json(PaginatedResponse::new(
        data,
        page,
        size,
        total.max(0) as u64,
    )))
}

/// Create a function.
#[utoipa::path(
    post, path = "/api/functions", tag = "functions",
    operation_id = "postApiFunctions",
    request_body = CreateFunctionRequest,
    responses(
        (status = 201, body = FunctionResponse),
        (status = 400), (status = 403), (status = 404), (status = 409, description = "FUNCTION_EXISTS"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_function(
    State(state): State<FunctionsState>,
    auth: Authenticated,
    body: Bytes,
) -> Result<(StatusCode, Json<FunctionResponse>), PlatformError> {
    checks::require_permission(&auth.0, FUNCTION_MANAGE)?;
    let req: CreateFunctionRequest = parse_body(&body)?;
    let command = CreateCommand {
        application_code: req.application_code,
        service_name: req.service_name,
        name: req.name,
        runtime: req.runtime,
        description: req.description,
        client_id: req.client_id,
    };
    let caller = state.caller(&auth.0).await?;
    let event = state
        .ops
        .create(caller)
        .run(command, ExecutionContext::from_auth(&auth.0))
        .await
        .into_result()?;
    let f = state
        .functions
        .find_by_id(&event.function_id)
        .await?
        .ok_or_else(|| PlatformError::internal("REPO: function created but row not found"))?;
    // A brand-new function has no version yet.
    Ok((StatusCode::CREATED, Json(FunctionResponse::from(&f, None))))
}

/// Get one function.
#[utoipa::path(
    get, path = "/api/functions/{address}", tag = "functions",
    operation_id = "getApiFunctionsByAddress",
    params(("address" = String, Path, description = "app.service.name")),
    responses(
        (status = 200, body = FunctionResponse),
        (status = 400, description = "ADDRESS_INVALID"),
        (status = 404, description = "Function_NOT_FOUND, also when out of reach"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_function(
    State(state): State<FunctionsState>,
    auth: Authenticated,
    Path(address): Path<String>,
) -> Result<Json<FunctionResponse>, PlatformError> {
    checks::require_permission(&auth.0, FUNCTION_VIEW)?;
    let address = address_from_path(&address)?;
    let caller = state.caller(&auth.0).await?;
    let f = reachable_function(&state, &address, &caller).await?;
    let live = match f.live_version_id() {
        Some(id) => state.versions.find_by_id(id).await?,
        None => None,
    };
    Ok(Json(FunctionResponse::from(&f, live.as_ref())))
}

/// The fields a `PUT` body may never carry, in the order they are checked.
const IMMUTABLE_FIELDS: [&str; 5] = [
    "serviceName",
    "name",
    "applicationCode",
    "clientId",
    "runtime",
];

/// Update `description` and/or `status`. The raw body is checked for an
/// immutable field before binding, which would otherwise drop it silently.
#[utoipa::path(
    put, path = "/api/functions/{address}", tag = "functions",
    operation_id = "putApiFunctionsByAddress",
    params(("address" = String, Path, description = "app.service.name")),
    request_body = UpdateFunctionRequest,
    responses(
        (status = 204),
        (status = 400, description = "FUNCTION_IMMUTABLE_FIELD, STATUS_INVALID, ADDRESS_INVALID"),
        (status = 404), (status = 409, description = "FUNCTION_ALREADY_ACTIVE / FUNCTION_ALREADY_DISABLED"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_function(
    State(state): State<FunctionsState>,
    auth: Authenticated,
    Path(address): Path<String>,
    body: Bytes,
) -> Result<StatusCode, PlatformError> {
    checks::require_permission(&auth.0, FUNCTION_MANAGE)?;
    let address = address_from_path(&address)?;
    let tree: serde_json::Value = parse_body(&body)?;
    if let Some(field) = IMMUTABLE_FIELDS
        .iter()
        .find(|f| tree.as_object().is_some_and(|o| o.contains_key(**f)))
    {
        return Err(UseCaseError::validation(
            "FUNCTION_IMMUTABLE_FIELD",
            format!("field '{field}' cannot be changed after creation"),
        )
        .into());
    }
    let req: UpdateFunctionRequest =
        serde_json::from_value(tree).map_err(|e| super::wire::invalid_json(&e))?;
    let command = UpdateCommand {
        address,
        description: req.description,
        status: req.status,
    };
    let caller = state.caller(&auth.0).await?;
    state
        .ops
        .update(caller)
        .run(command, ExecutionContext::from_auth(&auth.0))
        .await
        .into_result()?;
    Ok(StatusCode::NO_CONTENT)
}

/// Delete a function; its versions, aliases, routes and settings cascade.
#[utoipa::path(
    delete, path = "/api/functions/{address}", tag = "functions",
    operation_id = "deleteApiFunctionsByAddress",
    params(("address" = String, Path, description = "app.service.name")),
    responses((status = 204), (status = 400), (status = 403), (status = 404)),
    security(("bearer_auth" = []))
)]
pub async fn delete_function(
    State(state): State<FunctionsState>,
    auth: Authenticated,
    Path(address): Path<String>,
) -> Result<StatusCode, PlatformError> {
    checks::require_permission(&auth.0, FUNCTION_MANAGE)?;
    let address = address_from_path(&address)?;
    let caller = state.caller(&auth.0).await?;
    let event = state
        .ops
        .delete(caller)
        .run(
            DeleteCommand { address },
            ExecutionContext::from_auth(&auth.0),
        )
        .await
        .into_result()?;
    // After the commit, best-effort (Java FunctionApi.java:639-648): the
    // function's uploaded blobs are garbage now, and a store failure is a
    // WARN, never a failed delete.
    if let Some(store) = &state.ops.artifacts {
        if let Err(e) = store.delete_all(&event.function_id).await {
            tracing::warn!(id = %event.function_id, error = %e, "deleting a deleted function's artifacts failed");
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

/// What exists for one function now: its versions, the hosts reporting it
/// and its wiring. Hosts and wiring stay empty until the host control plane
/// (P6) and promote (P5) write them.
#[utoipa::path(
    get, path = "/api/functions/{address}/status", tag = "functions",
    operation_id = "getApiFunctionsByAddressStatus",
    params(("address" = String, Path, description = "app.service.name")),
    responses((status = 200, body = StatusResponse), (status = 400), (status = 403), (status = 404)),
    security(("bearer_auth" = []))
)]
pub async fn function_status(
    State(state): State<FunctionsState>,
    auth: Authenticated,
    Path(address): Path<String>,
) -> Result<Json<StatusResponse>, PlatformError> {
    checks::require_permission(&auth.0, FUNCTION_VIEW)?;
    let address = address_from_path(&address)?;
    let caller = state.caller(&auth.0).await?;
    let f = reachable_function(&state, &address, &caller).await?;
    let (versions, hosts, wiring) = tokio::try_join!(
        state.versions.list_by_function(&f.id),
        state.hosts.list_reporting(&address),
        state.trigger_objects.list_by_function(&f.id),
    )?;
    let live = f
        .live_version_id()
        .and_then(|id| versions.iter().find(|v| v.id == id))
        .map(|v| StatusLive { version: v.version });
    let stale_before = Utc::now() - super::entity::FunctionHost::live_window();
    let hosts = hosts
        .into_iter()
        .filter_map(|h| {
            let loaded: Vec<LoadedSummary> = h
                .loaded
                .iter()
                .filter(|lv| lv.address == address)
                .map(|lv| LoadedSummary {
                    version: lv.version,
                    state: lv.state.name().to_string(),
                    error: lv.state.error().map(str::to_string),
                })
                .collect();
            (!loaded.is_empty()).then(|| HostSummary {
                host_id: h.id,
                pool: h.pool,
                state: h.state.as_str().to_string(),
                last_heartbeat: h.last_heartbeat,
                stale: h.last_heartbeat < stale_before,
                loaded,
            })
        })
        .collect();
    Ok(Json(StatusResponse {
        address: address.render(),
        status: f.status.as_str().to_string(),
        live,
        versions: versions
            .iter()
            .map(|v| VersionSummary {
                version: v.version,
                state: v.state.name().to_string(),
            })
            .collect(),
        hosts,
        wiring: wiring
            .into_iter()
            .map(|w| WiringEntry {
                kind: w.kind.as_str().to_string(),
                code: w.trigger_key,
                object_id: w.object_id,
                present: w.present,
            })
            .collect(),
    }))
}

/// Pools with a host heartbeated in the last 45 s, and how many. Anchor
/// only: pools are cross-tenant infrastructure with no function to reach.
#[utoipa::path(
    get, path = "/api/function-pools", tag = "functions",
    operation_id = "getApiFunctionPools",
    responses(
        (status = 200, body = Vec<PoolSummaryResponse>),
        (status = 403, description = "ANCHOR_REQUIRED or PERMISSION_REQUIRED"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn function_pools(
    State(state): State<FunctionsState>,
    auth: Authenticated,
) -> Result<Json<Vec<PoolSummaryResponse>>, PlatformError> {
    checks::require_anchor_scope(&auth.0)?;
    checks::require_permission(&auth.0, FUNCTION_VIEW)?;
    let seen_since = Utc::now() - super::entity::FunctionHost::live_window();
    let pools = state.hosts.pools(seen_since).await?;
    Ok(Json(
        pools
            .into_iter()
            .map(|p| PoolSummaryResponse {
                pool: p.pool,
                hosts: p.hosts,
            })
            .collect(),
    ))
}

// ── Handlers: config and secrets ────────────────────────────────────────────

/// `declared` and `declaredBy`: the live manifest's keys first, then the
/// candidate's not already listed. The candidate is the `?version=` given
/// (retired included), else the newest non-retired version.
async fn declared(
    state: &FunctionsState,
    f: &Function,
    version_param: Option<&str>,
    keys_of: fn(&FunctionVersion) -> &[String],
) -> Result<(Vec<String>, Vec<DeclaredByEntry>), PlatformError> {
    let live = match f.live_version_id() {
        Some(id) => state.versions.find_by_id(id).await?,
        None => None,
    };
    let candidate = match version_param {
        None => state.versions.find_newest_non_retired(&f.id).await?,
        Some(raw) => {
            let version = parse_version_number(raw)?;
            Some(
                state
                    .versions
                    .find_by_function_and_version(&f.id, version)
                    .await?
                    .ok_or_else(|| {
                        super::operations::access::resource_not_found(
                            "FunctionVersion",
                            &format!("{}#{version}", f.address.render()),
                        )
                    })?,
            )
        }
    };
    let mut declared: Vec<String> = Vec::new();
    let mut declared_by = Vec::new();
    if let Some(live) = &live {
        let keys = keys_of(live).to_vec();
        declared.extend(keys.iter().cloned());
        declared_by.push(DeclaredByEntry {
            version: live.version,
            keys,
        });
    }
    if let Some(candidate) = candidate {
        if live.as_ref().is_none_or(|l| l.id != candidate.id) {
            let keys = keys_of(&candidate).to_vec();
            for key in &keys {
                if !declared.contains(key) {
                    declared.push(key.clone());
                }
            }
            declared_by.push(DeclaredByEntry {
                version: candidate.version,
                keys,
            });
        }
    }
    Ok((declared, declared_by))
}

fn config_keys(v: &FunctionVersion) -> &[String] {
    &v.manifest.config
}

fn secret_keys(v: &FunctionVersion) -> &[String] {
    &v.manifest.secrets
}

async fn config_response(
    state: &FunctionsState,
    f: &Function,
    version_param: Option<&str>,
) -> Result<ConfigResponse, PlatformError> {
    let values = state.settings.config_map(&f.id).await?;
    let (declared, declared_by) = declared(state, f, version_param, config_keys).await?;
    let missing = declared
        .iter()
        .filter(|k| !values.contains_key(*k))
        .cloned()
        .collect();
    Ok(ConfigResponse {
        values,
        declared,
        missing,
        declared_by,
    })
}

/// A function's config: the values, and which keys its manifests declare.
#[utoipa::path(
    get, path = "/api/functions/{address}/config", tag = "functions",
    operation_id = "getApiFunctionsByAddressConfig",
    params(("address" = String, Path, description = "app.service.name"), DeclaredQuery),
    responses(
        (status = 200, body = ConfigResponse),
        (status = 400, description = "ADDRESS_INVALID or VERSION_INVALID"),
        (status = 404, description = "Function_NOT_FOUND or FunctionVersion_NOT_FOUND"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_config(
    State(state): State<FunctionsState>,
    auth: Authenticated,
    Path(address): Path<String>,
    Query(params): Query<QueryParams>,
) -> Result<Json<ConfigResponse>, PlatformError> {
    checks::require_permission(&auth.0, FUNCTION_VIEW)?;
    let address = address_from_path(&address)?;
    let caller = state.caller(&auth.0).await?;
    let f = reachable_function(&state, &address, &caller).await?;
    Ok(Json(
        config_response(&state, &f, query_param(&params, "version")).await?,
    ))
}

/// Replace a function's config (at most 100 keys, each value at most 8 KiB).
#[utoipa::path(
    put, path = "/api/functions/{address}/config", tag = "functions",
    operation_id = "putApiFunctionsByAddressConfig",
    params(("address" = String, Path, description = "app.service.name"), DeclaredQuery),
    request_body = SetConfigRequest,
    responses(
        (status = 200, body = ConfigResponse),
        (status = 400, description = "SETTING_KEY_INVALID or SETTING_TOO_LARGE"),
        (status = 403), (status = 404),
    ),
    security(("bearer_auth" = []))
)]
pub async fn put_config(
    State(state): State<FunctionsState>,
    auth: Authenticated,
    Path(address): Path<String>,
    Query(params): Query<QueryParams>,
    body: Bytes,
) -> Result<Json<ConfigResponse>, PlatformError> {
    checks::require_permission(&auth.0, FUNCTION_MANAGE)?;
    let address = address_from_path(&address)?;
    let req: SetConfigRequest = parse_body(&body)?;
    let caller = state.caller(&auth.0).await?;
    let command = SetConfigCommand {
        address: address.clone(),
        values: req.values.unwrap_or_default(),
    };
    state
        .ops
        .set_config(caller.clone())
        .run(command, ExecutionContext::from_auth(&auth.0))
        .await
        .into_result()?;
    let f = reachable_function(&state, &address, &caller).await?;
    Ok(Json(
        config_response(&state, &f, query_param(&params, "version")).await?,
    ))
}

/// A function's secrets: keys and metadata, never a value.
#[utoipa::path(
    get, path = "/api/functions/{address}/secrets", tag = "functions",
    operation_id = "getApiFunctionsByAddressSecrets",
    params(("address" = String, Path, description = "app.service.name"), DeclaredQuery),
    responses(
        (status = 200, body = SecretListResponse),
        (status = 400), (status = 403), (status = 404),
        (status = 503, description = "ENCRYPTION_UNCONFIGURED"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_secrets(
    State(state): State<FunctionsState>,
    auth: Authenticated,
    Path(address): Path<String>,
    Query(params): Query<QueryParams>,
) -> Result<Json<SecretListResponse>, PlatformError> {
    checks::require_permission(&auth.0, FUNCTION_VIEW)?;
    require_encryption(&state)?;
    let address = address_from_path(&address)?;
    let caller = state.caller(&auth.0).await?;
    let f = reachable_function(&state, &address, &caller).await?;
    let infos = state.settings.list_secrets(&f.id).await?;
    let (declared, declared_by) =
        declared(&state, &f, query_param(&params, "version"), secret_keys).await?;
    let missing = declared
        .iter()
        .filter(|k| !infos.iter().any(|i| &i.key == *k))
        .cloned()
        .collect();
    Ok(Json(SecretListResponse {
        keys: infos
            .into_iter()
            .map(|i| SecretKeyResponse {
                key: i.key,
                updated_at: i.updated_at,
                updated_by: i.updated_by,
            })
            .collect(),
        declared,
        missing,
        declared_by,
    }))
}

/// Set or replace one secret. The value is never echoed back.
#[utoipa::path(
    put, path = "/api/functions/{address}/secrets/{key}", tag = "functions",
    operation_id = "putApiFunctionsByAddressSecretsByKey",
    params(
        ("address" = String, Path, description = "app.service.name"),
        ("key" = String, Path, description = "Setting key"),
    ),
    request_body = SetSecretRequest,
    responses(
        (status = 204),
        (status = 400, description = "SETTING_KEY_INVALID, SETTING_VALUE_REQUIRED or SETTING_TOO_LARGE"),
        (status = 403), (status = 404),
        (status = 503, description = "ENCRYPTION_UNCONFIGURED"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn put_secret(
    State(state): State<FunctionsState>,
    auth: Authenticated,
    Path((address, key)): Path<(String, String)>,
    body: Bytes,
) -> Result<StatusCode, PlatformError> {
    checks::require_permission(&auth.0, FUNCTION_SECRET_MANAGE)?;
    require_encryption(&state)?;
    let address = address_from_path(&address)?;
    let req: SetSecretRequest = parse_body(&body)?;
    let command = SetSecretCommand {
        address,
        key,
        value: SecretValue::new(req.value.unwrap_or_default()),
    };
    let caller = state.caller(&auth.0).await?;
    state
        .ops
        .set_secret(caller)
        .run(command, ExecutionContext::from_auth(&auth.0))
        .await
        .into_result()?;
    Ok(StatusCode::NO_CONTENT)
}

/// Delete one secret: 404 when it was never set.
#[utoipa::path(
    delete, path = "/api/functions/{address}/secrets/{key}", tag = "functions",
    operation_id = "deleteApiFunctionsByAddressSecretsByKey",
    params(
        ("address" = String, Path, description = "app.service.name"),
        ("key" = String, Path, description = "Setting key"),
    ),
    responses(
        (status = 204), (status = 400), (status = 403),
        (status = 404, description = "Function_NOT_FOUND or FunctionSecret_NOT_FOUND"),
        (status = 503, description = "ENCRYPTION_UNCONFIGURED"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_secret(
    State(state): State<FunctionsState>,
    auth: Authenticated,
    Path((address, key)): Path<(String, String)>,
) -> Result<StatusCode, PlatformError> {
    checks::require_permission(&auth.0, FUNCTION_SECRET_MANAGE)?;
    require_encryption(&state)?;
    let address = address_from_path(&address)?;
    let caller = state.caller(&auth.0).await?;
    state
        .ops
        .delete_secret(caller)
        .run(
            DeleteSecretCommand { address, key },
            ExecutionContext::from_auth(&auth.0),
        )
        .await
        .into_result()?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Router ──────────────────────────────────────────────────────────────────

/// Every function route: `/api/functions*`, `/api/function-pools`,
/// `/api/function-policies*`, `/api/function-domains*` and
/// `/api/function-routes`, with full paths (merge, don't nest).
pub fn function_routes() -> OpenApiRouter<FunctionsState> {
    OpenApiRouter::new()
        .routes(routes!(list_functions, create_function))
        .routes(routes!(get_function, update_function, delete_function))
        .routes(routes!(function_status))
        .routes(routes!(function_pools))
        .routes(routes!(
            super::version_api::publish_version,
            super::version_api::list_versions
        ))
        .routes(routes!(super::version_api::get_version))
        .routes(routes!(super::version_api::retire_version))
        .routes(routes!(super::version_api::check_manifest))
        .routes(routes!(super::version_api::upload_artifact))
        .routes(routes!(get_config, put_config))
        .routes(routes!(get_secrets))
        .routes(routes!(put_secret, delete_secret))
        .routes(routes!(super::policy_api::list_policies))
        .routes(routes!(
            super::policy_api::get_policy,
            super::policy_api::put_policy
        ))
        .routes(routes!(
            super::domain_api::claim_domain,
            super::domain_api::list_domains
        ))
        .routes(routes!(
            super::domain_api::get_domain,
            super::domain_api::release_domain
        ))
        .routes(routes!(super::domain_api::list_routes))
}

/// [`function_routes`] with its state.
pub fn functions_router(state: FunctionsState) -> OpenApiRouter {
    function_routes().with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(pairs: &[(&str, &str)]) -> QueryParams {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn page_query_resolves_as_javas_page_query() {
        assert_eq!(page_query(&params(&[])).unwrap(), (0, 20));
        assert_eq!(
            page_query(&params(&[("page", "-3"), ("size", "5")])).unwrap(),
            (0, 5)
        );
        assert_eq!(
            page_query(&params(&[("size", "0"), ("limit", "7")])).unwrap(),
            (0, 7)
        );
        assert_eq!(
            page_query(&params(&[("pageSize", "5000")])).unwrap(),
            (0, 1000)
        );
        assert_eq!(
            page_query(&params(&[("page", " 2 "), ("size", "")])).unwrap(),
            (2, 20)
        );
        match page_query(&params(&[("page", "x"), ("size", "1.5")])) {
            Err(PlatformError::Coded {
                status,
                code,
                message,
                details,
            }) => {
                assert_eq!(status, StatusCode::BAD_REQUEST);
                assert_eq!(code, "VALIDATION");
                assert_eq!(message, "validation failed");
                let errors = details["errors"].as_array().unwrap();
                assert_eq!(errors.len(), 2);
                assert_eq!(errors[0]["location"], "query.page");
                assert_eq!(errors[0]["message"], "invalid integer");
                assert_eq!(errors[1]["value"], "1.5");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn version_numbers_must_be_positive_integers() {
        assert_eq!(parse_version_number("3").unwrap(), 3);
        for raw in ["0", "-1", "x", "1.0", ""] {
            assert_eq!(
                parse_version_number(raw).unwrap_err().code(),
                "VERSION_INVALID"
            );
        }
    }

    #[test]
    fn empty_query_values_are_absent_and_the_first_one_wins() {
        let p = params(&[("a", ""), ("b", "1"), ("b", "2")]);
        assert_eq!(query_param(&p, "a"), None);
        assert_eq!(query_param(&p, "b"), Some("1"));
        assert_eq!(query_param(&p, "c"), None);
    }
}
