//! Principal routes: `/api/principals`, and Go's bulk import, version and
//! client-association routes at their full paths (`principal_go_router`).
//! The two-factor reset and the developer credentials nest under the same
//! prefix from their own modules (`mfa`, `developer_credential`).

use std::sync::Arc;

use axum::Router;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::api::{PrincipalGoState, PrincipalsState};
use super::operations::set_client_association::SetClientAssociationUseCase;
use super::operations::{
    ActivateUserUseCase, AssignApplicationAccessUseCase, AssignUserRolesUseCase, CreateUserUseCase,
    DeactivateUserUseCase, DeleteUserUseCase, GrantClientAccessUseCase, ResetPasswordUseCase,
    RevokeClientAccessUseCase, UpdateUserUseCase,
};
use crate::audit::service::AuditService;
use crate::shared::platform_context::{AggregateRoutes, PlatformContext};

pub fn routes(ctx: &PlatformContext) -> AggregateRoutes {
    AggregateRoutes {
        documented: OpenApiRouter::new()
            .nest("/api/principals", principals_router(principals_state(ctx)))
            .merge(principal_go_router(principal_go_state(ctx))),
        plain: Router::new(),
    }
}

pub fn principals_state(ctx: &PlatformContext) -> PrincipalsState {
    let repos = &ctx.repos;
    let uow = &ctx.unit_of_work;
    PrincipalsState {
        mfa_repo: Arc::new(crate::mfa::MfaRepository::new(&repos.pool)),
        principal_repo: repos.principal_repo.clone(),
        role_repo: repos.role_repo.clone(),
        client_repo: repos.client_repo.clone(),
        audit_service: Arc::new(AuditService::new(repos.audit_log_repo.clone())),
        anchor_domain_repo: repos.anchor_domain_repo.clone(),
        email_domain_mapping_repo: repos.edm_repo.clone(),
        identity_provider_repo: repos.idp_repo.clone(),
        application_repo: repos.application_repo.clone(),
        app_client_config_repo: repos.application_client_config_repo.clone(),
        client_access_grant_repo: repos.client_access_grant_repo.clone(),
        password_reset_emailer: ctx.password_reset_emailer.clone(),
        new_user_notifier: Some(crate::mfa::notify::Notifier {
            email: ctx.email_service.clone(),
            name: crate::mfa::notify::PlatformName {
                configs: Some(repos.platform_config_repo.clone()),
            },
        }),
        create_user_use_case: Arc::new(CreateUserUseCase::new(
            repos.principal_repo.clone(),
            ctx.auth.password.clone(),
            uow.clone(),
        )),
        grant_client_access_use_case: Arc::new(GrantClientAccessUseCase::new(
            repos.principal_repo.clone(),
            repos.client_repo.clone(),
            repos.client_access_grant_repo.clone(),
            uow.clone(),
        )),
        reset_password_use_case: Arc::new(ResetPasswordUseCase::new(
            repos.principal_repo.clone(),
            ctx.auth.password.clone(),
            uow.clone(),
        )),
        activate_use_case: Arc::new(ActivateUserUseCase::new(
            repos.principal_repo.clone(),
            uow.clone(),
        )),
        deactivate_use_case: Arc::new(DeactivateUserUseCase::new(
            repos.principal_repo.clone(),
            uow.clone(),
        )),
        delete_use_case: Arc::new(DeleteUserUseCase::new(
            repos.principal_repo.clone(),
            uow.clone(),
        )),
        update_use_case: Arc::new(UpdateUserUseCase::new(
            repos.principal_repo.clone(),
            uow.clone(),
        )),
        assign_roles_use_case: Arc::new(AssignUserRolesUseCase::new(
            repos.principal_repo.clone(),
            repos.role_repo.clone(),
            uow.clone(),
        )),
        revoke_client_access_use_case: Arc::new(RevokeClientAccessUseCase::new(
            repos.principal_repo.clone(),
            repos.client_access_grant_repo.clone(),
            uow.clone(),
        )),
        assign_app_access_use_case: Arc::new(AssignApplicationAccessUseCase::new(
            repos.principal_repo.clone(),
            repos.application_repo.clone(),
            uow.clone(),
        )),
        app_access: ctx.app_access.clone(),
        unit_of_work: uow.clone(),
    }
}

pub fn principal_go_state(ctx: &PlatformContext) -> PrincipalGoState {
    let repos = &ctx.repos;
    let uow = &ctx.unit_of_work;
    PrincipalGoState {
        principal_repo: repos.principal_repo.clone(),
        role_repo: repos.role_repo.clone(),
        edm_repo: repos.edm_repo.clone(),
        idp_repo: repos.idp_repo.clone(),
        client_config_repo: repos.application_client_config_repo.clone(),
        emailer: ctx.password_reset_emailer.clone(),
        create_user_use_case: Arc::new(CreateUserUseCase::new(
            repos.principal_repo.clone(),
            ctx.auth.password.clone(),
            uow.clone(),
        )),
        assign_roles_use_case: Arc::new(AssignUserRolesUseCase::new(
            repos.principal_repo.clone(),
            repos.role_repo.clone(),
            uow.clone(),
        )),
        set_client_association_use_case: Arc::new(SetClientAssociationUseCase::new(
            repos.principal_repo.clone(),
            repos.client_repo.clone(),
            uow.clone(),
        )),
    }
}

/// Create principals router
pub fn principals_router(state: PrincipalsState) -> OpenApiRouter {
    OpenApiRouter::new()
        // `routes!(...)` groups handlers on the SAME path; `create_user` is
        // `/users` and `list_principals` is `""`, so they must be registered
        // separately or only one gets mounted (previously the cause of 405s).
        .routes(routes!(
            crate::principal::api::list_principals,
            crate::principal::api::create_principal
        ))
        .routes(routes!(crate::principal::api::create_user))
        .routes(routes!(crate::principal::api::sync_users))
        .routes(routes!(crate::principal::api::check_email_domain))
        .routes(routes!(
            crate::principal::api::get_principal,
            crate::principal::api::update_principal,
            crate::principal::api::delete_principal
        ))
        .routes(routes!(crate::principal::api::activate_principal))
        .routes(routes!(crate::principal::api::deactivate_principal))
        .routes(routes!(crate::principal::api::reset_password))
        .routes(routes!(crate::principal::api::send_password_reset))
        .routes(routes!(
            crate::principal::api::get_roles,
            crate::principal::api::assign_role,
            crate::principal::api::batch_assign_roles
        ))
        .routes(routes!(crate::principal::api::remove_role))
        .routes(routes!(
            crate::principal::api::get_client_access,
            crate::principal::api::grant_client_access
        ))
        .routes(routes!(crate::principal::api::revoke_client_access))
        .routes(routes!(
            crate::principal::api::get_application_access,
            crate::principal::api::set_application_access
        ))
        .routes(routes!(crate::principal::api::get_available_applications))
        .with_state(state)
}

/// Full-path router; merged at the root.
pub fn principal_go_router(state: PrincipalGoState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(crate::principal::api::bulk_import_principals))
        .routes(routes!(crate::principal::api::get_principal_version))
        .routes(routes!(
            crate::principal::api::set_principal_client_association
        ))
        .with_state(state)
}
