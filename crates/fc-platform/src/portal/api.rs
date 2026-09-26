//! `/api/portal-users` and `/api/portal-apps` — the admin surface of the
//! portal identity plane (Go `portalidentity/api/api.go`). Portal backends'
//! service accounts ensure/invite, grant/revoke, search, suspend and offboard
//! their client's portal identities here; the platform UI uses the same
//! surface.
//!
//! Every handler resolves the target client from the request, then gates on
//! [`can_read_portal_users`] / [`can_write_portal_users`] for that client.

use std::collections::HashMap;

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use super::entity::{micros, normalize_app_code, LinkedOAuthClient, PortalApp, PortalIdentity};
use super::operations::{
    not_found, AppGrantCommand, AssignUnassignedCommand, AssignUnassignedPortalIdentitiesUseCase,
    CreateAppWithOAuthClientCommand, CreatePortalAppWithOAuthClientUseCase, DeleteAppCommand,
    DeleteCommand, DeletePortalAppUseCase, DeletePortalIdentityUseCase, EnsureCommand,
    EnsurePortalIdentityUseCase, GrantPortalIdentityAppUseCase, RevokePortalIdentityAppUseCase,
    SetPortalIdentityStatusUseCase, SetStatusCommand, UpdateAppCommand, UpdatePortalAppUseCase,
    NOTHING_TO_ASSIGN,
};
use super::repository::IdentitySearch;
use super::{can_read_portal_users, can_write_portal_users, PortalState};
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::usecase::{ExecutionContext, UseCase, UseCaseError};

type ApiResult<T> = Result<T, PlatformError>;

/// Go `apicommon.MaxPageSize`.
const MAX_PAGE_SIZE: i64 = 1000;

fn validation(code: &str, message: &str) -> PlatformError {
    PlatformError::from(UseCaseError::validation(code, message))
}

fn internal(code: &str, message: &str) -> PlatformError {
    PlatformError::Coded {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: code.to_string(),
        message: message.to_string(),
        details: Default::default(),
    }
}

/// The raw request body (read leniently by [`body`]).
pub struct RawBody(pub Bytes);

impl<S: Send + Sync> axum::extract::FromRequest<S> for RawBody {
    type Rejection = PlatformError;

    async fn from_request(req: axum::extract::Request, state: &S) -> Result<Self, Self::Rejection> {
        Bytes::from_request(req, state)
            .await
            .map(RawBody)
            .map_err(|_| PlatformError::bad_request_code("INVALID_BODY", "malformed request body"))
    }
}

/// A JSON body, absent or empty meaning all-defaults (the lenient reading
/// huma gives Go's bodies; no content-type is required).
fn body<T: DeserializeOwned + Default>(bytes: &Bytes) -> ApiResult<T> {
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(T::default());
    }
    serde_json::from_slice(bytes).map_err(|e| {
        PlatformError::bad_request_code("INVALID_BODY", format!("malformed request body: {e}"))
    })
}

/// Go's huma request validation for a JSON object body: each member the
/// Go struct declares without `omitempty` must be present (`expected
/// required property <name> to be present` at `body`, with the body as the
/// value), and an enum member must hold one of its values. Answered as
/// huma's 400 `VALIDATION` (`validation failed`, `details.errors`). A body
/// that is not a JSON object is left to [`body`].
fn huma_check(bytes: &Bytes, required: &[&str], enums: &[(&str, &[&str])]) -> ApiResult<()> {
    let Ok(serde_json::Value::Object(map)) = serde_json::from_slice::<serde_json::Value>(bytes)
    else {
        return Ok(());
    };
    let mut errors = Vec::new();
    for name in required {
        if !map.contains_key(*name) {
            errors.push(serde_json::json!({
                "location": "body",
                "message": format!("expected required property {name} to be present"),
                "value": serde_json::Value::Object(map.clone()),
            }));
        }
    }
    for (name, allowed) in enums {
        if let Some(value) = map.get(*name).filter(|v| !v.is_null()) {
            if !value.as_str().is_some_and(|v| allowed.contains(&v)) {
                errors.push(serde_json::json!({
                    "location": format!("body.{name}"),
                    "message": format!("expected value to be one of \"{}\"", allowed.join(", ")),
                    "value": value,
                }));
            }
        }
    }
    if errors.is_empty() {
        return Ok(());
    }
    let mut details = std::collections::HashMap::new();
    details.insert("errors".to_string(), serde_json::Value::Array(errors));
    Err(PlatformError::Coded {
        status: StatusCode::BAD_REQUEST,
        code: "VALIDATION".to_string(),
        message: "validation failed".to_string(),
        details,
    })
}

