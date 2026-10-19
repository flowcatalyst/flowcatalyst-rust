//! Invite Application User Use Case (Go F20 `InviteApplicationUser`,
//! principal/operations/invite_application_user.go).
//!
//! Creates one CLIENT-scope user on behalf of an application, holding that
//! application's roles, in one transaction, and emits `UserCreated`. The
//! invitation itself (the email or the link) is the caller's, after commit,
//! through the same invite mint the other user-creation routes use.
//!
//! The roles are what makes the user the application's: an application's users
//! are the CLIENT users homed at a client the application is enabled for that
//! hold one of its roles (`appusers::repository`). So at least one role is
//! required, every role must belong to the application (403
//! `ROLE_APP_FORBIDDEN`, the sync's code), and a name that is not a role is
//! refused (400 `UNKNOWN_ROLE`; never silently dropped, unlike the sync). The
//! role ceiling (`role::ceiling`, F8) bounds the grant as it does for every
//! role change.
//!
//! `authorize` repeats what the route has already enforced (Go leaves it
//! public and trusts the route): the permission, the application binding and
//! the caller's reach over the client, so the use case cannot be reached around
//! them.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use super::events::UserCreated;
use crate::application::repository::ApplicationRepository;
use crate::identity_provider::entity::IdentityProviderType;
use crate::principal::entity::Principal;
use crate::principal::repository::PrincipalRepository;
use crate::role::ceiling;
use crate::role::entity::AuthRole;
use crate::role::repository::RoleRepository;
use crate::service_account::entity::{AssignmentSource, RoleAssignment};
use fc_platform_core::principal_kind::UserScope;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::shared::caller_reach;
use fc_platform_core::shared::error::PlatformError;
use fc_platform_core::shared::id::ClientId;
use fc_platform_core::usecase::{
    parse_client_id, AuditMasked, Committed, ExecutionContext, UnitOfWork, UseCase, UseCaseError,
};

/// Go's `emailPattern` for a created user.
#[expect(
    clippy::expect_used,
    reason = "a regex literal that is valid by inspection"
)]
fn email_pattern() -> &'static regex::Regex {
    static PATTERN: OnceLock<regex::Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        regex::Regex::new(r"^[a-zA-Z0-9._%+\-]+@[a-zA-Z0-9.\-]+\.[a-zA-Z]{2,}$")
            .expect("static email pattern")
    })
}

/// The one answer for a client the application may not invite into: unknown,
/// not enabled for the application, or out of the caller's reach.
pub const CLIENT_REFUSED: &str = "client is not available to this application";

/// Command for inviting a user on behalf of an application.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteApplicationUserCommand {
    pub application_code: String,
    pub email: String,
    pub name: String,
    /// The resolved `clt_` id of the user's home client. The route has
    /// already checked that the application is enabled for it and that the
    /// caller reaches it.
    pub client_id: String,
    /// The application's roles to give the user: canonical (`app:role`) or
    /// short (`role`, prefixed with the application code).
    pub roles: Vec<String>,
    /// `Oidc` marks a user whose email domain is federated: created without a
    /// password and never sent a set-password invitation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idp_type: Option<IdentityProviderType>,
}

impl AuditMasked for InviteApplicationUserCommand {}

pub struct InviteApplicationUserUseCase<U: UnitOfWork> {
    principal_repo: Arc<PrincipalRepository>,
    role_repo: Arc<RoleRepository>,
    application_repo: Arc<ApplicationRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> InviteApplicationUserUseCase<U> {
    pub fn new(
        principal_repo: Arc<PrincipalRepository>,
        role_repo: Arc<RoleRepository>,
        application_repo: Arc<ApplicationRepository>,
        unit_of_work: Arc<U>,
    ) -> Self {
        Self {
            principal_repo,
            role_repo,
            application_repo,
            unit_of_work,
        }
    }
}

/// The names a command asks for: lower-cased, trimmed, a short name prefixed
/// with the application code. Without repeats, in request order.
pub(crate) fn requested_role_names(application_code: &str, roles: &[String]) -> Vec<String> {
    let prefix = application_code.to_lowercase();
    let mut out: Vec<String> = Vec::with_capacity(roles.len());
    for r in roles {
        let mut n = r.trim().to_lowercase();
        if !n.is_empty() && !n.contains(':') {
            n = format!("{prefix}:{n}");
        }
        if !out.contains(&n) {
            out.push(n);
        }
    }
    out
}

#[async_trait]
impl<U: UnitOfWork> UseCase for InviteApplicationUserUseCase<U> {
    type Command = InviteApplicationUserCommand;
    type Event = UserCreated;

