//! `POST /api/clients/search` (Go `client/api/api.go:39`, `searchClients`):
//! `{term}` → `{clients, total}`, at most 50 by identifier. Go's gate is
//! `CanReadClients` = anchor and `platform:admin:client:view`.

use axum::{extract::State, Json};
use serde::Deserialize;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::client::api::{ClientListResponse, ClientResponse};
use crate::client::repository::ClientRepository;
use crate::shared::authorization_service::checks;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;

#[derive(Clone)]
pub struct ClientSearchState {
    pub client_repo: std::sync::Arc<ClientRepository>,
}

/// Go `SearchClientRequest`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchClientRequest {
    pub term: String,
}

/// Search clients by name or identifier.
#[utoipa::path(
    post,
    path = "/api/clients/search",
    tag = "clients",
    operation_id = "searchClients",
    request_body = SearchClientRequest,
    responses(
        (status = 200, description = "Matching clients", body = ClientListResponse),
        (status = 403, description = "Not an anchor holding platform:admin:client:view")
    ),
    security(("bearer_auth" = []))
)]
pub async fn search_clients_by_body(
    State(state): State<ClientSearchState>,
    auth: Authenticated,
    Json(req): Json<SearchClientRequest>,
) -> Result<Json<ClientListResponse>, PlatformError> {
    checks::require_anchor_scope(&auth.0)?;
    checks::require_permission(&auth.0, crate::permissions::admin::CLIENT_READ)?;
    let clients: Vec<ClientResponse> = state
        .client_repo
        .search_top(&req.term)
        .await?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(ClientListResponse {
        total: clients.len(),
        clients,
    }))
}

/// Full-path router; merged at the root.
pub fn client_search_router(state: ClientSearchState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(search_clients_by_body))
        .with_state(state)
}