/// `{message}` (Go `apicommon.StatusChangeResponse`).
#[derive(Debug, Serialize)]
pub struct MessageResponse {
    pub message: String,
}

fn message(m: impl Into<String>) -> Json<MessageResponse> {
    Json(MessageResponse { message: m.into() })
}

/// One of the client's apps by code (404 when the client has none).
async fn app_by_code(state: &PortalState, client_id: &str, code: &str) -> ApiResult<PortalApp> {
    state
        .apps
        .find_by_client_and_code(client_id, code)
        .await?
        .ok_or_else(|| not_found("PortalApp", &normalize_app_code(code)).into())
}

/// Go `authorizeManage`'s first half: the target client must be named.
fn require_client_id(client_id: &str) -> ApiResult<()> {
    if client_id.is_empty() {
        return Err(validation("CLIENT_ID_REQUIRED", "clientId is required"));
    }
    Ok(())
}

// ── ensure / invite ───────────────────────────────────────────────────────

/// Go `PortalUserRequest`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortalUserRequest {
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub email: String,
    pub name: Option<String>,
    /// The portal app (code) the calling portal is.
    pub portal_app_code: Option<String>,
    /// Return the set-password link instead of mailing it.
    #[serde(default)]
    pub return_invite_link: bool,
    /// Followed after a successful set-password; must exactly match a
    /// registered redirect URI of one of the client's portal OAuth clients.
    pub redirect_uri: Option<String>,
}

/// Go `PortalUserResponse`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortalUserResponse {
    pub identity_id: String,
    pub created: bool,
    pub invited: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invite_url: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub sso_managed: bool,
    pub has_password: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub portal_app_code: Option<String>,
    pub state: String,
}

/// Ensure a portal identity for (client, email), grant a portal app, and
/// deliver a set-password invite
#[utoipa::path(
    post,
    path = "",
    tag = "portal-users",
    operation_id = "ensurePortalUser",
    responses((status = 200, description = "Idempotent outcome")),
    security(("bearer_auth" = []))
)]
pub async fn ensure_portal_user(
    State(state): State<PortalState>,
    auth: Authenticated,
    RawBody(raw): RawBody,
) -> ApiResult<Json<PortalUserResponse>> {
    huma_check(&raw, &["clientId", "email"], &[])?;
    let req: PortalUserRequest = body(&raw)?;
    let client_id = req.client_id.trim().to_string();
    if client_id.is_empty() {
        return Err(validation("CLIENT_ID_REQUIRED", "clientId is required"));
    }
    can_write_portal_users(&auth.0, &client_id)?;
    let app = match req
        .portal_app_code
        .as_deref()
        .filter(|c| !c.trim().is_empty())
    {
        Some(code) => Some(app_by_code(&state, &client_id, code).await?),
        None => None,
    };
    let redirect_uri = match req
        .redirect_uri
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty())
    {
        Some(r) => {
            validate_redirect_uri(&state, &client_id, r).await?;
            Some(r.to_string())
        }
        // No explicit redirect: the portal's own origin, from its portal
        // OAuth client's registered redirect URI (best-effort).
        None => default_portal_redirect(&state, &client_id, app.as_ref()).await,
    };

    let cmd = EnsureCommand {
        client_id: client_id.clone(),
        email: req.email.clone(),
        name: req.name.clone(),
        source: "INVITE".to_string(),
        portal_app_id: app.as_ref().map(|a| a.id.clone()),
    };
    let use_case = EnsurePortalIdentityUseCase::new(
        state.identities.clone(),
        state.apps.clone(),
        state.clients.clone(),
        state.unit_of_work.clone(),
    );
    let event = use_case
        .run(cmd, ExecutionContext::from_auth(&auth.0))
        .await
        .into_result()?;
    let mut ident = state
        .identities
        .find_by_id(&event.identity_id)
        .await?
        .ok_or_else(|| internal("REPO", "post-ensure identity lookup failed"))?;

    let mut resp = PortalUserResponse {
        identity_id: ident.id.clone(),
        created: event.created,
        invited: false,
        invite_url: None,
        sso_managed: false,
        has_password: ident.has_password(),
        portal_app_code: app.as_ref().map(|a| a.code.clone()),
        state: String::new(),
    };
    let now = Utc::now();

    // SSO-owned domain: never a set-password invite (their org signs them
    // in; a password would be an SSO bypass).
    let domain = super::entity::email_domain_of(&ident.email);
    if let Ok(Some(_)) = state.oidc_provider_for_domain(&domain).await {
        resp.sso_managed = true;
        let pending = ident.last_login_at.is_none();
        if req.return_invite_link {
            resp.invite_url = redirect_uri.clone();
        } else if let (Some(portal_url), true) = (redirect_uri.as_deref(), pending) {
            if state
                .passwords
                .send_portal_sso_invite(&ident.email, portal_url)
                .await
                .is_ok()
            {
                resp.invited = true;
            }
        }
        if pending && (resp.invited || resp.invite_url.is_some()) {
            mark_invited(&state, &mut ident, now, None).await;
        }
        resp.state = ident.state(now).as_str().to_string();
        return Ok(Json(resp));
    }

    // Invite while the identity cannot yet sign in with a password — on
    // first create AND on later ensures, so a lost or expired invite is
    // recovered by ensuring again.
    if !ident.has_password() {
        let expires = if req.return_invite_link {
            let (link, expires) = state
                .passwords
                .portal_invite_link(&ident.id, redirect_uri.as_deref())
                .await
                .map_err(|_| internal("INVITE_LINK", "could not mint the invite link"))?;
            resp.invite_url = Some(link);
            expires
        } else {
            let expires = state
                .passwords
                .send_portal_invite(&ident.id, &ident.email, redirect_uri.as_deref())
                .await
                .map_err(|_| internal("INVITE_EMAIL", "could not send the invite email"))?;
            resp.invited = true;
            expires
        };
        mark_invited(&state, &mut ident, now, Some(expires)).await;
    }
    resp.state = ident.state(now).as_str().to_string();
    Ok(Json(resp))
}

