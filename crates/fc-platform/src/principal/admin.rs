//! The bodies of the `/api/principals` handlers, after the axum extractors.
//!
//! Each function here is what its handler in [`super::api`] runs once the
//! request is parsed: the permission check, the per-resource reach rules,
//! the use case, and the answer. The handlers are thin wrappers around
//! them, and the server-rendered `fc-web` UI calls the same functions, so
//! both surfaces enforce the same rules and write through the same use
//! cases.

use std::collections::HashSet;

use crate::identity_provider::entity::IdentityProviderType;
use crate::principal::api::{
    assert_assignable_roles, assignment_source_label, bounded_role_set, client_application_ids,
    derive_user_scope, load_administered_user, load_role_administered_user, notify_new_user,
    resolve_client_ref, resolve_invite_redirect, ApplicationAccessListResponse,
    ApplicationAccessResponse, AvailableApplicationsResponse, BatchAssignRolesResponse,
    CheckEmailDomainResponse, ClientAccessGrantResponse, ClientAccessListResponse,
    CreateUserRequest, PrincipalListResponse, PrincipalResponse, PrincipalsQuery, PrincipalsState,
    ResetPasswordRequest, RoleAssignmentDto, RolesListResponse, SetApplicationAccessRequest,
    SetApplicationAccessResponse, StatusChangeResponse, UpdatePrincipalRequest,
};
use crate::principal::entity::UserScope;
use crate::shared::enum_str::parse_opt;
use crate::shared::error::{NotFoundExt, PlatformError};
use crate::usecase::{ExecutionContext, UseCase};
use crate::AuthContext;

