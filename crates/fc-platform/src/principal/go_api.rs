//! Principal routes Go serves that Rust lacked (`principal/api/api.go:93,96,115`):
//!
//! - `POST /api/principals/bulk-import`                → `{created, skipped, failed, results}`
//! - `GET  /api/principals/{id}/version`               → `{updatedAt}`
//! - `PUT  /api/principals/{id}/client-association`    → the principal

use axum::{
    extract::{Path, State},
    Json,
};
use chrono::SecondsFormat;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::application::ApplicationClientConfigRepository;
use crate::auth::password_reset_api::PasswordResetEmailer;
use crate::identity_provider::entity::IdentityProviderType;
use crate::principal::api::PrincipalResponse;
use crate::principal::entity::UserScope;
use crate::principal::operations::set_client_association::{
    SetClientAssociationCommand, SetClientAssociationUseCase,
};
use crate::principal::operations::{
    AssignUserRolesCommand, AssignUserRolesUseCase, CreateUserCommand, CreateUserUseCase,
};
use crate::shared::authorization_service::checks;
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::usecase::{ExecutionContext, PgUnitOfWork, UseCase};
use crate::{
    EmailDomainMappingRepository, IdentityProviderRepository, PrincipalRepository, RoleRepository,
};

/// Go's cap on one import.
const MAX_IMPORT_ROWS: usize = 1000;

#[derive(Clone)]
pub struct PrincipalGoState {
    pub principal_repo: Arc<PrincipalRepository>,
    pub role_repo: Arc<RoleRepository>,
    pub edm_repo: Arc<EmailDomainMappingRepository>,
    pub idp_repo: Arc<IdentityProviderRepository>,
    pub client_config_repo: Arc<ApplicationClientConfigRepository>,
    pub emailer: Arc<PasswordResetEmailer>,
    pub create_user_use_case: Arc<CreateUserUseCase<PgUnitOfWork>>,
    pub assign_roles_use_case: Arc<AssignUserRolesUseCase<PgUnitOfWork>>,
    pub set_client_association_use_case: Arc<SetClientAssociationUseCase<PgUnitOfWork>>,
}

/// Go `BulkImportRequest`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BulkImportRequest {
    /// Required, as in Go's huma schema (absent is a 400 `VALIDATION`).
    pub client_id: String,
    pub users: Vec<BulkImportUser>,
}

/// Go `BulkImportUser`: `name` and `email` are required members.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BulkImportUser {
    pub name: String,
    pub email: String,
    #[serde(default)]
    pub roles: Vec<String>,
}

/// Go `BulkImportRowResult`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BulkImportRowResult {
    pub row: usize,
    pub email: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Go `BulkImportResponse`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BulkImportResponse {
    pub created: usize,
    pub skipped: usize,
    pub failed: usize,
    pub results: Vec<BulkImportRowResult>,
}

/// Go `VersionResponse`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PrincipalVersionResponse {
    pub updated_at: String,
}

/// Go `ClientAssociationRequest`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientAssociationRequest {
    pub client_id: String,
    pub mode: Option<String>,
}

fn row(n: usize, email: &str, status: &str, message: Option<String>) -> BulkImportRowResult {
    BulkImportRowResult {
        row: n,
        email: email.to_string(),
        status: status.to_string(),
        message,
    }
}