/// Record the invite (best-effort bookkeeping: the invite already went out)
/// and mirror it on the loaded identity so the response state reflects it.
async fn mark_invited(
    state: &PortalState,
    ident: &mut PortalIdentity,
    at: DateTime<Utc>,
    expires: Option<DateTime<Utc>>,
) {
    if state
        .identities
        .mark_invited(&ident.id, at, expires)
        .await
        .is_ok()
    {
        ident.invited_at = Some(at);
        ident.invite_expires_at = expires;
    }
}

/// The post-set-password redirect must exactly match a registered redirect
/// URI of one of the client's portal OAuth clients (fail closed).
async fn validate_redirect_uri(state: &PortalState, client_id: &str, uri: &str) -> ApiResult<()> {
    let clients = state.portal_oauth.find_by_portal_client(client_id).await?;
    if clients
        .iter()
        .any(|c| c.redirect_uris.iter().any(|r| r == uri))
    {
        return Ok(());
    }
    Err(validation(
        "REDIRECT_URI_INVALID",
        "redirectUri must exactly match a registered redirect URI of one of the client's portal OAuth clients",
    ))
}

/// The portal's origin (`scheme://host/`) from the first usable registered
/// redirect URI of the client's portal OAuth clients — the app's own first
/// when an app is given (Go `defaultPortalRedirect`).
async fn default_portal_redirect(
    state: &PortalState,
    client_id: &str,
    app: Option<&PortalApp>,
) -> Option<String> {
    let mut clients = state
        .portal_oauth
        .find_by_portal_client(client_id)
        .await
        .ok()?;
    if let Some(app) = app {
        // Stable partition: the app's own OAuth clients first.
        clients.sort_by_key(|c| c.portal_app_id.as_deref() != Some(app.id.as_str()));
    }
    clients
        .iter()
        .flat_map(|c| c.redirect_uris.iter())
        .find_map(|raw| origin_of(raw))
}

/// `scheme://host/` of a URL, or `None` when it has no host or the host is a
/// wildcard pattern (Go `portalOriginOf`). Go's `u.Host` keeps the port.
pub(crate) fn origin_of(raw: &str) -> Option<String> {
    let u = reqwest::Url::parse(raw).ok()?;
    let host = u.host_str().filter(|h| !h.is_empty() && !h.contains('*'))?;
    let host = match u.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    };
    Some(format!("{}://{}/", u.scheme(), host))
}

// ── search / status / delete ──────────────────────────────────────────────

/// One portal app an identity is granted (Go `PortalUserAppRef`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortalUserAppRef {
    pub id: String,
    pub code: String,
    pub name: String,
    pub source: String,
    pub granted_at: String,
}

