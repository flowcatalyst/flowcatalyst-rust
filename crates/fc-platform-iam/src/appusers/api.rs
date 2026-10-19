//! Application users and invitations (Go F20, internal/platform/appusers/api.go).
//!
//! The user and invitation administration an application performs for its own
//! users, as its own service account. It backs the application admin pages of
//! the framework: list and read the application's users, invite one, and
//! list, resend and revoke pending invitations. Everything is confined to the
//! applications the credential is bound to; see [`super::repository`] for who
//! counts as the application's user.

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::repository::{AppUsersRepository, Invitation, ListQuery, Scope, User};
use crate::application::client_config_repository::ApplicationClientConfigRepository;
use crate::application::entity::Application;
use crate::auth::password_reset_emailer::PasswordResetEmailer;
use crate::client::repository::ClientRepository;
use crate::email_domain_mapping::repository::EmailDomainMappingRepository;
use crate::identity_provider::entity::IdentityProviderType;
use crate::identity_provider::repository::IdentityProviderRepository;
use crate::principal::api::resolve_invite_redirect;
use crate::principal::entity::Principal;
use crate::principal::operations::invite_application_user::CLIENT_REFUSED;
use crate::principal::operations::{
    DeactivateUserCommand, DeactivateUserUseCase, InviteApplicationUserCommand,
    InviteApplicationUserUseCase,
};
use crate::principal::repository::PrincipalRepository;
use crate::shared::authorization_service::ApplicationAccessService;
use fc_platform_core::shared::authorization_service::{checks, AuthContext};
use fc_platform_core::shared::caller_reach;
use fc_platform_core::shared::error::PlatformError;
use fc_platform_core::shared::id::{ClientId, PrincipalId};
use fc_platform_core::shared::middleware::Authenticated;
use fc_platform_core::usecase::{ExecutionContext, PgUnitOfWork, UseCase};

const DEFAULT_PAGE_SIZE: i64 = 50;
const MAX_PAGE_SIZE: i64 = 200;

/// What the application user routes work on.
#[derive(Clone)]
pub struct AppUsersState {
    pub repo: Arc<AppUsersRepository>,
    pub principal_repo: Arc<PrincipalRepository>,
    /// Resolves `{appCode}` and confines the caller to its applications.
    pub app_access: Arc<ApplicationAccessService>,
    pub client_repo: Arc<ClientRepository>,
    pub client_config_repo: Arc<ApplicationClientConfigRepository>,
    /// Tell whether an invitee's email domain is federated, whose users are
    /// never sent a set-password invitation.
    pub email_domain_mapping_repo: Arc<EmailDomainMappingRepository>,
    pub identity_provider_repo: Arc<IdentityProviderRepository>,
    /// Mints and delivers the first-time set-password invitation. Minting
    /// replaces the user's earlier tokens, so a resend replaces the old link.
    pub emailer: Arc<PasswordResetEmailer>,
    pub invite_use_case: Arc<InviteApplicationUserUseCase<PgUnitOfWork>>,
    pub deactivate_use_case: Arc<DeactivateUserUseCase<PgUnitOfWork>>,
}

// ── wire types ───────────────────────────────────────────────────────────

fn micros(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Micros, true)
}

/// One of an application's users. No password hash, token or second-factor
/// state is ever included.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = ApplicationUser)]
pub struct ApplicationUserDto {
    pub id: String,
    pub email: String,
    pub name: String,
    pub active: bool,
    pub client_id: String,
    /// `INTERNAL` (a platform password) or `OIDC` (federated).
    pub idp_type: String,
    /// The application's own roles the user holds (`app:role`).
    pub roles: Vec<String>,
    /// `PENDING` or `EXPIRED` while the user is awaiting an invitation (has
    /// not set a password); absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invitation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(format = DateTime)]
    pub last_login_at: Option<String>,
    #[schema(format = DateTime)]
    pub created_at: String,
}

impl From<User> for ApplicationUserDto {
    fn from(u: User) -> Self {
        Self {
            id: u.id,
            email: u.email,
            name: u.name,
            active: u.active,
            client_id: u.client_id,
            idp_type: u.idp_type,
            roles: u.roles,
            invitation: u.invitation.map(str::to_string),
            last_login_at: u.last_login_at.map(micros),
            created_at: micros(u.created_at),
        }
    }
}