    async fn validate(&self, command: &InviteApplicationUserCommand) -> Result<(), UseCaseError> {
        let email = command.email.trim().to_lowercase();
        if email.is_empty() {
            return Err(UseCaseError::validation(
                "EMAIL_REQUIRED",
                "email is required",
            ));
        }
        if !email_pattern().is_match(&email) {
            return Err(UseCaseError::validation(
                "INVALID_EMAIL",
                "email must be a valid address",
            ));
        }
        if command.client_id.trim().is_empty() {
            return Err(UseCaseError::validation(
                "CLIENT_REQUIRED",
                "clientId is required",
            ));
        }
        if command.application_code.trim().is_empty() {
            return Err(UseCaseError::validation(
                "APPLICATION_REQUIRED",
                "application is required",
            ));
        }
        if command.roles.is_empty() {
            return Err(UseCaseError::validation(
                "ROLES_REQUIRED",
                "at least one role of the application is required",
            ));
        }
        Ok(())
    }

    /// The permission to invite, the application in the caller's scope (404
    /// as a missing application, the SDK syncs' rule) and the caller's reach
    /// over the home client (one 403 answer, whatever the reason).
    async fn authorize(
        &self,
        command: &InviteApplicationUserCommand,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        checks::can_create_application_users(ctx.caller()).map_err(UseCaseError::verbatim)?;
        let application = self
            .application_repo
            .find_by_code(&command.application_code)
            .await?;
        checks::require_caller_application_access(
            ctx.caller(),
            &command.application_code,
            application,
        )
        .map_err(UseCaseError::verbatim)?;
        if !caller_reach::reaches_client(
            ctx.caller(),
            &ClientId::from_wire(command.client_id.trim()),
        ) {
            return Err(UseCaseError::verbatim(PlatformError::forbidden(
                CLIENT_REFUSED,
            )));
        }
        Ok(())
    }

    async fn execute(
        &self,
        command: InviteApplicationUserCommand,
        ctx: ExecutionContext,
    ) -> Result<Committed<UserCreated>, UseCaseError> {
        let email = command.email.trim().to_lowercase();
        if self.principal_repo.find_by_email(&email).await?.is_some() {
            return Err(UseCaseError::business_rule(
                "EMAIL_EXISTS",
                format!("User with email '{email}' already exists"),
            ));
        }

        // The roles: all must be the application's; a name that is not a role
        // is refused after the foreign-role check (Go `applicationRoles`
        // refuses a foreign role while walking the names, and reports
        // unknown ones afterwards).
        let names = requested_role_names(&command.application_code, &command.roles);
        let found: HashMap<String, AuthRole> = self
            .role_repo
            .find_by_codes(&names)
            .await
            .map_err(|e| UseCaseError::commit(format!("Failed to load roles: {e}")))?
            .into_iter()
            .map(|r| (r.name.clone(), r))
            .collect();
        let owner = command.application_code.to_lowercase();
        let mut assigned: Vec<String> = Vec::with_capacity(names.len());
        let mut unknown: Vec<&String> = Vec::new();
        for name in &names {
            match found.get(name) {
                None => unknown.push(name),
                Some(role) if role.owning_application_code().to_lowercase() != owner => {
                    return Err(UseCaseError::forbidden(
                        "ROLE_APP_FORBIDDEN",
                        format!(
                            "role '{name}' does not belong to application '{}'",
                            command.application_code
                        ),
                    ));
                }
                Some(role) => assigned.push(role.name.clone()),
            }
        }
        if let Some(first) = unknown.first() {
            return Err(UseCaseError::validation(
                "UNKNOWN_ROLE",
                format!("role '{first}' does not exist"),
            ));
        }
        ceiling::require_roles(Some(ctx.caller()), &assigned, &found)?;

        let mut principal = Principal::new_user(&email, UserScope::Client)
            .with_client_id(parse_client_id(&command.client_id)?);
        let name = command.name.trim();
        if !name.is_empty() {
            principal.name = name.to_string();
        }
        if command.idp_type == Some(IdentityProviderType::Oidc) {
            if let Some(identity) = principal.user_identity.as_mut() {
                identity.password_hash = None;
                identity.provider = Some(IdentityProviderType::Oidc.as_str().to_string());
            }
        }
        // `ADMIN_ASSIGNED`: an SDK sync sweep strips only `SDK_SYNC` roles, so
        // an invitation's roles survive one.
        principal.roles = assigned
            .into_iter()
            .map(|r| RoleAssignment::with_source(r, AssignmentSource::AdminAssigned))
            .collect();

        let event = UserCreated::new(&ctx, &principal.id, &email);
        self.unit_of_work
            .commit(&principal, &*self.principal_repo, event, &command)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_names_take_the_application_prefix_and_repeats_collapse() {
        let names = requested_role_names(
            "Orders",
            &[
                "Clerk".to_string(),
                " orders:clerk ".to_string(),
                "billing:admin".to_string(),
                "".to_string(),
            ],
        );
        assert_eq!(names, vec!["orders:clerk", "billing:admin", ""]);
    }
}