/// `POST /api/principals/users`: create a user, the tier derived from the
/// requested scope and the email domain (Go `createUser`).
pub async fn create_user(
    state: &PrincipalsState,
    ctx: &AuthContext,
    req: CreateUserRequest,
) -> Result<PrincipalResponse, PlatformError> {
    use crate::principal::operations::{CreateUserCommand, GrantClientAccessCommand};

    // Go `createUser`: the user-write permission first; the tier bound once
    // the scope is derived below.
    crate::checks::can_write_principals(ctx)?;

    let domain = req
        .email
        .split('@')
        .nth(1)
        .ok_or_else(|| PlatformError::validation("Invalid email format"))?
        .to_lowercase();

    let is_anchor_domain = state.anchor_domain_repo.is_anchor_domain(&domain).await?;

    let mapping = state
        .email_domain_mapping_repo
        .find_by_email_domain(&domain)
        .await?;

    // Resolve IdP type (INTERNAL / OIDC) so the use case can key its password
    // handling off it. Unmapped domains default to INTERNAL — they can only
    // log in through embedded auth anyway.
    let idp_type = match &mapping {
        Some(m) => state
            .identity_provider_repo
            .find_by_id(&m.identity_provider_id)
            .await?
            .map_or(IdentityProviderType::Internal, |idp| idp.r#type),
        None => IdentityProviderType::Internal,
    };

    // Resolve the client reference (clt_ id or identifier) before the tier,
    // so mapping allow-lists compare canonical ids (Go createUser,
    // principal/api/api.go:594-606).
    let req_client_id = match req.client_id.as_deref().map(str::trim) {
        Some(r) if !r.is_empty() => Some(resolve_client_ref(state, r).await?),
        _ => None,
    };
    let (scope, primary_client_id) = derive_user_scope(
        req.scope.as_deref(),
        is_anchor_domain,
        mapping.as_ref(),
        req_client_id,
    )?;

    // Anchors create any scope and client. A client administrator creates
    // CLIENT-tier users only, in a client it reaches (Go `createUser` +
    // `RequireUserAdmin`, principal/api/api.go:608-616).
    if !ctx.is_anchor() && scope != UserScope::Client {
        return Err(PlatformError::forbidden(
            "Client administrators can only create client-scope users",
        ));
    }
    crate::checks::require_user_admin(ctx, primary_client_id.as_deref())?;
    // Before anything is written: a rejected redirect must not leave a user
    // whose invite was never minted (Go createUser).
    let invite_redirect = resolve_invite_redirect(req.invite_redirect_uri.as_deref())?;

    // Partner-merge: if a user already exists for this email, emit a
    // ClientAccessGranted event via the grant use case rather than a fresh
    // UserCreated. Keeps events + audit logs accurate.
    let granted_client_ids = if scope == UserScope::Partner {
        let client_id = primary_client_id.clone().unwrap_or_default();
        if let Some(existing) = state.principal_repo.find_by_email(&req.email).await? {
            let already_linked = existing.client_id.as_deref() == Some(client_id.as_str())
                || existing.assigned_clients.iter().any(|c| c == &client_id);
            if already_linked {
                return Err(PlatformError::duplicate("Principal", "email", &req.email));
            }
            let cmd = GrantClientAccessCommand {
                user_id: existing.id.clone(),
                client_id: client_id.clone(),
            };
            let exec = ExecutionContext::create(&ctx.principal_id);
            state
                .grant_client_access_use_case
                .run(cmd, exec)
                .await
                .into_result()?;
            let refreshed = state
                .principal_repo
                .find_by_id(&existing.id)
                .await?
                .or_not_found("Principal", &existing.id)?;
            return Ok(refreshed.into());
        }
        // New partner user — home client + single grant for the requested
        // client.
        vec![client_id]
    } else {
        Vec::new()
    };

    let cmd = CreateUserCommand {
        email: req.email.clone(),
        name: Some(req.name.clone()),
        scope,
        client_id: primary_client_id,
        granted_client_ids,
        password: req.password.clone(),
        enforce_password_complexity: req.enforce_password_complexity,
        idp_type: Some(idp_type),
    };
    let exec = ExecutionContext::create(&ctx.principal_id);
    let event = state
        .create_user_use_case
        .run(cmd, exec)
        .await
        .into_result()?;

    let created = state
        .principal_repo
        .find_by_id(&event.principal_id)
        .await?
        .or_not_found("Principal", &event.principal_id)?;

    let invite_link = notify_new_user(
        state,
        &created,
        req.password.as_deref(),
        req.send_invitation.unwrap_or(true),
        req.return_invite_link.unwrap_or(false),
        invite_redirect,
    )
    .await;
    let mut response = PrincipalResponse::from(created);
    response.invite_link = invite_link;
    Ok(response)
}

/// `GET /api/principals/{id}`, with the confirmed second factors.
pub async fn detail(
    state: &PrincipalsState,
    ctx: &AuthContext,
    id: &str,
) -> Result<PrincipalResponse, PlatformError> {
    // Go `getByID` (principal/api/api.go:288-323): a principal reads itself
    // with no permission; anyone else needs the user read permission, and a
    // principal of a client the caller does not reach answers the same 404
    // as a missing one.
    let is_self = ctx.principal_id == id;
    if !is_self {
        crate::checks::can_read_principals(ctx)?;
    }
    let principal = state
        .principal_repo
        .find_by_id(id)
        .await?
        .or_not_found("Principal", id)?;
    if !is_self {
        if let Some(cid) = principal.client_id.as_deref() {
            if !crate::shared::caller_reach::reaches_client(ctx, cid) {
                return Err(PlatformError::not_found("Principal", id));
            }
        }
    }

    // Go enriches the detail read with the confirmed second factors,
    // best-effort: a lookup failure leaves them out.
    let methods: Vec<String> = match state.mfa_repo.find_methods(&principal.id).await {
        Ok(methods) => methods
            .into_iter()
            .filter(crate::mfa::entity::Method::is_confirmed)
            .map(|m| m.method.as_str().to_string())
            .collect(),
        Err(_) => Vec::new(),
    };
    let mut response = PrincipalResponse::from(principal);
    response.two_factor_methods = (!methods.is_empty()).then_some(methods);
    Ok(response)
}

/// `GET /api/principals`: filtered to what the caller reaches, sorted and
/// paged as Go does.
pub async fn list(
    state: &PrincipalsState,
    ctx: &AuthContext,
    query: &PrincipalsQuery,
) -> Result<PrincipalListResponse, PlatformError> {
    crate::checks::can_read_principals(ctx)?;

    // Validate client_id access upfront
    if let Some(ref client_id) = query.client_id {
        if !ctx.can_access_client(client_id) {
            return Err(PlatformError::forbidden(format!(
                "No access to client: {}",
                client_id
            )));
        }
    }

    // Apply all combinable filters at the DB level
    let principals = state
        .principal_repo
        .find_with_filters(
            query.client_id.as_deref(),
            parse_opt(query.scope.as_deref())?,
            parse_opt(query.principal_type.as_deref())?,
            query.active_filter(),
            query.q.as_deref(),
            query.email.as_deref(),
        )
        .await?;

    // Post-filter: access control + roles (requires hydrated data)
    let mut filtered: Vec<PrincipalResponse> = principals
        .into_iter()
        // Access control
        .filter(|p| {
            if ctx.is_anchor() {
                return true;
            }
            match &p.client_id {
                Some(cid) => ctx.can_access_client(cid),
                None => p.scope == UserScope::Anchor && ctx.is_anchor(),
            }
        })
        .map(|p| p.into())
        // Roles filter (requires checking hydrated roles, stays in-memory)
        .filter(|p: &PrincipalResponse| match &query.roles {
            Some(roles_str) if !roles_str.trim().is_empty() => {
                // Go splitCSV (api.go:239-247): trimmed, empties dropped.
                let required: Vec<&str> = roles_str
                    .split(',')
                    .map(str::trim)
                    .filter(|r| !r.is_empty())
                    .collect();
                required
                    .iter()
                    .any(|r| p.roles.iter().any(|role| role == r))
            }
            _ => true,
        })
        .collect();

    // Go sortPrincipals (api.go:251-269): a stable ascending sort, reversed
    // for "desc"; name and email compare case-insensitively.
    match query.sort_field.as_deref() {
        Some("name") => filtered.sort_by_key(|p| p.name.to_lowercase()),
        Some("email") => filtered.sort_by_key(|p| p.email.as_deref().unwrap_or("").to_lowercase()),
        _ => filtered.sort_by(|a, b| a.created_at.cmp(&b.created_at)),
    }
    if query
        .sort_order
        .as_deref()
        .is_some_and(|o| o.eq_ignore_ascii_case("desc"))
    {
        filtered.reverse();
    }

    let total = filtered.len();
    let principals: Vec<PrincipalResponse> = match query.paging()? {
        None => filtered,
        Some((page, size)) => filtered
            .into_iter()
            .skip(page.saturating_mul(size))
            .take(size)
            .collect(),
    };
    Ok(PrincipalListResponse { principals, total })
}

/// `PUT /api/principals/{id}`.
pub async fn update(
    state: &PrincipalsState,
    ctx: &AuthContext,
    id: &str,
    req: UpdatePrincipalRequest,
) -> Result<PrincipalResponse, PlatformError> {
    use crate::principal::operations::UpdateUserCommand;

    // The permission before anything is loaded, as Go's `update`
    // (principal/api/api.go:1026-1032): without it, nothing is touched.
    crate::checks::can_write_principals(ctx)?;

    // Handler-level auth: target-resource access + high-trust gates on
    // scope/client_id changes. Field-level mutations happen inside the
    // use case so the write commits atomically with the UserUpdated event.
    load_administered_user(state, ctx, id).await?;

    if (req.scope.is_some() || req.client_id.is_some()) && !ctx.is_anchor() {
        return Err(PlatformError::forbidden(
            "Only anchor users can change a principal's scope or client",
        ));
    }

    let cmd = UpdateUserCommand {
        principal_id: id.to_string(),
        name: req.name,
        first_name: req.first_name,
        last_name: req.last_name,
        active: req.active,
        scope: parse_opt(req.scope.as_deref())?,
        client_id: req.client_id,
    };
    let exec = ExecutionContext::create(&ctx.principal_id);
    state.update_use_case.run(cmd, exec).await.into_result()?;

    let refreshed = state
        .principal_repo
        .find_by_id(id)
        .await?
        .or_not_found("Principal", id)?;
    Ok(refreshed.into())
}

/// `GET /api/principals/{id}/roles`.
pub async fn role_assignments(
    state: &PrincipalsState,
    ctx: &AuthContext,
    id: &str,
) -> Result<RolesListResponse, PlatformError> {
    crate::checks::can_read_principals(ctx)?;

    let principal = state
        .principal_repo
        .find_by_id(id)
        .await?
        .or_not_found("Principal", id)?;

    // Go (principal/api/api.go, PR-4): a principal of a client the caller
    // does not reach answers the same 404 as a missing one.
    if !ctx.is_anchor() {
        if let Some(ref cid) = principal.client_id {
            if !ctx.can_access_client(cid) {
                return Err(PlatformError::not_found("Principal", id));
            }
        }
    }

    // Convert role assignments to DTOs
    let roles: Vec<RoleAssignmentDto> = principal
        .roles
        .iter()
        .enumerate()
        .map(|(i, r)| RoleAssignmentDto {
            id: format!("{}-role-{}", id, i),
            role_name: r.role.clone(),
            assignment_source: assignment_source_label(r),
            assigned_at: r.assigned_at.to_rfc3339(),
        })
        .collect();

    Ok(RolesListResponse { roles })
}

/// `POST /api/principals/{id}/roles`: add one role.
pub async fn assign_role(
    state: &PrincipalsState,
    ctx: &AuthContext,
    id: &str,
    role: String,
) -> Result<PrincipalResponse, PlatformError> {
    use crate::principal::operations::AssignUserRolesCommand;

    crate::checks::can_assign_principal_roles(ctx)?;

    // Additive assign: take existing roles + new role, run through UoW.
    let principal = load_role_administered_user(state, ctx, id).await?;
    if !ctx.is_anchor() {
        let allowed = client_application_ids(state, principal.client_id.as_deref()).await?;
        assert_assignable_roles(state, std::slice::from_ref(&role), &allowed).await?;
    }
    let before: Vec<String> = principal.roles.iter().map(|r| r.role.clone()).collect();
    let mut roles = before.clone();
    if !roles.iter().any(|r| r == &role) {
        roles.push(role.clone());
    }
    crate::role::ceiling::require_role_change(ctx, &state.role_repo, &before, &roles).await?;

    let cmd = AssignUserRolesCommand {
        user_id: id.to_string(),
        roles,
    };
    let exec = ExecutionContext::create(&ctx.principal_id);
    state
        .assign_roles_use_case
        .run(cmd, exec)
        .await
        .into_result()?;

    let refreshed = state
        .principal_repo
        .find_by_id(id)
        .await?
        .or_not_found("Principal", id)?;
    Ok(refreshed.into())
}

/// `PUT /api/principals/{id}/roles`: the declarative role set, bounded for
/// a client administrator and by the caller's role ceiling.
pub async fn set_roles(
    state: &PrincipalsState,
    ctx: &AuthContext,
    id: &str,
    roles: Vec<String>,
) -> Result<BatchAssignRolesResponse, PlatformError> {
    use crate::principal::operations::AssignUserRolesCommand;

    crate::checks::can_assign_principal_roles(ctx)?;

    let principal = load_role_administered_user(state, ctx, id).await?;
    let desired = bounded_role_set(state, ctx, &principal, roles).await?;

    let before: Vec<String> = principal.roles.iter().map(|r| r.role.clone()).collect();
    crate::role::ceiling::require_role_change(ctx, &state.role_repo, &before, &desired).await?;

    let old_roles: HashSet<String> = principal.roles.iter().map(|r| r.role.clone()).collect();
    let new_roles_set: HashSet<String> = desired.iter().cloned().collect();
    let added: Vec<String> = new_roles_set.difference(&old_roles).cloned().collect();
    let removed: Vec<String> = old_roles.difference(&new_roles_set).cloned().collect();

    let cmd = AssignUserRolesCommand {
        user_id: id.to_string(),
        roles: desired,
    };
    let exec = ExecutionContext::create(&ctx.principal_id);
    state
        .assign_roles_use_case
        .run(cmd, exec)
        .await
        .into_result()?;

    let refreshed = state
        .principal_repo
        .find_by_id(id)
        .await?
        .or_not_found("Principal", id)?;
    let roles: Vec<RoleAssignmentDto> = refreshed
        .roles
        .iter()
        .enumerate()
        .map(|(i, r)| RoleAssignmentDto {
            id: format!("{}-role-{}", id, i),
            role_name: r.role.clone(),
            assignment_source: assignment_source_label(r),
            assigned_at: r.assigned_at.to_rfc3339(),
        })
        .collect();

    Ok(BatchAssignRolesResponse {
        roles,
        added,
        removed,
    })
}

/// `DELETE /api/principals/{id}/roles/{role}`.
pub async fn remove_role(
    state: &PrincipalsState,
    ctx: &AuthContext,
    id: &str,
    role: &str,
) -> Result<PrincipalResponse, PlatformError> {
    use crate::principal::operations::AssignUserRolesCommand;

    crate::checks::can_assign_principal_roles(ctx)?;

    let principal = load_role_administered_user(state, ctx, id).await?;
    // A client administrator removes only roles it could assign (Go
    // `removeRole`).
    if !ctx.is_anchor() {
        let allowed = client_application_ids(state, principal.client_id.as_deref()).await?;
        assert_assignable_roles(state, &[role.to_string()], &allowed).await?;
    }
    let before: Vec<String> = principal.roles.iter().map(|r| r.role.clone()).collect();
    let roles: Vec<String> = principal
        .roles
        .iter()
        .filter(|r| r.role != role)
        .map(|r| r.role.clone())
        .collect();
    crate::role::ceiling::require_role_change(ctx, &state.role_repo, &before, &roles).await?;

    let cmd = AssignUserRolesCommand {
        user_id: id.to_string(),
        roles,
    };
    let exec = ExecutionContext::create(&ctx.principal_id);
    state
        .assign_roles_use_case
        .run(cmd, exec)
        .await
        .into_result()?;

    let refreshed = state
        .principal_repo
        .find_by_id(id)
        .await?
        .or_not_found("Principal", id)?;
    Ok(refreshed.into())
}

/// `GET /api/principals/{id}/client-access`: anchor only.
pub async fn client_grants(
    state: &PrincipalsState,
    ctx: &AuthContext,
    id: &str,
) -> Result<ClientAccessListResponse, PlatformError> {
    // Go `listClientAccess`: anchor reach alone (principal/api/api.go:1251).
    crate::checks::require_anchor_scope(ctx)?;
    let principal = state
        .principal_repo
        .find_by_id(id)
        .await?
        .or_not_found("Principal", id)?;

    // Convert assigned_clients to grants (synthesized since we don't store grant metadata)
    let grants: Vec<ClientAccessGrantResponse> = principal
        .assigned_clients
        .iter()
        .enumerate()
        .map(|(i, client_id)| ClientAccessGrantResponse {
            id: format!("{}-{}", id, i), // Synthetic ID
            client_id: client_id.clone(),
            granted_at: principal.created_at.to_rfc3339(), // Use principal creation as fallback
            expires_at: None,
        })
        .collect();

    Ok(ClientAccessListResponse { grants })
}

/// `POST /api/principals/{id}/client-access`.
pub async fn grant_client_access(
    state: &PrincipalsState,
    ctx: &AuthContext,
    id: &str,
    client_id: String,
) -> Result<ClientAccessGrantResponse, PlatformError> {
    use crate::principal::operations::GrantClientAccessCommand;

    crate::checks::can_grant_client_access(ctx)?;

    let granted_at = chrono::Utc::now();
    let cmd = GrantClientAccessCommand {
        user_id: id.to_string(),
        client_id: client_id.clone(),
    };
    let exec = ExecutionContext::create(&ctx.principal_id);
    state
        .grant_client_access_use_case
        .run(cmd, exec)
        .await
        .into_result()?;

    let refreshed = state
        .principal_repo
        .find_by_id(id)
        .await?
        .or_not_found("Principal", id)?;
    Ok(ClientAccessGrantResponse {
        id: format!(
            "{}-{}",
            id,
            refreshed.assigned_clients.len().saturating_sub(1)
        ),
        client_id,
        granted_at: granted_at.to_rfc3339(),
        expires_at: None,
    })
}

/// `DELETE /api/principals/{id}/client-access/{clientId}`.
pub async fn revoke_client_access(
    state: &PrincipalsState,
    ctx: &AuthContext,
    id: &str,
    client_id: &str,
) -> Result<(), PlatformError> {
    use crate::principal::operations::RevokeClientAccessCommand;

    crate::checks::can_revoke_client_access(ctx)?;

    let cmd = RevokeClientAccessCommand {
        user_id: id.to_string(),
        client_id: client_id.to_string(),
    };
    let exec = ExecutionContext::create(&ctx.principal_id);
    state
        .revoke_client_access_use_case
        .run(cmd, exec)
        .await
        .into_result()?;
    Ok(())
}

/// `DELETE /api/principals/{id}`.
pub async fn delete(
    state: &PrincipalsState,
    ctx: &AuthContext,
    id: &str,
) -> Result<(), PlatformError> {
    use crate::principal::operations::DeleteUserCommand;

    crate::checks::can_delete_principals(ctx)?;
    load_administered_user(state, ctx, id).await?;

    let cmd = DeleteUserCommand {
        principal_id: id.to_string(),
    };
    let exec = ExecutionContext::create(&ctx.principal_id);
    state.delete_use_case.run(cmd, exec).await.into_result()?;
    Ok(())
}

/// `POST /api/principals/{id}/activate`.
pub async fn activate(
    state: &PrincipalsState,
    ctx: &AuthContext,
    id: &str,
) -> Result<StatusChangeResponse, PlatformError> {
    use crate::principal::operations::ActivateUserCommand;

    crate::checks::can_write_principals(ctx)?;
    load_administered_user(state, ctx, id).await?;

    let cmd = ActivateUserCommand {
        principal_id: id.to_string(),
    };
    let exec = ExecutionContext::create(&ctx.principal_id);
    state.activate_use_case.run(cmd, exec).await.into_result()?;

    tracing::info!(principal_id = %id, admin_id = %ctx.principal_id, "Principal activated");

    Ok(StatusChangeResponse {
        message: "Principal activated".to_string(),
    })
}

/// `POST /api/principals/{id}/deactivate`.
pub async fn deactivate(
    state: &PrincipalsState,
    ctx: &AuthContext,
    id: &str,
) -> Result<StatusChangeResponse, PlatformError> {
    use crate::principal::operations::DeactivateUserCommand;

    crate::checks::can_write_principals(ctx)?;
    load_administered_user(state, ctx, id).await?;

    let cmd = DeactivateUserCommand {
        principal_id: id.to_string(),
        reason: Some("Admin deactivated principal".to_string()),
    };
    let exec = ExecutionContext::create(&ctx.principal_id);
    state
        .deactivate_use_case
        .run(cmd, exec)
        .await
        .into_result()?;

    tracing::info!(principal_id = %id, admin_id = %ctx.principal_id, "Principal deactivated");

    Ok(StatusChangeResponse {
        message: "Principal deactivated".to_string(),
    })
}

/// `POST /api/principals/{id}/reset-password`: an administrator sets the
/// password (internal users only; the use case refuses the rest).
pub async fn reset_password(
    state: &PrincipalsState,
    ctx: &AuthContext,
    id: &str,
    req: ResetPasswordRequest,
) -> Result<StatusChangeResponse, PlatformError> {
    use crate::principal::operations::ResetPasswordCommand;

    crate::checks::can_write_principals(ctx)?;
    load_administered_user(state, ctx, id).await?;

    let cmd = ResetPasswordCommand {
        principal_id: id.to_string(),
        new_password: req.new_password,
        enforce_password_complexity: req.enforce_password_complexity,
    };
    let exec = ExecutionContext::create(&ctx.principal_id);
    state
        .reset_password_use_case
        .run(cmd, exec)
        .await
        .into_result()?;

    tracing::info!(principal_id = %id, admin_id = %ctx.principal_id, "Password reset");

    Ok(StatusChangeResponse {
        message: "Password reset successfully".to_string(),
    })
}

/// `POST /api/principals/{id}/send-password-reset`: email the user a
/// single-use reset link; `reset_2fa` also clears their second factors
/// when they complete it.
pub async fn send_password_reset(
    state: &PrincipalsState,
    ctx: &AuthContext,
    id: &str,
    reset_2fa: bool,
) -> Result<StatusChangeResponse, PlatformError> {
    crate::checks::can_write_principals(ctx)?;

    let emailer = &state.password_reset_emailer;

    let principal = load_administered_user(state, ctx, id).await?;

    if !principal.is_user() {
        return Err(PlatformError::validation(
            "Password reset only applies to user accounts",
        ));
    }
    if principal.external_identity.is_some() {
        return Err(PlatformError::validation(
            "Cannot send password reset for OIDC-federated users — they manage credentials at their IDP",
        ));
    }
    if principal
        .user_identity
        .as_ref()
        .map(|i| i.email.is_empty())
        .unwrap_or(true)
    {
        return Err(PlatformError::validation(
            "User does not have an email address on file",
        ));
    }

    emailer
        .send_reset_email_with(
            &principal,
            crate::auth::password_reset_api::ResetOptions {
                reset_2fa,
                ..Default::default()
            },
        )
        .await?;

    tracing::info!(
        principal_id = %id,
        admin_id = %ctx.principal_id,
        "Admin triggered password reset email"
    );

    state
        .audit_service
        .log(ctx, "Principal", id, "Password reset email sent by admin")
        .await;

    Ok(StatusChangeResponse {
        message: "Password reset email sent".to_string(),
    })
}

/// `GET /api/principals/check-email-domain?email=`: how a user with this
/// email would be created (Go `checkEmailDomain`).
pub async fn check_email_domain(
    state: &PrincipalsState,
    ctx: &AuthContext,
    email: &str,
) -> Result<CheckEmailDomainResponse, PlatformError> {
    // Go `checkEmailDomain`: the user read permission, no anchor reach (a
    // client administrator's create form calls it).
    crate::checks::can_read_principals(ctx)?;

    // Go checkEmailDomain (principal/api/api.go).
    let email = email.trim().to_lowercase();
    if email.is_empty() {
        return Err(PlatformError::bad_request_code(
            "EMAIL_REQUIRED",
            "email query param is required",
        ));
    }
    let domain = match email.find('@') {
        Some(at) if at + 1 < email.len() => email[at + 1..].to_string(),
        _ => {
            return Err(PlatformError::bad_request_code(
                "INVALID_EMAIL",
                "Invalid email format",
            ))
        }
    };

    let email_exists = state.principal_repo.find_by_email(&email).await?.is_some();
    let is_anchor_domain = state.anchor_domain_repo.is_anchor_domain(&domain).await?;
    let mapping = state
        .email_domain_mapping_repo
        .find_by_email_domain(&domain)
        .await?;

    // The IdP type; an unmapped domain or a missing IdP is INTERNAL.
    let mut idp_type = "INTERNAL".to_string();
    let mut idp_issuer = None;
    if let Some(ref m) = mapping {
        if let Some(idp) = state
            .identity_provider_repo
            .find_by_id(&m.identity_provider_id)
            .await?
        {
            idp_type = idp.r#type.as_str().to_string();
            idp_issuer = idp.oidc_issuer_url.clone();
        }
    }
    let external = idp_type == "OIDC";

    use crate::email_domain_mapping::entity::ScopeType;
    let derived_scope = if is_anchor_domain {
        "ANCHOR"
    } else {
        match mapping.as_ref().map(|m| m.scope_type) {
            None => "CLIENT",
            Some(ScopeType::Anchor) => "ANCHOR",
            Some(ScopeType::Partner) => "PARTNER",
            Some(ScopeType::Client) => "CLIENT",
        }
    };
    // Go allowedClientIDsForDomain: PARTNER allows the primary and the
    // granted clients; CLIENT just the primary.
    let mut allowed_client_ids: Vec<String> = Vec::new();
    if let Some(ref m) = mapping {
        match m.scope_type {
            ScopeType::Partner => {
                for id in m
                    .primary_client_id
                    .iter()
                    .chain(m.granted_client_ids.iter())
                {
                    if !id.is_empty() && !allowed_client_ids.contains(id) {
                        allowed_client_ids.push(id.clone());
                    }
                }
            }
            ScopeType::Client => {
                if let Some(p) = m.primary_client_id.as_ref().filter(|p| !p.is_empty()) {
                    allowed_client_ids.push(p.clone());
                }
            }
            ScopeType::Anchor => {}
        }
    }

    Ok(CheckEmailDomainResponse {
        auth_method: if external { "external" } else { "internal" }.to_string(),
        login_url: external
            .then(|| format!("/auth/oidc/login?domain={}", urlencoding::encode(&domain))),
        idp_issuer: if external { idp_issuer } else { None },
        domain,
        auth_provider: idp_type,
        is_anchor_domain,
        has_idp_config: external,
        email_exists,
        info: None,
        warning: email_exists.then(|| "A user with this email address already exists.".to_string()),
        derived_scope: derived_scope.to_string(),
        requires_client_id: derived_scope != "ANCHOR",
        allowed_client_ids,
    })
}

/// `GET /api/principals/{id}/application-access`.
pub async fn application_access(
    state: &PrincipalsState,
    ctx: &AuthContext,
    id: &str,
) -> Result<ApplicationAccessListResponse, PlatformError> {
    crate::checks::can_read_principals(ctx)?;

    let principal = state
        .principal_repo
        .find_by_id(id)
        .await?
        .or_not_found("Principal", id)?;

    // Go (principal/api/api.go, PR-4): a principal of a client the caller
    // does not reach answers the same 404 as a missing one.
    if !ctx.is_anchor() {
        if let Some(ref cid) = principal.client_id {
            if !ctx.can_access_client(cid) {
                return Err(PlatformError::not_found("Principal", id));
            }
        }
    }

    let app_repo = &state.application_repo;

    // Resolve application details for each accessible application ID
    let mut applications = Vec::new();
    for app_id in &principal.accessible_application_ids {
        if let Some(app) = app_repo.find_by_id(app_id).await? {
            applications.push(ApplicationAccessResponse {
                application_id: app.id,
                application_code: app.code,
                application_name: app.name,
            });
        }
    }

    let total = applications.len();
    Ok(ApplicationAccessListResponse {
        applications,
        total,
        all_applications: principal.all_applications,
    })
}

/// `PUT /api/principals/{id}/application-access`: the declarative grant
/// set, and optionally the all-applications flag.
pub async fn set_application_access(
    state: &PrincipalsState,
    ctx: &AuthContext,
    id: &str,
    req: SetApplicationAccessRequest,
) -> Result<SetApplicationAccessResponse, PlatformError> {
    use crate::principal::operations::AssignApplicationAccessCommand;

    crate::checks::can_write_principals(ctx)?;

    let principal = load_role_administered_user(state, ctx, id).await?;

    if req.all_applications == Some(true) {
        // Granting every application exceeds what the caller may itself
        // reach unless it has every application too (Go's rule).
        if state.app_access.scope_for(&ctx.principal_id).await?
            != crate::shared::authorization_service::ApplicationScope::All
        {
            return Err(PlatformError::forbidden(
                "Only an all-applications administrator may grant all-applications access",
            ));
        }
    }

    // A client administrator grants only applications the target's client is
    // entitled to, and its SET keeps the grants outside that reach (Go
    // `assignApplicationAccess`, principal/api/api.go:1183-1249).
    let mut req = req;
    if !ctx.is_anchor() {
        let allowed = client_application_ids(state, principal.client_id.as_deref()).await?;
        if let Some(app_id) = req.application_ids.iter().find(|a| !allowed.contains(*a)) {
            return Err(PlatformError::forbidden_code(
                "APP_FORBIDDEN",
                format!("application the client cannot access: {app_id}"),
            ));
        }
        for kept in principal
            .accessible_application_ids
            .iter()
            .filter(|a| !allowed.contains(*a))
        {
            if !req.application_ids.contains(kept) {
                req.application_ids.push(kept.clone());
            }
        }
    }

    let app_repo = &state.application_repo;

    // Validate applications exist and are active (kept in handler for 400 mapping).
    for app_id in &req.application_ids {
        match app_repo.find_by_id(app_id).await? {
            Some(app) => {
                if !app.active {
                    return Err(PlatformError::validation(format!(
                        "Application is not active: {}",
                        app_id
                    )));
                }
            }
            None => {
                return Err(PlatformError::validation(format!(
                    "Application not found: {}",
                    app_id
                )));
            }
        }
    }

    let old_set: HashSet<&str> = principal
        .accessible_application_ids
        .iter()
        .map(|s| s.as_str())
        .collect();
    let new_set: HashSet<&str> = req.application_ids.iter().map(|s| s.as_str()).collect();
    let added_count = new_set.difference(&old_set).count();
    let removed_count = old_set.difference(&new_set).count();

    let cmd = AssignApplicationAccessCommand {
        user_id: id.to_string(),
        application_ids: req.application_ids.clone(),
        all_applications: req.all_applications,
    };
    let exec = ExecutionContext::create(&ctx.principal_id);
    state
        .assign_app_access_use_case
        .run(cmd, exec)
        .await
        .into_result()?;
    state.app_access.forget(id);

    let mut applications = Vec::new();
    for app_id in &req.application_ids {
        if let Some(app) = app_repo.find_by_id(app_id).await? {
            applications.push(ApplicationAccessResponse {
                application_id: app.id,
                application_code: app.code,
                application_name: app.name,
            });
        }
    }

    Ok(SetApplicationAccessResponse {
        applications,
        added: added_count,
        removed: removed_count,
        all_applications: req.all_applications.unwrap_or(principal.all_applications),
    })
}

/// `GET /api/principals/{id}/available-applications`: every active
/// application for an anchor, else those the target's client has enabled.
pub async fn available_applications(
    state: &PrincipalsState,
    ctx: &AuthContext,
    id: &str,
) -> Result<AvailableApplicationsResponse, PlatformError> {
    crate::checks::can_read_principals(ctx)?;

    let principal = state
        .principal_repo
        .find_by_id(id)
        .await?
        .or_not_found("Principal", id)?;

    // Go listAvailableApplications (principal/api/api.go): a principal of a
    // client the caller does not reach answers the same 404 as a missing
    // one.
    if let Some(ref cid) = principal.client_id {
        if !crate::shared::caller_reach::reaches_client(ctx, cid) {
            return Err(PlatformError::not_found("Principal", id));
        }
    }

    // Every active application, ordered by code; a non-anchor caller's menu
    // is bounded to the applications the target's client has enabled.
    let mut apps = state.application_repo.find_active().await?;
    if !ctx.is_anchor() {
        let allowed: HashSet<String> = match principal.client_id.as_deref() {
            Some(cid) => state
                .app_client_config_repo
                .find_by_client(cid)
                .await?
                .into_iter()
                .filter(|c| c.enabled)
                .map(|c| c.application_id)
                .collect(),
            None => Default::default(),
        };
        apps.retain(|a| allowed.contains(&a.id));
    }
    apps.sort_by(|a, b| a.code.cmp(&b.code));

    Ok(AvailableApplicationsResponse {
        applications: apps.into_iter().map(Into::into).collect(),
    })
}