/// A page of users.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = ApplicationUserList)]
pub struct ApplicationUserListDto {
    pub users: Vec<ApplicationUserDto>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

/// One outstanding invitation. The invite link is never listed; it is
/// returned only at creation or resend, to the caller that asked for it.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = ApplicationInvitation)]
pub struct ApplicationInvitationDto {
    pub user_id: String,
    pub email: String,
    pub name: String,
    pub client_id: String,
    pub roles: Vec<String>,
    /// `PENDING` (a live link exists) or `EXPIRED` (it lapsed: resend).
    pub status: String,
    #[schema(format = DateTime)]
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(format = DateTime)]
    pub expires_at: Option<String>,
}

impl From<Invitation> for ApplicationInvitationDto {
    fn from(i: Invitation) -> Self {
        Self {
            user_id: i.principal_id,
            email: i.email,
            name: i.name,
            client_id: i.client_id,
            roles: i.roles,
            status: i.status.to_string(),
            created_at: micros(i.created_at),
            expires_at: i.expires_at.map(micros),
        }
    }
}

/// A page of invitations.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = ApplicationInvitationList)]
pub struct ApplicationInvitationListDto {
    pub invitations: Vec<ApplicationInvitationDto>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

/// Body of `POST /api/applications/{appCode}/users`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = InviteApplicationUserRequest)]
pub struct InviteApplicationUserRequest {
    pub email: String,
    pub name: String,
    /// The user's home client: a `clt_` id or the client's identifier. The
    /// application must be enabled for it.
    pub client_id: String,
    /// The application's roles to give the user (`app:role`, or the short
    /// name). At least one: the roles are what make the user the
    /// application's.
    #[schema(min_items = 1)]
    pub roles: Vec<String>,
    /// Default true: the platform emails the invitation.
    #[serde(default)]
    pub send_invitation: Option<bool>,
    /// When true, returns the live 72h link as `inviteLink` and suppresses the
    /// platform email. A bearer credential: never log it.
    #[serde(default)]
    pub return_invite_link: Option<bool>,
    /// Where the invitee is sent after setting a password (the application's
    /// own page; any absolute http(s) URL).
    #[serde(default)]
    pub invite_redirect_uri: Option<String>,
}

/// The answer to an invitation.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = InviteApplicationUserResponse)]
pub struct InviteApplicationUserResponse {
    pub id: String,
    /// The live set-password link, only when `returnInviteLink` was asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invite_link: Option<String>,
    /// True when the platform emailed the invitation. False when the caller
    /// asked for the link or no email, the user is federated (no password to
    /// set), or delivery failed (resend it).
    pub invitation_sent: bool,
}

/// Body of the resend route; every field optional.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = ResendInvitationRequest)]
pub struct ResendInvitationRequest {
    #[serde(default)]
    pub send_invitation: Option<bool>,
    #[serde(default)]
    pub return_invite_link: Option<bool>,
    #[serde(default)]
    pub invite_redirect_uri: Option<String>,
}

/// The answer to a resend.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = ResendInvitationResponse)]
pub struct ResendInvitationResponse {
    /// Set only when `returnInviteLink` was asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invite_link: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(format = DateTime)]
    pub expires_at: Option<String>,
}

// ── access ───────────────────────────────────────────────────────────────

/// The caller's scope over `app`'s users: a caller that is not an anchor
/// keeps its own client reach on top of the application's.
fn scope_of(auth: &AuthContext, app: &Application) -> Scope {
    Scope {
        application_id: app.id.as_str().to_string(),
        application_code: app.code.clone(),
        client_ids: (!auth.is_anchor()).then(|| caller_reach::client_ids(auth)),
    }
}

fn page_of(page: Option<i64>, size: Option<i64>) -> (i64, i64) {
    let size = match size {
        Some(s) if s > 0 => s.min(MAX_PAGE_SIZE),
        _ => DEFAULT_PAGE_SIZE,
    };
    (page.unwrap_or(0).max(0), size)
}

fn split_csv(raw: Option<&str>) -> Vec<String> {
    raw.unwrap_or_default()
        .split(',')
        .map(|p| p.trim().to_lowercase())
        .filter(|p| !p.is_empty())
        .collect()
}

/// Keeps the role names that belong to the application, prefixing a short
/// name with the application code.
fn own_roles(app_code: &str, names: Vec<String>) -> Vec<String> {
    let prefix = format!("{}:", app_code.to_lowercase());
    names
        .into_iter()
        .map(|n| {
            if n.contains(':') {
                n
            } else {
                format!("{prefix}{n}")
            }
        })
        .filter(|n| n.starts_with(&prefix))
        .collect()
}

// ── users ────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ListUsersQuery {
    /// Case-insensitive substring of name or email
    pub q: Option<String>,
    /// true | false; absent = both
    pub active: Option<String>,
    /// Only users homed at this client
    pub client_id: Option<String>,
    /// CSV of the application's roles; users holding any
    pub roles: Option<String>,
    /// 0-based page (default 0)
    pub page: Option<i64>,
    /// Page size (default 50, max 200)
    pub page_size: Option<i64>,
    /// name | email | createdAt (default createdAt)
    pub sort_field: Option<String>,
    /// asc | desc (default asc)
    pub sort_order: Option<String>,
}