/// Import users into one client (Go `bulkImportPrincipals`). Anchor callers
/// need a user-write permission; a client administrator also needs access
/// to the client and may hand out only roles of applications the client
/// has enabled. Rows are independent: an existing email is skipped, an
/// email whose domain is registered to another client is dropped.
#[utoipa::path(
    post,
    path = "/api/principals/bulk-import",
    tag = "principals",
    operation_id = "bulkImportPrincipals",
    request_body = BulkImportRequest,
    responses(
        (status = 200, description = "Per-row outcomes", body = BulkImportResponse),
        (status = 400, description = "No client, no rows or too many rows"),
        (status = 403, description = "Not allowed to manage this client's users")
    ),
    security(("bearer_auth" = []))
)]
pub async fn bulk_import_principals(
    State(state): State<PrincipalGoState>,
    auth: Authenticated,
    Json(req): Json<BulkImportRequest>,
) -> Result<Json<BulkImportResponse>, PlatformError> {
    let client_id = req.client_id.trim().to_string();
    if client_id.is_empty() {
        return Err(PlatformError::bad_request_code(
            "CLIENT_REQUIRED",
            "A target client is required",
        ));
    }
    // Go `RequireUserAdmin`: a client administrator must reach the client.
    let anchor = auth.0.is_anchor();
    if !anchor && !auth.0.can_access_client(&client_id) {
        return Err(PlatformError::forbidden_code(
            "SCOPE_FORBIDDEN",
            "no access to this user's client",
        ));
    }
    checks::can_write_principals(&auth.0)?;
    if req.users.is_empty() {
        return Err(PlatformError::bad_request_code(
            "NO_ROWS",
            "No users to import",
        ));
    }
    if req.users.len() > MAX_IMPORT_ROWS {
        return Err(PlatformError::bad_request_code(
            "TOO_MANY",
            "Import is limited to 1000 users at a time",
        ));
    }

    // Normalise every row first, so the lookups batch.
    struct Row {
        email: String,
        name: String,
        roles: Vec<String>,
    }
    let rows: Vec<Row> = req
        .users
        .into_iter()
        .map(|u| {
            let mut seen = HashSet::new();
            Row {
                email: u.email.trim().to_lowercase(),
                name: u.name.trim().to_string(),
                roles: u
                    .roles
                    .into_iter()
                    .map(|r| r.trim().to_string())
                    .filter(|r| !r.is_empty() && seen.insert(r.clone()))
                    .collect(),
            }
        })
        .collect();
    let emails: Vec<String> = rows.iter().map(|r| r.email.clone()).collect();
    let mut role_names: Vec<String> = rows.iter().flat_map(|r| r.roles.clone()).collect();
    role_names.sort();
    role_names.dedup();

    let (existing, mappings, idps, definitions, configs) = tokio::try_join!(
        state.principal_repo.existing_emails(&emails),
        state.edm_repo.find_all(),
        state.idp_repo.find_all(),
        async {
            crate::role::ceiling::definitions(&state.role_repo, &role_names)
                .await
                .map_err(PlatformError::from)
        },
        state.client_config_repo.find_by_client(&client_id),
    )?;
    let existing: HashSet<String> = existing.into_iter().collect();
    let mappings: HashMap<String, _> = mappings
        .into_iter()
        .map(|m| (m.email_domain.clone(), m))
        .collect();
    let idp_types: HashMap<String, IdentityProviderType> =
        idps.into_iter().map(|i| (i.id, i.r#type)).collect();
    let entitled: HashSet<String> = configs
        .into_iter()
        .filter(|c| c.enabled)
        .map(|c| c.application_id)
        .collect();
    let can_assign = checks::can_assign_principal_roles(&auth.0).is_ok();

    let mut results = Vec::with_capacity(rows.len());
    let mut in_file = HashSet::new();
    for (i, r) in rows.into_iter().enumerate() {
        let n = i + 1;
        let email = r.email.as_str();
        if email.is_empty() || !email.contains('@') {
            results.push(row(n, email, "error", Some("invalid email address".into())));
            continue;
        }
        if r.name.is_empty() {
            results.push(row(n, email, "error", Some("name is required".into())));
            continue;
        }
        if !in_file.insert(email.to_string()) {
            results.push(row(
                n,
                email,
                "error",
                Some("duplicate email in file".into()),
            ));
            continue;
        }
        if !r.roles.is_empty() {
            // Go `assertAssignableRoles`, for a client administrator.
            if !anchor {
                let refused = r.roles.iter().find_map(|name| match definitions.get(name) {
                    None => Some(format!("role not found: {name}")),
                    Some(role) => match role.application_id.as_deref() {
                        None => Some("client administrators cannot assign platform roles".into()),
                        Some(app) if !entitled.contains(app) => {
                            Some("role belongs to an application the client cannot access".into())
                        }
                        _ => None,
                    },
                });
                if let Some(msg) = refused {
                    results.push(row(n, email, "error", Some(msg)));
                    continue;
                }
            }
            // Owner ruling 14: role assignment needs its own permission and
            // stays under the caller's ceiling.
            let refused = if can_assign {
                crate::role::ceiling::require_roles(Some(&auth.0), &r.roles, &definitions)
                    .err()
                    .map(|e| e.message().to_string())
            } else {
                Some(format!(
                    "permission required: {}",
                    crate::permissions::iam::USER_ASSIGN_ROLES
                ))
            };
            if let Some(msg) = refused {
                results.push(row(n, email, "error", Some(msg)));
                continue;
            }
        }
        if existing.contains(email) {
            results.push(row(
                n,
                email,
                "exists",
                Some("already exists — skipped".into()),
            ));
            continue;
        }
        let domain = email.split('@').nth(1).unwrap_or_default();
        let mapping = mappings.get(domain);
        if let Some(m) = mapping {
            let owners: Vec<&str> = m
                .primary_client_id
                .iter()
                .map(String::as_str)
                .chain(m.additional_client_ids.iter().map(String::as_str))
                .collect();
            if !owners.is_empty() && !owners.contains(&client_id.as_str()) {
                results.push(row(
                    n,
                    email,
                    "dropped",
                    Some("email domain is registered to another client — skipped".into()),
                ));
                continue;
            }
        }
        let idp_type = mapping
            .and_then(|m| idp_types.get(&m.identity_provider_id).copied())
            .unwrap_or(IdentityProviderType::Internal);

        let cmd = CreateUserCommand {
            email: email.to_string(),
            name: Some(r.name.clone()),
            scope: UserScope::Client,
            client_id: Some(client_id.clone()),
            granted_client_ids: Vec::new(),
            password: None,
            enforce_password_complexity: None,
            idp_type: Some(idp_type),
        };
        let event = match state
            .create_user_use_case
            .run(cmd, ExecutionContext::from_auth(&auth.0))
            .await
            .into_result()
        {
            Ok(e) => e,
            Err(e) => {
                results.push(row(n, email, "error", Some(e.message().to_string())));
                continue;
            }
        };
        let mut message = None;
        if !r.roles.is_empty() {
            if let Err(e) = state
                .assign_roles_use_case
                .run(
                    AssignUserRolesCommand {
                        user_id: event.principal_id.clone(),
                        roles: r.roles.clone(),
                    },
                    ExecutionContext::from_auth(&auth.0),
                )
                .await
                .into_result()
            {
                message = Some(format!("created, but roles not applied: {}", e.message()));
            }
        }
        // Go `notifyNewUser`: an internal, passwordless user gets the
        // set-your-password email; a federated one signs in at its IdP.
        if idp_type == IdentityProviderType::Internal {
            if let Ok(Some(created)) = state.principal_repo.find_by_id(&event.principal_id).await {
                if let Err(e) = state.emailer.send_reset_email(&created).await {
                    tracing::error!(principal_id = %created.id, error = %e,
                        "Imported user created but the set-password email failed to send");
                }
            }
        }
        results.push(row(n, email, "created", message));
    }

    let count = |s: &str| results.iter().filter(|r| r.status == s).count();
    Ok(Json(BulkImportResponse {
        created: count("created"),
        skipped: count("exists") + count("dropped"),
        failed: count("error"),
        results,
    }))
}

/// When a principal (or one of its roles) last changed (Go
/// `getPrincipalVersion`). A caller may always read its own; anyone else
/// needs `platform:iam:user:view` and reach to the principal's client.
#[utoipa::path(
    get,
    path = "/api/principals/{id}/version",
    tag = "principals",
    operation_id = "getPrincipalVersion",
    params(("id" = String, Path, description = "Principal id")),
    responses(
        (status = 200, description = "The version", body = PrincipalVersionResponse),
        (status = 404, description = "Not found or out of reach")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_principal_version(
    State(state): State<PrincipalGoState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<PrincipalVersionResponse>, PlatformError> {
    if auth.0.principal_id != id {
        checks::require_permission(&auth.0, crate::permissions::iam::USER_READ)?;
        let p = state
            .principal_repo
            .find_by_id(&id)
            .await?
            .ok_or_else(|| PlatformError::not_found_code("Principal", &id))?;
        if let Some(client) = p.client_id.as_deref() {
            if !auth.0.can_access_client(client) {
                return Err(PlatformError::not_found_code("Principal", &id));
            }
        }
    }
    let version = state
        .principal_repo
        .lookup_version(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found_code("Principal", &id))?;
    Ok(Json(PrincipalVersionResponse {
        updated_at: version.to_rfc3339_opts(SecondsFormat::Micros, true),
    }))
}

/// Move a user between client tiers (Go `setPrincipalClientAssociation`).
/// Go asks anchor scope alone; this reassigns tenancy and grants client
/// access, so Rust asks anchor and `platform:iam:client-access:grant`
/// (owner decision #25).
#[utoipa::path(
    put,
    path = "/api/principals/{id}/client-association",
    tag = "principals",
    operation_id = "setPrincipalClientAssociation",
    params(("id" = String, Path, description = "Principal id")),
    request_body = ClientAssociationRequest,
    responses(
        (status = 200, description = "The principal", body = PrincipalResponse),
        (status = 400, description = "No client, or no mode for a specific client"),
        (status = 404, description = "Unknown user or client"),
        (status = 409, description = "Not a user")
    ),
    security(("bearer_auth" = []))
)]
pub async fn set_principal_client_association(
    State(state): State<PrincipalGoState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<ClientAssociationRequest>,
) -> Result<Json<PrincipalResponse>, PlatformError> {
    checks::can_grant_client_access(&auth.0)?;
    Ok(Json(client_association(&state, &auth.0, &id, req).await?))
}

/// The body of `PUT /api/principals/{id}/client-association`, shared with
/// the server-rendered `fc-web` UI.
pub async fn client_association(
    state: &PrincipalGoState,
    ctx: &crate::AuthContext,
    id: &str,
    req: ClientAssociationRequest,
) -> Result<PrincipalResponse, PlatformError> {
    checks::can_grant_client_access(ctx)?;
    state
        .set_client_association_use_case
        .run(
            SetClientAssociationCommand {
                user_id: id.to_string(),
                client_id: req.client_id,
                mode: req.mode,
            },
            ExecutionContext::from_auth(ctx),
        )
        .await
        .into_result()?;
    let p = state
        .principal_repo
        .find_by_id(id)
        .await?
        .ok_or_else(|| PlatformError::not_found_code("Principal", id))?;
    Ok(p.into())
}

/// Full-path router; merged at the root.
pub fn principal_go_router(state: PrincipalGoState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(bulk_import_principals))
        .routes(routes!(get_principal_version))
        .routes(routes!(set_principal_client_association))
        .with_state(state)
}