/// One row of `GET /api/portal-users` (Go `PortalUserListItem`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortalUserListItem {
    pub identity_id: String,
    pub email: String,
    pub name: String,
    pub status: String,
    pub state: String,
    pub source: String,
    pub has_password: bool,
    pub apps: Vec<PortalUserAppRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invited_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invite_expires_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_login_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Go `PortalUserListResponse`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortalUserListResponse {
    pub portal_users: Vec<PortalUserListItem>,
    pub total: i64,
    pub page: i64,
    pub size: i64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    pub client_id: Option<String>,
    pub q: Option<String>,
    pub portal_app_code: Option<String>,
    pub unassigned: Option<String>,
    pub page: Option<String>,
    pub size: Option<String>,
}

fn int_param(v: Option<&str>) -> ApiResult<i64> {
    match v.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(0),
        Some(s) => s.parse().map_err(|_| {
            PlatformError::bad_request_code("INVALID_PARAM", format!("not an integer: {s}"))
        }),
    }
}

/// Search a client's portal identities (prefix match on email and name)
#[utoipa::path(
    get,
    path = "",
    tag = "portal-users",
    operation_id = "listPortalUsers",
    responses((status = 200, description = "A page of the client's portal identities")),
    security(("bearer_auth" = []))
)]
pub async fn list_portal_users(
    State(state): State<PortalState>,
    auth: Authenticated,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<PortalUserListResponse>> {
    let client_id = q.client_id.as_deref().unwrap_or("").trim().to_string();
    if client_id.is_empty() {
        return Err(validation(
            "CLIENT_ID_REQUIRED",
            "clientId query param is required",
        ));
    }
    can_read_portal_users(&auth.0, &client_id)?;
    let unassigned = matches!(q.unassigned.as_deref(), Some("true") | Some("1"));
    let code = q.portal_app_code.as_deref().unwrap_or("").trim();
    if unassigned && !code.is_empty() {
        return Err(validation(
            "FILTER_CONFLICT",
            "unassigned and portalAppCode cannot be combined",
        ));
    }
    let app_id = if code.is_empty() {
        None
    } else {
        Some(app_by_code(&state, &client_id, code).await?.id)
    };
    let page = int_param(q.page.as_deref())?.max(0);
    let mut size = int_param(q.size.as_deref())?;
    if size <= 0 {
        size = 100;
    }
    let size = size.min(MAX_PAGE_SIZE);

    let search = IdentitySearch {
        client_id: client_id.clone(),
        query: q.q.clone().unwrap_or_default(),
        app_id,
        unassigned,
        offset: page * size,
        limit: size,
    };
    let (rows, total) = state.identities.search(&search).await?;
    let apps: HashMap<String, PortalApp> = state
        .apps
        .find_by_client(Some(&client_id))
        .await?
        .into_iter()
        .map(|a| (a.id.clone(), a))
        .collect();
    let now = Utc::now();
    let portal_users = rows.iter().map(|i| list_item(i, &apps, now)).collect();
    Ok(Json(PortalUserListResponse {
        portal_users,
        total,
        page,
        size,
    }))
}

fn list_item(
    i: &PortalIdentity,
    apps: &HashMap<String, PortalApp>,
    now: DateTime<Utc>,
) -> PortalUserListItem {
    PortalUserListItem {
        identity_id: i.id.clone(),
        email: i.email.clone(),
        name: i.name.clone(),
        status: i.status.as_str().to_string(),
        state: i.state(now).as_str().to_string(),
        source: i.source.as_str().to_string(),
        has_password: i.has_password(),
        apps: i
            .apps
            .iter()
            .map(|g| {
                let (code, name) = apps
                    .get(&g.app_id)
                    .map(|a| (a.code.clone(), a.name.clone()))
                    .unwrap_or_else(|| (g.app_id.clone(), g.app_id.clone()));
                PortalUserAppRef {
                    id: g.app_id.clone(),
                    code,
                    name,
                    source: g.source.as_str().to_string(),
                    granted_at: micros(&g.granted_at),
                }
            })
            .collect(),
        invited_at: i.invited_at.as_ref().map(micros),
        invite_expires_at: i.invite_expires_at.as_ref().map(micros),
        last_login_at: i.last_login_at.as_ref().map(micros),
        created_at: micros(&i.created_at),
        updated_at: micros(&i.updated_at),
    }
}

/// The tenant client an id-addressed mutation targets.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientBody {
    #[serde(default)]
    pub client_id: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientQuery {
    pub client_id: Option<String>,
}

async fn set_status(
    state: &PortalState,
    auth: &Authenticated,
    id: String,
    raw: &Bytes,
    status: &str,
) -> ApiResult<Json<MessageResponse>> {
    huma_check(raw, &["clientId"], &[])?;
    let req: ClientBody = body(raw)?;
    let client_id = req.client_id.trim().to_string();
    if client_id.is_empty() {
        return Err(validation("CLIENT_ID_REQUIRED", "clientId is required"));
    }
    can_write_portal_users(&auth.0, &client_id)?;
    let use_case =
        SetPortalIdentityStatusUseCase::new(state.identities.clone(), state.unit_of_work.clone());
    let cmd = SetStatusCommand {
        client_id,
        email: String::new(),
        id,
        status: status.to_string(),
    };
    use_case
        .run(cmd, ExecutionContext::from_auth(&auth.0))
        .await
        .into_result()?;
    Ok(message(if status == "DISABLED" {
        "Portal user deactivated"
    } else {
        "Portal user activated"
    }))
}

/// Reactivate a suspended portal identity
#[utoipa::path(
    post,
    path = "/{id}/activate",
    tag = "portal-users",
    operation_id = "activatePortalUser",
    responses((status = 200, description = "Activated")),
    security(("bearer_auth" = []))
)]
pub async fn activate_portal_user(
    State(state): State<PortalState>,
    auth: Authenticated,
    Path(id): Path<String>,
    RawBody(raw): RawBody,
) -> ApiResult<Json<MessageResponse>> {
    // Gated in set_status: can_write_portal_users for the body's client.
    set_status(&state, &auth, id, &raw, "ACTIVE").await
}

