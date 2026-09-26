//! The `aud_logs.operation` a command is recorded under.
//!
//! Go records the bare name of its command struct (shared/platformsink/
//! sink.go `WriteAudit`, by reflection), and its commands are named per
//! package (`application.CreateCommand` is `CreateCommand`). Rust's command
//! types carry their aggregate in the name (`CreateApplicationCommand`), so
//! each is recorded under Go's name instead: the audit-log operation facet
//! and filter then match Go, and rows written before and after the cutover
//! read alike. A Rust command Go has no counterpart for (the function
//! runner's, for one) keeps its own name.
//!
//! Where Go records one Rust command under different names by route (the
//! provision-service-account route records `ProvisionServiceAccountCommand`;
//! connection pause and activate record `statusCommand`), the plain route's
//! name is used.

/// The operation name Go records for the Rust command type `C`.
pub fn audit_operation_name<C: ?Sized>() -> &'static str {
    let type_name = std::any::type_name::<C>();
    let short = type_name
        .split('<')
        .next()
        .unwrap_or(type_name)
        .rsplit("::")
        .next()
        .unwrap_or("Unknown");
    go_operation_name(short)
}

/// Go's name for a Rust command's short type name; a name Go shares, or
/// has no counterpart for, comes back unchanged.
pub fn go_operation_name(rust_name: &str) -> &str {
    match rust_name {
        // application/operations/activate.go:23
        "ActivateApplicationCommand" => "ActivateCommand",
        // application/operations/attach_service_account.go:29
        "AttachServiceAccountToApplicationCommand" => "AttachServiceAccountCommand",
        // application/operations/create.go:32
        "CreateApplicationCommand" => "CreateCommand",
        // application/operations/deactivate.go:23
        "DeactivateApplicationCommand" => "DeactivateCommand",
        // application/operations/delete.go:23
        "DeleteApplicationCommand" => "DeleteCommand",
        // application/operations/disable_for_client.go:29
        "DisableApplicationForClientCommand" => "DisableForClientCommand",
        // application/operations/enable_for_client.go:29
        "EnableApplicationForClientCommand" => "EnableForClientCommand",
        // application/operations/update.go:30
        "UpdateApplicationCommand" => "UpdateCommand",
        // principal/operations/oidc_login.go:30
        "OidcLoginCommand" => "OidcLogin",
        // client/operations/activate.go:19
        "ActivateClientCommand" => "ActivateCommand",
        // client/operations/add_note.go:21
        "AddClientNoteCommand" => "AddNoteCommand",
        // client/operations/create.go:26
        "CreateClientCommand" => "CreateCommand",
        // client/operations/delete.go:22
        "DeleteClientCommand" => "DeleteCommand",
        // client/operations/suspend.go:20
        "SuspendClientCommand" => "SuspendCommand",
        // client/operations/update.go:20
        "UpdateClientCommand" => "UpdateCommand",
        // connection/operations/create.go:37
        "CreateConnectionCommand" => "CreateCommand",
        // connection/operations/delete.go:20
        "DeleteConnectionCommand" => "DeleteCommand",
        // connection/operations/update.go:29
        "UpdateConnectionCommand" => "UpdateCommand",
        // cors/operations/add_origin.go:26
        "AddCorsOriginCommand" => "AddCommand",
        // cors/operations/delete_origin.go:18
        "DeleteCorsOriginCommand" => "DeleteCommand",
        // dispatchpool/operations/archive.go:21
        "ArchiveDispatchPoolCommand" => "ArchiveCommand",
        // dispatchpool/operations/create.go:27
        "CreateDispatchPoolCommand" => "CreateCommand",
        // dispatchpool/operations/delete.go:21
        "DeleteDispatchPoolCommand" => "DeleteCommand",
        // dispatchpool/operations/suspend.go:22
        "SuspendDispatchPoolCommand" => "SuspendCommand",
        // dispatchpool/operations/suspend.go:65
        "ActivateDispatchPoolCommand" => "ActivateCommand",
        // dispatchpool/operations/update.go:25
        "UpdateDispatchPoolCommand" => "UpdateCommand",
        // emaildomainmapping/operations/create.go:48
        "CreateEmailDomainMappingCommand" => "CreateCommand",
        // emaildomainmapping/operations/delete.go:22
        "DeleteEmailDomainMappingCommand" => "DeleteCommand",
        // emaildomainmapping/operations/move_provider.go:60
        "MoveMappingToProviderCommand" => "MoveProviderCommand",
        // emaildomainmapping/operations/update.go:36
        "UpdateEmailDomainMappingCommand" => "UpdateCommand",
        // eventtype/operations/archive.go:22
        "ArchiveEventTypeCommand" => "ArchiveCommand",
        // eventtype/operations/create.go:29
        "CreateEventTypeCommand" => "CreateCommand",
        // eventtype/operations/delete.go:21
        "DeleteEventTypeCommand" => "DeleteCommand",
        // eventtype/operations/update.go:25
        "UpdateEventTypeCommand" => "UpdateCommand",
        // identityprovider/operations/create.go:240
        "CreateIdentityProviderCommand" => "CreateCommand",
        // identityprovider/operations/delete.go:23
        "DeleteIdentityProviderCommand" => "DeleteCommand",
        // identityprovider/operations/update.go:56
        "UpdateIdentityProviderCommand" => "UpdateCommand",
        // platformconfig/operations/grant_access.go:22
        "GrantPlatformConfigAccessCommand" => "GrantAccessCommand",
        // platformconfig/operations/revoke_access.go:20
        "RevokePlatformConfigAccessCommand" => "RevokeAccessCommand",
        // platformconfig/operations/set_property.go:42
        "SetPlatformConfigPropertyCommand" => "SetPropertyCommand",
        // principal/operations/activate.go:24
        "ActivateUserCommand" => "ActivateCommand",
        // principal/operations/assign_roles.go:29
        "AssignUserRolesCommand" => "AssignRolesCommand",
        // principal/operations/create.go:36
        "CreateUserCommand" => "CreateCommand",
        // principal/operations/deactivate.go:23
        "DeactivateUserCommand" => "DeactivateCommand",
        // principal/operations/delete.go:23
        "DeleteUserCommand" => "DeleteCommand",
        // principal/operations/sync_principals.go:68
        "SyncUsersCommand" => "SyncPrincipalsCommand",
        // principal/operations/update.go:26
        "UpdateUserCommand" => "UpdateCommand",
        // process/operations/archive.go:19
        "ArchiveProcessCommand" => "ArchiveCommand",
        // process/operations/create.go:26
        "CreateProcessCommand" => "CreateCommand",
        // process/operations/delete.go:19
        "DeleteProcessCommand" => "DeleteCommand",
        // process/operations/update.go:24
        "UpdateProcessCommand" => "UpdateCommand",
        // role/operations/create.go:23
        "CreateRoleCommand" => "CreateCommand",
        // role/operations/delete.go:20
        "DeleteRoleCommand" => "DeleteCommand",
        // role/operations/update.go:25
        "UpdateRoleCommand" => "UpdateCommand",
        // scheduledjob/operations/ops.go:298
        "ArchiveScheduledJobCommand" => "transitionCommand",
        // scheduledjob/operations/ops.go:47
        "CreateScheduledJobCommand" => "CreateCommand",
        // scheduledjob/operations/ops.go:317
        "DeleteScheduledJobCommand" => "DeleteCommand",
        // scheduledjob/operations/ops.go:367
        "FireScheduledJobCommand" => "FireNowCommand",
        // scheduledjob/operations/ops.go:274
        "PauseScheduledJobCommand" => "transitionCommand",
        // scheduledjob/operations/ops.go:286
        "ResumeScheduledJobCommand" => "transitionCommand",
        // scheduledjob/operations/ops.go:144
        "UpdateScheduledJobCommand" => "UpdateCommand",
        // serviceaccount/operations/create_credentials.go:55
        "CreateServiceAccountCommand" => "CreateCommand",
        // serviceaccount/operations/deactivate.go:20
        "DeactivateServiceAccountCommand" => "DeactivateCommand",
        // serviceaccount/operations/delete.go:19
        "DeleteServiceAccountCommand" => "DeleteCommand",
        // serviceaccount/api/api.go:373
        "MintServiceAccountTokenCommand" => "TOKEN_MINTED_BY_ADMIN",
        // serviceaccount/operations/update.go:34
        "UpdateServiceAccountCommand" => "UpdateCommand",
        // subscription/operations/create.go:46
        "CreateSubscriptionCommand" => "CreateCommand",
        // subscription/operations/delete.go:20
        "DeleteSubscriptionCommand" => "DeleteCommand",
        // subscription/operations/pause.go:21
        "PauseSubscriptionCommand" => "PauseCommand",
        // subscription/operations/resume.go:20
        "ResumeSubscriptionCommand" => "ResumeCommand",
        // subscription/operations/update.go:40
        "UpdateSubscriptionCommand" => "UpdateCommand",
        // webauthn/operations/authenticate.go:26
        "AuthenticatePasskeyCommand" => "AuthenticateCommand",
        // webauthn/operations/register.go:34
        "RegisterPasskeyCommand" => "RegisterCommand",
        // webauthn/operations/revoke.go:19
        "RevokePasskeyCommand" => "RevokeCommand",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_commands_are_recorded_under_gos_names() {
        assert_eq!(
            go_operation_name("CreateApplicationCommand"),
            "CreateCommand"
        );
        assert_eq!(
            go_operation_name("AssignUserRolesCommand"),
            "AssignRolesCommand"
        );
        assert_eq!(
            go_operation_name("SetPlatformConfigPropertyCommand"),
            "SetPropertyCommand"
        );
        assert_eq!(go_operation_name("SyncRolesCommand"), "SyncRolesCommand");
        assert_eq!(go_operation_name("PublishCommand"), "PublishCommand");
    }

    #[test]
    fn the_name_comes_from_the_type() {
        assert_eq!(
            audit_operation_name::<crate::client::operations::CreateClientCommand>(),
            "CreateCommand"
        );
    }
}