/// List an application's users (CLIENT users of its enabled clients holding its roles)
#[utoipa::path(
    get,
    path = "/{appCode}/users",
    tag = "application-users",
    operation_id = "listApplicationUsers",
    params(("appCode" = String, Path, description = "Application code"), ListUsersQuery),
    responses((status = 200, description = "OK", body = ApplicationUserListDto)),
    security(("bearer_auth" = []))
)]
pub async fn list_users(
    State(state): State<AppUsersState>,
    auth: Authenticated,
    Path(app_code): Path<String>,
    Query(query): Query<ListUsersQuery>,
) -> Result<Json<ApplicationUserListDto>, PlatformError> {
    checks::can_read_application_users(&auth.0)?;
    // The SDK syncs' rule: an application outside the caller's scope and a
    // missing one are the same 404.
    let app = state
        .app_access
        .require_application_access(&auth.0, &app_code)
        .await?;
    let scope = scope_of(&auth.0, &app);
    let (page, page_size) = page_of(query.page, query.page_size);
    let mut q = ListQuery {
        q: query.q.as_deref().unwrap_or_default().trim().to_string(),
        client_id: query
            .client_id
            .as_deref()
            .unwrap_or_default()
            .trim()
            .to_string(),
        sort: query.sort_field.unwrap_or_default(),
        desc: query
            .sort_order
            .as_deref()
            .is_some_and(|o| o.eq_ignore_ascii_case("desc")),
        page,
        page_size,
        ..ListQuery::default()
    };
    match query.active.as_deref() {
        Some("true") => q.active = Some(true),
        Some("false") => q.active = Some(false),
        _ => {}
    }
    // Only the application's own roles can narrow its list: a foreign role
    // name matches nothing rather than revealing who holds it.
    let wanted = split_csv(query.roles.as_deref());
    if !wanted.is_empty() {
        let own = own_roles(&app.code, wanted);
        if own.is_empty() {
            return Ok(Json(ApplicationUserListDto {
                users: Vec::new(),
                total: 0,
                page,
                page_size,
            }));
        }
        q.roles = own;
    }
    let (users, total) = state.repo.list(&scope, &q).await?;
    Ok(Json(ApplicationUserListDto {
        users: users.into_iter().map(Into::into).collect(),
        total,
        page,
        page_size,
    }))
}

/// Get one of an application's users
#[utoipa::path(
    get,
    path = "/{appCode}/users/{id}",
    tag = "application-users",
    operation_id = "getApplicationUser",
    params(
        ("appCode" = String, Path, description = "Application code"),
        ("id" = String, Path)
    ),
    responses((status = 200, description = "OK", body = ApplicationUserDto)),
    security(("bearer_auth" = []))
)]
pub async fn get_user(
    State(state): State<AppUsersState>,
    auth: Authenticated,
    Path((app_code, id)): Path<(String, String)>,
) -> Result<Json<ApplicationUserDto>, PlatformError> {
    checks::can_read_application_users(&auth.0)?;
    let app = state
        .app_access
        .require_application_access(&auth.0, &app_code)
        .await?;
    let scope = scope_of(&auth.0, &app);
    let user = state
        .repo
        .get(&scope, &id)
        .await?
        .ok_or_else(|| PlatformError::not_found("User", &id))?;
    Ok(Json(user.into()))
}

