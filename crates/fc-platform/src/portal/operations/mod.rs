//! Portal identity plane use cases. Every write goes through a use case and
//! the unit of work (event + audit row), as Go's `usecaseop` operations do.

pub mod apps;
pub mod events;
pub mod identity;

pub use apps::{
    AssignUnassignedCommand, AssignUnassignedPortalIdentitiesUseCase,
    CreateAppWithOAuthClientCommand, CreatePortalAppWithOAuthClientUseCase, DeleteAppCommand,
    DeletePortalAppUseCase, UpdateAppCommand, UpdatePortalAppUseCase, NOTHING_TO_ASSIGN,
};
pub use identity::{
    AppGrantCommand, DeleteCommand, DeletePortalIdentityUseCase, EnsureCommand,
    EnsurePortalIdentityUseCase, GrantPortalIdentityAppUseCase, RevokePortalIdentityAppUseCase,
    SetPortalIdentityStatusUseCase, SetStatusCommand,
};

use crate::portal::entity::PortalApp;
use crate::portal::repository::PortalAppRepository;
use crate::usecase::UseCaseError;

/// Go's `httperror.NotFound(resource, id)`: 404 `<RESOURCE>_NOT_FOUND` (in
/// UPPER_SNAKE, owner decision 5) and `<Resource> not found: <id>`.
pub fn not_found(resource: &str, id: &str) -> UseCaseError {
    UseCaseError::not_found(
        crate::shared::error::not_found_code(resource),
        format!("{resource} not found: {id}"),
    )
}

/// An app for a grant: it must exist, belong to the client, and be active
/// (Go `loadClientApp`).
pub async fn load_client_app(
    apps: &PortalAppRepository,
    client_id: &str,
    app_id: &str,
) -> Result<PortalApp, UseCaseError> {
    let app = apps
        .find_by_id(app_id)
        .await?
        .filter(|a| a.client_id == client_id)
        .ok_or_else(|| not_found("PortalApp", app_id))?;
    if !app.active {
        return Err(UseCaseError::validation(
            "PORTAL_APP_INACTIVE",
            format!("portal app '{}' is inactive", app.code),
        ));
    }
    Ok(app)
}

/// The app, scoped to `client_id` when one is given (Go `findClientApp`).
pub async fn find_client_app(
    apps: &PortalAppRepository,
    client_id: &str,
    id: &str,
) -> Result<PortalApp, UseCaseError> {
    apps.find_by_id(id)
        .await?
        .filter(|a| client_id.is_empty() || a.client_id == client_id)
        .ok_or_else(|| not_found("PortalApp", id))
}
