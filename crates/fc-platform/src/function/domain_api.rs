//! `/api/function-domains` and `/api/function-routes` (Java
//! `function/api/FunctionDomainApi.java`). There is no verify route: a claim
//! is verified by being made.

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::api::{address_from_path, query_param, reachable_function, FunctionsState, QueryParams};
use super::entity::{FunctionDomain, FunctionRoute};
use super::operations::access::domain_by_hostname;
use super::operations::create::blank_to_none;
use super::operations::{ClaimCommand, ReleaseCommand};
use super::wire::{micros_ser, parse_body};
use super::{FunctionOwner, Hostname};
use crate::permissions::function::{FUNCTION_DOMAIN_MANAGE, FUNCTION_VIEW};
use crate::shared::authorization_service::checks;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::usecase::{ExecutionContext, UseCase, UseCaseError};

// ── DTOs ────────────────────────────────────────────────────────────────────

/// Body of `POST /api/function-domains`. No `clientId`: the platform's.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClaimRequest {
    pub hostname: Option<String>,
    pub client_id: Option<String>,
}

/// `{id, hostname, owner, createdAt}`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DomainResponse {
    pub id: String,
    pub hostname: String,
    /// A client id, or `platform`.
    pub owner: String,
    #[serde(serialize_with = "micros_ser")]
    pub created_at: DateTime<Utc>,
}

impl From<&FunctionDomain> for DomainResponse {
    fn from(d: &FunctionDomain) -> Self {
        Self {
            id: d.id.clone(),
            hostname: d.hostname.value().to_string(),
            owner: d.owner.to_wire().to_string(),
            created_at: d.created_at,
        }
    }
}

/// One public route.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FunctionRouteResponse {
    pub hostname: String,
    pub path_prefix: String,
    pub address: String,
    pub alias_prefixes: Vec<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[allow(dead_code)]
