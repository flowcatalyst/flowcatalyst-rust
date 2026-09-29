//! Function routes: the management API (`/api/functions*`,
//! `/api/function-{pools,policies,domains,routes}`, full paths), the host
//! control plane (`/control/functions/*`), and the two unauthenticated
//! documents (the manifest's JSON Schema and Java's function API contract).

#![allow(
    clippy::absolute_paths,
    reason = "handler paths stay fully qualified: the route-auth scanner reads them"
)]

use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::api::FunctionsState;
use super::control_api::FunctionControlState;
use super::openapi::PATH_FUNCTIONS_OPENAPI;
use super::schema::PATH_FUNCTION_MANIFEST_SCHEMA;
use crate::shared::platform_context::{AggregateRoutes, PlatformContext};

pub fn routes(ctx: &PlatformContext) -> AggregateRoutes {
    let functions = functions_state(ctx);
    // The host control plane reads what the function routes write, with the
    // same artifact store and the same credentials resolver (read fresh).
    let control = FunctionControlState {
        desired: Arc::new(super::desired_state::DesiredStateBuilder {
            functions: ctx.repos.function_repo.clone(),
            versions: ctx.repos.function_version_repo.clone(),
            hosts: ctx.repos.function_host_repo.clone(),
            settings: functions.settings.clone(),
            routes: ctx.repos.function_route_repo.clone(),
            credentials: ctx.outbound_credentials.clone(),
        }),
        functions: ctx.repos.function_repo.clone(),
        versions: ctx.repos.function_version_repo.clone(),
        hosts: ctx.repos.function_host_repo.clone(),
        applications: ctx.repos.application_repo.clone(),
        event_types: ctx.repos.event_type_repo.clone(),
        events: ctx.repos.event_repo.clone(),
        artifacts: functions.ops.artifacts.clone(),
        unit_of_work: functions.ops.unit_of_work.clone(),
    };
    AggregateRoutes {
        // Full paths under five prefixes, so merged.
        documented: OpenApiRouter::new().merge(functions_router(functions)),
        plain: Router::new()
            // The host control plane (desired state, heartbeat, emit,
            // artifact download), gated on the host role.
            .merge(function_control_router(control))
            // The manifest's JSON Schema: unauthenticated, as in Java
            // (Platform.java:724-727), because an editor fetches it with no
            // token.
            .merge(function_manifest_schema_router())
            // Java's function API contract, verbatim and unauthenticated
            // (FunctionOpenApiRoutes.java).
            .merge(functions_openapi_router()),
    }
}

pub fn functions_state(ctx: &PlatformContext) -> FunctionsState {
    let repos = &ctx.repos;
    // Java reads the FC_FN_DEFAULT_* limits once at startup and refuses to
    // start on a non-positive one (Env.java:600-605).
    let limits = super::FunctionLimits::from_env()
        .unwrap_or_else(|e| panic!("invalid function limits: {e}"));
    let settings = Arc::new(super::settings_repository::FunctionSettingsRepository::new(
        &repos.pool,
        ctx.encryption.clone(),
    ));
    // FC_FN_ARTIFACT_STORE and FC_FN_SIGNATURES/FC_FN_TRUST_ROOT are resolved
    // once; an unrecognised store or signatures off outside dev mode refuse
    // to start, as in Java (ArtifactBlobStores.configure, Signatures.resolve).
    let artifacts = super::artifact::store_from_env()
        .unwrap_or_else(|e| panic!("invalid function artifact store: {e}"));
    let signatures = super::artifact::signatures_from_env()
        .unwrap_or_else(|e| panic!("invalid function signature settings: {e}"));
    // FC_FN_POOL_URL is resolved once; a bad template refuses to start, as
    // in Java (Env.java, PoolUrlTemplate).
    let pool_url = super::PoolUrlTemplate::from_env()
        .unwrap_or_else(|e| panic!("invalid function pool URL: {e}"));
    let trigger_sync =
        super::operations::TriggerSync::from_repositories(repos, settings.clone(), pool_url);
    FunctionsState {
        functions: repos.function_repo.clone(),
        versions: repos.function_version_repo.clone(),
        hosts: repos.function_host_repo.clone(),
        settings: settings.clone(),
        policies: repos.function_policy_repo.clone(),
        domains: repos.function_domain_repo.clone(),
        routes: repos.function_route_repo.clone(),
        trigger_objects: repos.function_trigger_object_repo.clone(),
        app_access: ctx.app_access.clone(),
        limits,
        ops: super::operations::FunctionOperations {
            functions: repos.function_repo.clone(),
            versions: repos.function_version_repo.clone(),
            applications: repos.application_repo.clone(),
            clients: repos.client_repo.clone(),
            settings,
            policies: repos.function_policy_repo.clone(),
            domains: repos.function_domain_repo.clone(),
            routes: repos.function_route_repo.clone(),
            trigger_sync,
            limits,
            signatures,
            artifacts,
            publish_checks: super::operations::PublishChecks {
                event_types: repos.event_type_repo.clone(),
                service_accounts: repos.service_account_repo.clone(),
                versions: repos.function_version_repo.clone(),
                functions: repos.function_repo.clone(),
                domains: repos.function_domain_repo.clone(),
                routes: repos.function_route_repo.clone(),
                hosts: repos.function_host_repo.clone(),
                limits,
            },
            unit_of_work: ctx.unit_of_work.clone(),
        },
    }
}