/// Suspend a portal identity (blocks portal login, keeps the row)
#[utoipa::path(
    post,
    path = "/{id}/deactivate",
    tag = "portal-users",
    operation_id = "deactivatePortalUser",
    responses((status = 200, description = "Deactivated")),
    security(("bearer_auth" = []))
)]
pub async fn deactivate_portal_user(
    State(state): State<PortalState>,
    auth: Authenticated,
    Path(id): Path<String>,
    RawBody(raw): RawBody,
) -> ApiResult<Json<MessageResponse>> {
    // Gated in set_status: can_write_portal_users for the body's client.
    set_status(&state, &auth, id, &raw, "DISABLED").await
}

/// Delete a portal identity (offboarding from every portal of the client)
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "portal-users",
    operation_id = "deletePortalUser",
    responses((status = 200, description = "Deleted")),
    security(("bearer_auth" = []))
)]
pub async fn delete_portal_user(
    State(state): State<PortalState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Query(q): Query<ClientQuery>,
) -> ApiResult<Json<MessageResponse>> {
    let client_id = q.client_id.as_deref().unwrap_or("").trim().to_string();
    if client_id.is_empty() {
        return Err(validation(
            "CLIENT_ID_REQUIRED",
            "clientId query param is required",
        ));
    }
    can_write_portal_users(&auth.0, &client_id)?;
    let use_case =
        DeletePortalIdentityUseCase::new(state.identities.clone(), state.unit_of_work.clone());
    use_case
        .run(
            DeleteCommand { client_id, id },
            ExecutionContext::from_auth(&auth.0),
        )
        .await
        .into_result()?;
    Ok(message("Portal user deleted"))
}

// ── app grants ────────────────────────────────────────────────────────────

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantBody {
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub portal_app_code: String,
}

/// Grant a portal identity access to one of the client's portal apps
#[utoipa::path(
    post,
    path = "/{id}/apps",
    tag = "portal-users",
    operation_id = "grantPortalUserApp",
    responses((status = 200, description = "Granted")),
    security(("bearer_auth" = []))
)]
pub async fn grant_portal_user_app(
    State(state): State<PortalState>,
    auth: Authenticated,
    Path(id): Path<String>,
    RawBody(raw): RawBody,
) -> ApiResult<Json<MessageResponse>> {
    let req: GrantBody = body(&raw)?;
    let client_id = req.client_id.trim().to_string();
    require_client_id(&client_id)?;
    can_write_portal_users(&auth.0, &client_id)?;
    let app = app_by_code(&state, &client_id, &req.portal_app_code).await?;
    if !app.active {
        return Err(validation(
            "PORTAL_APP_INACTIVE",
            &format!("portal app '{}' is inactive", app.code),
        ));
    }
    let use_case = GrantPortalIdentityAppUseCase::new(
        state.identities.clone(),
        state.apps.clone(),
        state.unit_of_work.clone(),
    );
    let cmd = AppGrantCommand {
        client_id,
        identity_id: id,
        portal_app_id: app.id,
    };
    use_case
        .run(cmd, ExecutionContext::from_auth(&auth.0))
        .await
        .into_result()?;
    Ok(message("Portal app access granted"))
}