/// Resolves the request's client reference (`clt_` id or identifier) to an
/// id, and requires the application to be enabled for it and the caller to
/// reach it. Every failure is the same 403.
async fn invitable_client(
    state: &AppUsersState,
    auth: &AuthContext,
    app: &Application,
    reference: &str,
) -> Result<ClientId, PlatformError> {
    let reference = reference.trim();
    if reference.is_empty() {
        return Err(PlatformError::bad_request_code(
            "CLIENT_REQUIRED",
            "clientId is required",
        ));
    }
    let mut client = state
        .client_repo
        .find_by_id(&ClientId::from_wire(reference))
        .await?;
    if client.is_none() {
        client = state
            .client_repo
            .find_by_identifier(&reference.to_lowercase())
            .await?;
    }
    let refused = || PlatformError::forbidden(CLIENT_REFUSED);
    let client = client.ok_or_else(refused)?;
    if !caller_reach::reaches_client(auth, &client.id) {
        return Err(refused());
    }
    let enabled = state
        .client_config_repo
        .find_by_application_and_client(&app.id, &client.id)
        .await?
        .is_some_and(|cfg| cfg.enabled);
    if !enabled {
        return Err(refused());
    }
    Ok(client.id)
}

/// Whether the email's domain is mapped to an OIDC identity provider
/// (`Oidc`), `Internal` otherwise (as create-user does).
async fn idp_type_for(state: &AppUsersState, email: &str) -> IdentityProviderType {
    let Some((_, domain)) = email.rsplit_once('@') else {
        return IdentityProviderType::Internal;
    };
    let Ok(Some(mapping)) = state
        .email_domain_mapping_repo
        .find_by_email_domain(&domain.trim().to_lowercase())
        .await
    else {
        return IdentityProviderType::Internal;
    };
    match state
        .identity_provider_repo
        .find_by_id(&mapping.identity_provider_id)
        .await
    {
        Ok(Some(idp)) if idp.r#type == IdentityProviderType::Oidc => IdentityProviderType::Oidc,
        _ => IdentityProviderType::Internal,
    }
}

/// Mints the invitation: `returnInviteLink` gives the link back (and
/// suppresses the email: the token can only be minted once per delivery),
/// otherwise `sendInvitation` emails it. Failures are logged, never returned
/// to the caller as success: `(None, false)` is "nothing delivered".
async fn deliver(
    state: &AppUsersState,
    p: &Principal,
    send: bool,
    return_link: bool,
    redirect: Option<String>,
) -> (Option<String>, bool) {
    if return_link {
        return match state.emailer.invite_link(p, redirect).await {
            Ok(link) => (link, false),
            Err(e) => {
                tracing::warn!(principal_id = %p.id, error = %e, "mint invite link failed");
                (None, false)
            }
        };
    }
    if send {
        return match state.emailer.send_invite(p, redirect).await {
            Ok(()) => (None, true),
            Err(e) => {
                tracing::warn!(principal_id = %p.id, error = %e, "send application invite failed");
                (None, false)
            }
        };
    }
    (None, false)
}

