//! Shared builder for `PlatformRoutes`: the [`PlatformContext`] every
//! route module builds its state from.

use std::sync::Arc;

use crate::repository::Repositories;
use crate::router::PlatformRoutes;
use crate::usecase::PgUnitOfWork;

use super::AuthServices;

use crate::shared::platform_context::PlatformContext;
pub use crate::shared::platform_context::PlatformRoutesConfig;

/// Build `PlatformRoutes` for the server binaries. Binaries call `.build()`
/// and add their own middleware/static layers.
pub fn build_platform_routes(
    repos: &Repositories,
    auth: &AuthServices,
    unit_of_work: &Arc<PgUnitOfWork>,
    config: PlatformRoutesConfig,
    platform_application_id: String,
) -> PlatformRoutes {
    PlatformRoutes {
        ctx: PlatformContext::new(repos, auth, unit_of_work, config, platform_application_id),
    }
}