/// Revoke a portal identity's access to one portal app (the identity stays)
#[utoipa::path(
    delete,
    path = "/{id}/apps/{portalAppCode}",
    tag = "portal-users",
    operation_id = "revokePortalUserApp",
    responses((status = 200, description = "Revoked")),
    security(("bearer_auth" = []))
)]
pub async fn revoke_portal_user_app(
    State(state): State<PortalState>,
    auth: Authenticated,
    Path((id, portal_app_code)): Path<(String, String)>,
    Query(q): Query<ClientQuery>,
) -> ApiResult<Json<MessageResponse>> {
    let client_id = q.client_id.as_deref().unwrap_or("").trim().to_string();
    require_client_id(&client_id)?;
    can_write_portal_users(&auth.0, &client_id)?;
    let app = app_by_code(&state, &client_id, &portal_app_code).await?;
    let use_case = RevokePortalIdentityAppUseCase::new(
        state.identities.clone(),
        state.apps.clone(),
        state.unit_of_work.clone(),
    );
    let cmd = AppGrantCommand {
        client_id,
        identity_id: id,
        portal_app_id: app.id,
    };
    use_case
        .run(cmd, ExecutionContext::from_auth(&auth.0))
        .await
        .into_result()?;
    Ok(message("Portal app access revoked"))
}

// ── portal apps ───────────────────────────────────────────────────────────

/// One portal app with its OAuth clients and user count (Go
/// `PortalAppResponse`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortalAppResponse {
    pub id: String,
    pub client_id: String,
    pub code: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub active: bool,
    pub oauth_clients: Vec<LinkedOAuthClient>,
    pub user_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// Go `PortalAppListResponse`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortalAppListResponse {
    pub portal_apps: Vec<PortalAppResponse>,
    /// The client's portal users granted no app (only when `clientId` is
    /// given).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unassigned_users: Option<i64>,
}

async fn app_responses(
    state: &PortalState,
    apps: Vec<PortalApp>,
) -> ApiResult<Vec<PortalAppResponse>> {
    let ids: Vec<String> = apps.iter().map(|a| a.id.clone()).collect();
    let (counts, mut linked) = tokio::try_join!(
        state.apps.grant_counts(&ids),
        state.apps.linked_oauth_clients(&ids)
    )?;
    Ok(apps
        .into_iter()
        .map(|a| PortalAppResponse {
            oauth_clients: linked.remove(&a.id).unwrap_or_default(),
            user_count: counts.get(&a.id).copied().unwrap_or(0),
            created_at: micros(&a.created_at),
            updated_at: micros(&a.updated_at),
            id: a.id,
            client_id: a.client_id,
            code: a.code,
            name: a.name,
            description: a.description,
            active: a.active,
        })
        .collect())
}

async fn app_out(state: &PortalState, id: &str) -> ApiResult<PortalAppResponse> {
    let app = state
        .apps
        .find_by_id(id)
        .await?
        .ok_or_else(|| internal("REPO", "portal app lookup failed"))?;
    app_responses(state, vec![app])
        .await?
        .pop()
        .ok_or_else(|| internal("REPO", "portal app lookup failed"))
}

/// List portal apps (a client's, or every client's for anchors)
#[utoipa::path(
    get,
    path = "",
    tag = "portal-apps",
    operation_id = "listPortalApps",
    responses((status = 200, description = "Portal apps")),
    security(("bearer_auth" = []))
)]
pub async fn list_portal_apps(
    State(state): State<PortalState>,
    auth: Authenticated,
    Query(q): Query<ClientQuery>,
) -> ApiResult<Json<PortalAppListResponse>> {
    let client_id = q.client_id.as_deref().unwrap_or("").trim().to_string();
    if client_id.is_empty() {
        if !auth.0.is_anchor() {
            return Err(validation(
                "CLIENT_ID_REQUIRED",
                "clientId query param is required",
            ));
        }
    } else {
        can_read_portal_users(&auth.0, &client_id)?;
    }
    let scope = (!client_id.is_empty()).then_some(client_id.as_str());
    let apps = state.apps.find_by_client(scope).await?;
    let portal_apps = app_responses(&state, apps).await?;
    let unassigned_users = match scope {
        Some(c) => Some(state.identities.count_unassigned(c).await?),
        None => None,
    };
    Ok(Json(PortalAppListResponse {
        portal_apps,
        unassigned_users,
    }))
}

/// Go `CreatePortalAppRequest`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePortalAppRequest {
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub redirect_uris: Vec<String>,
    pub client_type: Option<String>,
}