/// Invite (create) a user holding the application's roles at one of its enabled clients
#[utoipa::path(
    post,
    path = "/{appCode}/users",
    tag = "application-users",
    operation_id = "inviteApplicationUser",
    params(("appCode" = String, Path, description = "Application code")),
    request_body = InviteApplicationUserRequest,
    responses((status = 201, description = "Created", body = InviteApplicationUserResponse)),
    security(("bearer_auth" = []))
)]
pub async fn invite_user(
    State(state): State<AppUsersState>,
    auth: Authenticated,
    Path(app_code): Path<String>,
    Json(req): Json<InviteApplicationUserRequest>,
) -> Result<(StatusCode, Json<InviteApplicationUserResponse>), PlatformError> {
    checks::can_create_application_users(&auth.0)?;
    let app = state
        .app_access
        .require_application_access(&auth.0, &app_code)
        .await?;
    let client_id = invitable_client(&state, &auth.0, &app, &req.client_id).await?;
    // Validated before anything is written: a rejected redirect must not leave
    // behind a user whose invite was never minted.
    let redirect = resolve_invite_redirect(req.invite_redirect_uri.as_deref())?;
    let idp_type = idp_type_for(&state, &req.email).await;

    let command = InviteApplicationUserCommand {
        application_code: app.code.clone(),
        email: req.email.clone(),
        name: req.name.clone(),
        client_id: client_id.into_string(),
        roles: req.roles.clone(),
        idp_type: Some(idp_type),
    };
    let event = state
        .invite_use_case
        .run(
            command,
            ExecutionContext::from_auth(&auth.0)
                .with_application_scope(state.app_access.scope_for(&auth.0.principal_id).await?),
        )
        .await
        .into_result()?;
    let mut resp = InviteApplicationUserResponse {
        id: event.principal_id.as_str().to_string(),
        invite_link: None,
        invitation_sent: false,
    };
    if idp_type == IdentityProviderType::Oidc {
        return Ok((StatusCode::CREATED, Json(resp)));
    }
    let Some(created) = state.principal_repo.find_by_id(&event.principal_id).await? else {
        tracing::warn!(
            principal_id = %event.principal_id,
            "invited user not reloaded; invitation not delivered"
        );
        return Ok((StatusCode::CREATED, Json(resp)));
    };
    let (link, sent) = deliver(
        &state,
        &created,
        req.send_invitation.unwrap_or(true),
        req.return_invite_link.unwrap_or(false),
        redirect,
    )
    .await;
    resp.invite_link = link;
    resp.invitation_sent = sent;
    Ok((StatusCode::CREATED, Json(resp)))
}

// ── invitations ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct ListInvitationsQuery {
    /// PENDING | EXPIRED; absent = both
    pub status: Option<String>,
    /// 0-based page (default 0)
    pub page: Option<i64>,
    /// Page size (default 50, max 200)
    pub page_size: Option<i64>,
}

/// List an application's outstanding invitations (users who have not set a password)
#[utoipa::path(
    get,
    path = "/{appCode}/invitations",
    tag = "application-users",
    operation_id = "listApplicationInvitations",
    params(("appCode" = String, Path, description = "Application code"), ListInvitationsQuery),
    responses((status = 200, description = "OK", body = ApplicationInvitationListDto)),
    security(("bearer_auth" = []))
)]
pub async fn list_invitations(
    State(state): State<AppUsersState>,
    auth: Authenticated,
    Path(app_code): Path<String>,
    Query(query): Query<ListInvitationsQuery>,
) -> Result<Json<ApplicationInvitationListDto>, PlatformError> {
    checks::can_read_application_invitations(&auth.0)?;
    let app = state
        .app_access
        .require_application_access(&auth.0, &app_code)
        .await?;
    let scope = scope_of(&auth.0, &app);
    let status = query
        .status
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_uppercase();
    if !status.is_empty() && status != "PENDING" && status != "EXPIRED" {
        return Err(PlatformError::bad_request_code(
            "INVALID_STATUS",
            "status must be PENDING or EXPIRED",
        ));
    }
    let (page, page_size) = page_of(query.page, query.page_size);
    let (rows, total) = state
        .repo
        .invitations(&scope, &status, page, page_size)
        .await?;
    Ok(Json(ApplicationInvitationListDto {
        invitations: rows.into_iter().map(Into::into).collect(),
        total,
        page,
        page_size,
    }))
}