/// Every function route: `/api/functions*`, `/api/function-pools`,
/// `/api/function-policies*`, `/api/function-domains*` and
/// `/api/function-routes`, with full paths (merge, don't nest).
pub fn function_routes() -> OpenApiRouter<FunctionsState> {
    OpenApiRouter::new()
        .routes(routes!(
            crate::function::api::list_functions,
            crate::function::api::create_function
        ))
        .routes(routes!(
            crate::function::api::get_function,
            crate::function::api::update_function,
            crate::function::api::delete_function
        ))
        .routes(routes!(crate::function::api::function_status))
        .routes(routes!(crate::function::api::function_pools))
        .routes(routes!(
            crate::function::version_api::publish_version,
            crate::function::version_api::list_versions
        ))
        .routes(routes!(crate::function::version_api::get_version))
        .routes(routes!(crate::function::version_api::retire_version))
        .routes(routes!(crate::function::version_api::check_manifest))
        .routes(routes!(
            crate::function::version_api::promote,
            crate::function::version_api::remove_alias
        ))
        .routes(routes!(crate::function::version_api::list_aliases))
        .routes(routes!(crate::function::version_api::upload_artifact))
        .routes(routes!(
            crate::function::api::get_config,
            crate::function::api::put_config
        ))
        .routes(routes!(crate::function::api::get_secrets))
        .routes(routes!(
            crate::function::api::put_secret,
            crate::function::api::delete_secret
        ))
        .routes(routes!(crate::function::policy_api::list_policies))
        .routes(routes!(
            crate::function::policy_api::get_policy,
            crate::function::policy_api::put_policy
        ))
        .routes(routes!(
            crate::function::domain_api::claim_domain,
            crate::function::domain_api::list_domains
        ))
        .routes(routes!(
            crate::function::domain_api::get_domain,
            crate::function::domain_api::release_domain
        ))
        .routes(routes!(crate::function::domain_api::list_routes))
}

/// [`function_routes`] with its state. Their errors keep the function
/// contract (owner decision #5: `code` beside `error`, UPPER_SNAKE codes),
/// not the platform routes' Go envelope.
pub fn functions_router(state: FunctionsState) -> OpenApiRouter {
    function_routes()
        .with_state(state)
        .layer(axum::middleware::map_response(
            crate::shared::error::keep_function_contract,
        ))
}

/// The four routes. Not in the OpenAPI document: Java keeps the control
/// plane out of its lockfile too, naming it in `parity/surface.json`.
pub fn function_control_router(state: FunctionControlState) -> Router {
    Router::new()
        .route(
            "/control/functions/desired-state",
            get(crate::function::control_api::desired_state),
        )
        .route(
            "/control/functions/heartbeat",
            post(crate::function::control_api::heartbeat),
        )
        .route(
            "/control/functions/events",
            post(crate::function::control_api::emit_events),
        )
        .route(
            "/control/functions/artifacts/{version_id}",
            get(crate::function::control_api::download_artifact),
        )
        .with_state(state)
        // The function contract's errors (owner decision #5), not the
        // platform routes' Go envelope.
        .layer(axum::middleware::map_response(
            crate::shared::error::keep_function_contract,
        ))
}

/// The unauthenticated schema route, to merge into the platform router.
pub fn function_manifest_schema_router<S: Clone + Send + Sync + 'static>() -> Router<S> {
    Router::new().route(
        PATH_FUNCTION_MANIFEST_SCHEMA,
        get(crate::function::schema::function_manifest_schema),
    )
}

/// The unauthenticated route, to merge into the platform router.
pub fn functions_openapi_router<S: Clone + Send + Sync + 'static>() -> Router<S> {
    Router::new().route(
        PATH_FUNCTIONS_OPENAPI,
        get(crate::function::openapi::functions_openapi),
    )
}