/// Go `CreatePortalAppResponse`: the app plus its provisioned OAuth client.
/// `clientSecret` (CONFIDENTIAL only) is shown exactly once.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePortalAppResponse {
    pub portal_app: PortalAppResponse,
    pub oauth_client_id: String,
    pub oauth_client_row_id: String,
    pub client_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
}

/// Register a portal app for a client and provision its portal OAuth client
#[utoipa::path(
    post,
    path = "",
    tag = "portal-apps",
    operation_id = "createPortalApp",
    responses((status = 201, description = "Created")),
    security(("bearer_auth" = []))
)]
pub async fn create_portal_app(
    State(state): State<PortalState>,
    auth: Authenticated,
    RawBody(raw): RawBody,
) -> ApiResult<(StatusCode, Json<CreatePortalAppResponse>)> {
    huma_check(
        &raw,
        &["clientId", "code", "name"],
        &[("clientType", &["CONFIDENTIAL", "PUBLIC"])],
    )?;
    let req: CreatePortalAppRequest = body(&raw)?;
    let client_id = req.client_id.trim().to_string();
    require_client_id(&client_id)?;
    can_write_portal_users(&auth.0, &client_id)?;

    let client_type = req.client_type.clone().unwrap_or_default();
    let confidential = client_type.is_empty() || client_type == "CONFIDENTIAL";
    // The secret is generated here so the plaintext can be returned once;
    // only its hash reaches the use case.
    let (secret, secret_ref) = match (confidential, state.encryption_service.as_ref()) {
        (true, Some(enc)) => {
            let plaintext = super::entity::random_token(32);
            let secret_ref = enc.hash_secret(&plaintext);
            (Some(plaintext), Some(secret_ref))
        }
        _ => (None, None),
    };
    let cmd = CreateAppWithOAuthClientCommand {
        client_id,
        code: req.code,
        name: req.name,
        description: req.description,
        redirect_uris: req.redirect_uris,
        client_type: client_type.clone(),
        oauth_client_row_id: crate::shared::tsid::generate(crate::EntityType::OAuthClient),
        oauth_client_id: crate::shared::tsid::generate(crate::EntityType::OAuthClient),
        client_secret_ref: secret_ref,
    };
    let oauth_client_id = cmd.oauth_client_id.clone();
    let oauth_client_row_id = cmd.oauth_client_row_id.clone();
    let ctx = ExecutionContext::from_auth(&auth.0);
    let (apps, clients, oauth_clients) = (
        state.apps.clone(),
        state.clients.clone(),
        state.oauth_clients.clone(),
    );
    let event = state
        .unit_of_work
        .run(|session| async move {
            CreatePortalAppWithOAuthClientUseCase::new(apps, clients, oauth_clients, session)
                .run(cmd, ctx)
                .await
        })
        .await
        .into_result()?;
    let portal_app = app_out(&state, &event.portal_app_id).await?;
    Ok((
        StatusCode::CREATED,
        Json(CreatePortalAppResponse {
            portal_app,
            oauth_client_id,
            oauth_client_row_id,
            client_type: if confidential {
                "CONFIDENTIAL".to_string()
            } else {
                "PUBLIC".to_string()
            },
            client_secret: secret,
        }),
    ))
}

/// Go `UpdatePortalAppRequest`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePortalAppRequest {
    #[serde(default)]
    pub client_id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub active: Option<bool>,
}

/// Update a portal app's name, description, or active flag
#[utoipa::path(
    put,
    path = "/{id}",
    tag = "portal-apps",
    operation_id = "updatePortalApp",
    responses((status = 200, description = "Updated")),
    security(("bearer_auth" = []))
)]
pub async fn update_portal_app(
    State(state): State<PortalState>,
    auth: Authenticated,
    Path(id): Path<String>,
    RawBody(raw): RawBody,
) -> ApiResult<Json<PortalAppResponse>> {
    let req: UpdatePortalAppRequest = body(&raw)?;
    let client_id = req.client_id.trim().to_string();
    require_client_id(&client_id)?;
    can_write_portal_users(&auth.0, &client_id)?;
    let use_case = UpdatePortalAppUseCase::new(state.apps.clone(), state.unit_of_work.clone());
    let cmd = UpdateAppCommand {
        client_id,
        id: id.clone(),
        name: req.name,
        description: req.description,
        active: req.active,
    };
    use_case
        .run(cmd, ExecutionContext::from_auth(&auth.0))
        .await
        .into_result()?;
    Ok(Json(app_out(&state, &id).await?))
}