pub struct DomainListQuery {
    /// Required: a client id, or `platform`.
    #[serde(rename = "clientId")]
    #[param(rename = "clientId")]
    client_id: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[allow(dead_code)]
pub struct RouteListQuery {
    /// Every route on this hostname whose function the caller reaches.
    hostname: Option<String>,
    /// One function's routes; wins over `hostname`.
    address: Option<String>,
}

// ── Handlers ────────────────────────────────────────────────────────────────

/// Claim a zone.
#[utoipa::path(
    post, path = "/api/function-domains", tag = "function-domains",
    operation_id = "postApiFunctionDomains",
    request_body = ClaimRequest,
    responses(
        (status = 201, body = DomainResponse),
        (status = 400, description = "HOSTNAME_INVALID"),
        (status = 403, description = "PERMISSION_REQUIRED or SCOPE_FORBIDDEN"),
        (status = 409, description = "DOMAIN_TAKEN"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn claim_domain(
    State(state): State<FunctionsState>,
    auth: Authenticated,
    body: Bytes,
) -> Result<(StatusCode, Json<DomainResponse>), PlatformError> {
    checks::require_permission(&auth.0, FUNCTION_DOMAIN_MANAGE)?;
    let req: ClaimRequest = parse_body(&body)?;
    let owner = match blank_to_none(req.client_id.as_deref()) {
        Some(id) => FunctionOwner::Client(id.to_string()),
        None => FunctionOwner::Platform,
    };
    let caller = state.caller(&auth.0).await?;
    let event = state
        .ops
        .claim_domain(caller)
        .run(
            ClaimCommand {
                owner,
                hostname: req.hostname,
            },
            ExecutionContext::from_auth(&auth.0),
        )
        .await
        .into_result()?;
    let d = state
        .domains
        .find_by_id(&event.domain_id)
        .await?
        .ok_or_else(|| PlatformError::internal("REPO: domain claimed but row not found"))?;
    Ok((StatusCode::CREATED, Json(DomainResponse::from(&d))))
}

/// One owner's claims. A caller who cannot reach that owner sees an empty
/// list, not a 403 or 404.
#[utoipa::path(
    get, path = "/api/function-domains", tag = "function-domains",
    operation_id = "getApiFunctionDomains",
    params(DomainListQuery),
    responses(
        (status = 200, body = Vec<DomainResponse>),
        (status = 400, description = "CLIENT_ID_REQUIRED"),
        (status = 403),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_domains(
    State(state): State<FunctionsState>,
    auth: Authenticated,
    Query(params): Query<QueryParams>,
) -> Result<Json<Vec<DomainResponse>>, PlatformError> {
    checks::require_permission(&auth.0, FUNCTION_VIEW)?;
    let raw = query_param(&params, "clientId").ok_or_else(|| {
        PlatformError::from(UseCaseError::validation(
            "CLIENT_ID_REQUIRED",
            "clientId is required",
        ))
    })?;
    let owner = FunctionOwner::from_wire(raw)
        .map_err(|e| PlatformError::bad_request_code("CLIENT_ID_INVALID", e.to_string()))?;
    let caller = state.caller(&auth.0).await?;
    if !caller.can_access_scope(owner.client_id_or_none()) {
        return Ok(Json(Vec::new()));
    }
    let domains = state.domains.list_by_owner(&owner).await?;
    Ok(Json(domains.iter().map(DomainResponse::from).collect()))
}

/// The claim covering a hostname (the hostname itself or its zone).
#[utoipa::path(
    get, path = "/api/function-domains/{hostname}", tag = "function-domains",
    operation_id = "getApiFunctionDomainsByHostname",
    params(("hostname" = String, Path, description = "A claimed hostname, or one under a claimed zone")),
    responses(
        (status = 200, body = DomainResponse),
        (status = 400, description = "HOSTNAME_INVALID"),
        (status = 404, description = "FUNCTION_DOMAIN_NOT_FOUND, also when out of reach"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_domain(
    State(state): State<FunctionsState>,
    auth: Authenticated,
    Path(hostname): Path<String>,
) -> Result<Json<DomainResponse>, PlatformError> {
    checks::require_permission(&auth.0, FUNCTION_VIEW)?;
    let hostname = Hostname::parse(&hostname)?;
    let caller = state.caller(&auth.0).await?;
    let d = domain_by_hostname(&state.domains, &hostname, &caller).await?;
    Ok(Json(DomainResponse::from(&d)))
}

/// Release the zone covering a hostname: 409 `DOMAIN_IN_USE` while any
/// public route is under it.
#[utoipa::path(
    delete, path = "/api/function-domains/{hostname}", tag = "function-domains",
    operation_id = "deleteApiFunctionDomainsByHostname",
    params(("hostname" = String, Path, description = "A claimed hostname, or one under a claimed zone")),
    responses(
        (status = 204), (status = 400), (status = 403), (status = 404),
        (status = 409, description = "DOMAIN_IN_USE"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn release_domain(
    State(state): State<FunctionsState>,
    auth: Authenticated,
    Path(hostname): Path<String>,
) -> Result<StatusCode, PlatformError> {
    checks::require_permission(&auth.0, FUNCTION_DOMAIN_MANAGE)?;
    let caller = state.caller(&auth.0).await?;
    state
        .ops
        .release_domain(caller)
        .run(
            ReleaseCommand { hostname },
            ExecutionContext::from_auth(&auth.0),
        )
        .await
        .into_result()?;
    Ok(StatusCode::NO_CONTENT)
}

/// Public routes by function (`address`) or by hostname, sorted by
/// hostname then path prefix. By hostname, a route whose function is out
/// of reach is left out.
#[utoipa::path(
    get, path = "/api/function-routes", tag = "function-domains",
    operation_id = "getApiFunctionRoutes",
    params(RouteListQuery),
    responses(
        (status = 200, body = Vec<FunctionRouteResponse>),
        (status = 400, description = "FUNCTION_ROUTE_FILTER_REQUIRED, ADDRESS_INVALID or HOSTNAME_INVALID"),
        (status = 404, description = "FUNCTION_NOT_FOUND"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_routes(
    State(state): State<FunctionsState>,
    auth: Authenticated,
    Query(params): Query<QueryParams>,
) -> Result<Json<Vec<FunctionRouteResponse>>, PlatformError> {
    checks::require_permission(&auth.0, FUNCTION_VIEW)?;
    let caller = state.caller(&auth.0).await?;
    let (rows, addresses): (Vec<FunctionRoute>, HashMap<String, String>) =
        if let Some(raw) = query_param(&params, "address") {
            let address = address_from_path(raw)?;
            let f = reachable_function(&state, &address, &caller).await?;
            let rows = state.routes.list_by_function(&f.id).await?;
            (rows, HashMap::from([(f.id.clone(), f.address.render())]))
        } else if let Some(raw) = query_param(&params, "hostname") {
            let hostname = Hostname::parse(raw)?;
            let rows = state.routes.list_by_hostname(&hostname).await?;
            let mut ids: Vec<String> = rows.iter().map(|r| r.function_id.clone()).collect();
            ids.sort();
            ids.dedup();
            let reachable: HashMap<String, String> = state
                .functions
                .find_by_ids(&ids)
                .await?
                .into_iter()
                .filter(|f| caller.can_reach(f))
                .map(|f| (f.id.clone(), f.address.render()))
                .collect();
            let rows = rows
                .into_iter()
                .filter(|r| reachable.contains_key(&r.function_id))
                .collect();
            (rows, reachable)
        } else {
            return Err(UseCaseError::validation(
                "FUNCTION_ROUTE_FILTER_REQUIRED",
                "hostname or address is required",
            )
            .into());
        };
    let mut out: Vec<FunctionRouteResponse> = rows
        .into_iter()
        .map(|r| FunctionRouteResponse {
            address: addresses
                .get(&r.function_id)
                .cloned()
                .unwrap_or_else(|| r.function_id.clone()),
            hostname: r.hostname.value().to_string(),
            path_prefix: r.path_prefix.value().to_string(),
            alias_prefixes: r.alias_prefixes,
        })
        .collect();
    out.sort_by(|a, b| {
        a.hostname
            .cmp(&b.hostname)
            .then_with(|| a.path_prefix.cmp(&b.path_prefix))
    });
    Ok(Json(out))
}