/// Resend an application user's invitation (replaces the earlier link)
#[utoipa::path(
    post,
    path = "/{appCode}/invitations/{id}/resend",
    tag = "application-users",
    operation_id = "resendApplicationInvitation",
    params(
        ("appCode" = String, Path, description = "Application code"),
        ("id" = String, Path, description = "The invited user's id")
    ),
    request_body = ResendInvitationRequest,
    responses((status = 200, description = "OK", body = ResendInvitationResponse)),
    security(("bearer_auth" = []))
)]
pub async fn resend_invitation(
    State(state): State<AppUsersState>,
    auth: Authenticated,
    Path((app_code, id)): Path<(String, String)>,
    body: Bytes,
) -> Result<Json<ResendInvitationResponse>, PlatformError> {
    checks::can_update_application_invitations(&auth.0)?;
    let app = state
        .app_access
        .require_application_access(&auth.0, &app_code)
        .await?;
    let scope = scope_of(&auth.0, &app);
    state
        .repo
        .get_invitation(&scope, &id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Invitation", &id))?;
    // Every field is optional, so an empty body is the defaults (Go's huma
    // declares the body but nothing in it is required).
    let req: ResendInvitationRequest = if body.iter().all(u8::is_ascii_whitespace) {
        ResendInvitationRequest::default()
    } else {
        serde_json::from_slice(&body).map_err(|e| {
            PlatformError::bad_request_code("INVALID_JSON", format!("invalid request body: {e}"))
        })?
    };
    let redirect = resolve_invite_redirect(req.invite_redirect_uri.as_deref())?;
    let send = req.send_invitation.unwrap_or(true);
    let return_link = req.return_invite_link.unwrap_or(false);
    if !send && !return_link {
        return Err(PlatformError::bad_request_code(
            "INVITATION_DELIVERY_REQUIRED",
            "a resend needs sendInvitation or returnInviteLink",
        ));
    }
    let principal = state
        .principal_repo
        .find_by_id(&PrincipalId::from_wire(&id))
        .await?
        .ok_or_else(|| PlatformError::not_found("Invitation", &id))?;
    let (link, sent) = deliver(&state, &principal, send, return_link, redirect).await;
    // A resend that delivered nothing is an error, never a silent success: the
    // administrator would believe the invitee has a fresh link.
    if link.is_none() && !sent {
        return Err(PlatformError::Coded {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "INVITE_DELIVERY_FAILED".to_string(),
            message: "the invitation could not be delivered; try again".to_string(),
            details: Default::default(),
        });
    }
    let expires_at = state
        .repo
        .get_invitation(&scope, &id)
        .await?
        .and_then(|fresh| fresh.expires_at)
        .map(micros);
    Ok(Json(ResendInvitationResponse {
        invite_link: link,
        expires_at,
    }))
}

/// Revoke an application user's invitation (removes the link and deactivates the user)
#[utoipa::path(
    delete,
    path = "/{appCode}/invitations/{id}",
    tag = "application-users",
    operation_id = "revokeApplicationInvitation",
    params(
        ("appCode" = String, Path, description = "Application code"),
        ("id" = String, Path)
    ),
    responses((status = 204, description = "No Content")),
    security(("bearer_auth" = []))
)]
pub async fn revoke_invitation(
    State(state): State<AppUsersState>,
    auth: Authenticated,
    Path((app_code, id)): Path<(String, String)>,
) -> Result<StatusCode, PlatformError> {
    checks::can_update_application_invitations(&auth.0)?;
    let app = state
        .app_access
        .require_application_access(&auth.0, &app_code)
        .await?;
    let scope = scope_of(&auth.0, &app);
    state
        .repo
        .get_invitation(&scope, &id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Invitation", &id))?;
    // Deactivate first: a passwordless active user can ask for a fresh
    // set-password link at the login page, so deleting the link alone would
    // not revoke anything. A retry after a partial failure finds the user
    // deactivated (no longer an invitee) and the link, if it survived,
    // expired by its own TTL.
    state
        .deactivate_use_case
        .run(
            DeactivateUserCommand {
                principal_id: PrincipalId::from_wire(&id),
                reason: None,
            },
            ExecutionContext::from_auth(&auth.0),
        )
        .await
        .into_result()?;
    state.repo.revoke_invites(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_defaults_and_clamps() {
        assert_eq!(page_of(None, None), (0, 50));
        assert_eq!(page_of(Some(-3), Some(0)), (0, 50));
        assert_eq!(page_of(Some(2), Some(100_000)), (2, 200));
        assert_eq!(page_of(Some(1), Some(7)), (1, 7));
    }

    #[test]
    fn only_the_applications_roles_survive() {
        let own = own_roles(
            "Orders",
            split_csv(Some("Admin, orders:clerk, billing:admin, ,")),
        );
        assert_eq!(own, vec!["orders:admin", "orders:clerk"]);
        assert!(own_roles("orders", split_csv(Some("billing:clerk"))).is_empty());
    }
}