/// Delete a portal app together with its portal OAuth clients
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "portal-apps",
    operation_id = "deletePortalApp",
    responses((status = 200, description = "Deleted")),
    security(("bearer_auth" = []))
)]
pub async fn delete_portal_app(
    State(state): State<PortalState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Query(q): Query<ClientQuery>,
) -> ApiResult<Json<MessageResponse>> {
    let client_id = q.client_id.as_deref().unwrap_or("").trim().to_string();
    require_client_id(&client_id)?;
    can_write_portal_users(&auth.0, &client_id)?;
    let cmd = DeleteAppCommand { client_id, id };
    let ctx = ExecutionContext::from_auth(&auth.0);
    let (apps, portal_oauth, oauth_clients) = (
        state.apps.clone(),
        state.portal_oauth.clone(),
        state.oauth_clients.clone(),
    );
    let event = state
        .unit_of_work
        .run(|session| async move {
            DeletePortalAppUseCase::new(apps, portal_oauth, oauth_clients, session)
                .run(cmd, ctx)
                .await
        })
        .await
        .into_result()?;
    let mut msg = "Portal app deleted".to_string();
    match event.deleted_oauth_client_ids.len() {
        0 => {}
        1 => msg.push_str(" with its OAuth client"),
        _ => msg.push_str(" with its OAuth clients"),
    }
    Ok(message(msg))
}

/// Go `AssignUnassignedResponse`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignUnassignedResponse {
    pub portal_app_code: String,
    pub assigned: usize,
}

/// Grant a portal app to every one of the client's portal users that has no
/// portal app
#[utoipa::path(
    post,
    path = "/{id}/assign-unassigned",
    tag = "portal-apps",
    operation_id = "assignUnassignedPortalUsers",
    responses((status = 200, description = "Assigned")),
    security(("bearer_auth" = []))
)]
pub async fn assign_unassigned_portal_users(
    State(state): State<PortalState>,
    auth: Authenticated,
    Path(id): Path<String>,
    RawBody(raw): RawBody,
) -> ApiResult<Json<AssignUnassignedResponse>> {
    let req: ClientBody = body(&raw)?;
    let client_id = req.client_id.trim().to_string();
    require_client_id(&client_id)?;
    can_write_portal_users(&auth.0, &client_id)?;
    let cmd = AssignUnassignedCommand {
        client_id,
        portal_app_id: id,
    };
    let ctx = ExecutionContext::from_auth(&auth.0);
    let (identities, apps) = (state.identities.clone(), state.apps.clone());
    let outcome = state
        .unit_of_work
        .run(|session| async move {
            AssignUnassignedPortalIdentitiesUseCase::new(identities, apps, session)
                .run(cmd, ctx)
                .await
        })
        .await
        .into_result();
    match outcome {
        Ok(done) => Ok(Json(AssignUnassignedResponse {
            portal_app_code: done.app_code,
            assigned: done.identity_ids.len(),
        })),
        // Nobody to assign is a success with nothing done, as in Go.
        Err(e) if e.is_unchanged() && e.code() == NOTHING_TO_ASSIGN => {
            let code = e
                .details()
                .get("portalAppCode")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            Ok(Json(AssignUnassignedResponse {
                portal_app_code: code,
                assigned: 0,
            }))
        }
        Err(e) => Err(e.into()),
    }
}

/// `/api/portal-users` routes.
pub fn portal_users_router(state: PortalState) -> Router {
    Router::new()
        .route("/", post(ensure_portal_user).get(list_portal_users))
        .route("/{id}", delete(delete_portal_user))
        .route("/{id}/activate", post(activate_portal_user))
        .route("/{id}/deactivate", post(deactivate_portal_user))
        .route("/{id}/apps", post(grant_portal_user_app))
        .route(
            "/{id}/apps/{portal_app_code}",
            delete(revoke_portal_user_app),
        )
        .with_state(state)
}

/// `/api/portal-apps` routes.
pub fn portal_apps_router(state: PortalState) -> Router {
    Router::new()
        .route("/", get(list_portal_apps).post(create_portal_app))
        .route(
            "/{id}",
            axum::routing::put(update_portal_app).delete(delete_portal_app),
        )
        .route(
            "/{id}/assign-unassigned",
            post(assign_unassigned_portal_users),
        )
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::origin_of;

    #[test]
    fn origins_keep_the_port_and_skip_wildcards() {
        assert_eq!(
            origin_of("https://portal.example.com/callback").as_deref(),
            Some("https://portal.example.com/")
        );
        assert_eq!(
            origin_of("http://localhost:3000/cb").as_deref(),
            Some("http://localhost:3000/")
        );
        assert_eq!(origin_of("https://*.example.com/cb"), None);
        assert_eq!(origin_of("/relative"), None);
    }
}
