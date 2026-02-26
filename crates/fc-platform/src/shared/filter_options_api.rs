//! Filter Options BFF API
//!
//! REST endpoints for fetching filter options for UI dropdowns.

use axum::{
    routing::get,
    extract::{State, Query},
    Json, Router,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use utoipa::{ToSchema, IntoParams};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::collections::HashSet;

use crate::{
    ClientRepository, EventTypeRepository, SubscriptionRepository,
    DispatchPoolRepository, ApplicationRepository,
};
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;

/// Filter option item
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FilterOption {
    pub value: String,
    pub label: String,
}

/// Client filter options response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientFilterOptions {
    pub clients: Vec<FilterOption>,
}

/// Event type filter options response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EventTypeFilterOptions {
    pub event_types: Vec<FilterOption>,
    pub applications: Vec<FilterOption>,
    pub subdomains: Vec<FilterOption>,
}

/// Subscription filter options response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionFilterOptions {
    pub subscriptions: Vec<FilterOption>,
}

/// Dispatch pool filter options response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DispatchPoolFilterOptions {
    pub dispatch_pools: Vec<FilterOption>,
}

/// All filter options combined
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AllFilterOptions {
    pub clients: Vec<FilterOption>,
    pub event_types: Vec<FilterOption>,
    pub applications: Vec<FilterOption>,
    pub subscriptions: Vec<FilterOption>,
    pub dispatch_pools: Vec<FilterOption>,
}

/// Filter options service state
#[derive(Clone)]
pub struct FilterOptionsState {
    pub client_repo: Arc<ClientRepository>,
    pub event_type_repo: Arc<EventTypeRepository>,
    pub subscription_repo: Arc<SubscriptionRepository>,
    pub dispatch_pool_repo: Arc<DispatchPoolRepository>,
    pub application_repo: Arc<ApplicationRepository>,
}

/// Get client filter options
#[utoipa::path(
    get,
    path = "/clients",
    tag = "filter-options",
    operation_id = "getApiBffFilterOptionsClients",
    responses(
        (status = 200, description = "Client filter options", body = ClientFilterOptions)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_client_options(
    State(state): State<FilterOptionsState>,
    auth: Authenticated,
) -> Result<Json<ClientFilterOptions>, PlatformError> {
    let clients = state.client_repo.find_active().await?;

    // Filter by access
    let options: Vec<FilterOption> = clients.into_iter()
        .filter(|c| auth.0.is_anchor() || auth.0.can_access_client(&c.id))
        .map(|c| FilterOption {
            value: c.id,
            label: c.name,
        })
        .collect();

    Ok(Json(ClientFilterOptions { clients: options }))
}

/// Get event type filter options
#[utoipa::path(
    get,
    path = "/event-types",
    tag = "filter-options",
    operation_id = "getApiBffFilterOptionsEventTypes",
    responses(
        (status = 200, description = "Event type filter options", body = EventTypeFilterOptions)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_event_type_options(
    State(state): State<FilterOptionsState>,
    _auth: Authenticated,
) -> Result<Json<EventTypeFilterOptions>, PlatformError> {
    let event_types = state.event_type_repo.find_active().await?;

    // Build event type options
    let event_type_options: Vec<FilterOption> = event_types.iter()
        .map(|et| FilterOption {
            value: et.code.clone(),
            label: et.name.clone(),
        })
        .collect();

    // Extract unique applications
    let mut applications: Vec<FilterOption> = event_types.iter()
        .map(|et| et.application.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .map(|app| FilterOption {
            value: app.clone(),
            label: app,
        })
        .collect();
    applications.sort_by(|a, b| a.label.cmp(&b.label));

    // Extract unique subdomains
    let mut subdomains: Vec<FilterOption> = event_types.iter()
        .map(|et| et.subdomain.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .map(|sub| FilterOption {
            value: sub.clone(),
            label: sub,
        })
        .collect();
    subdomains.sort_by(|a, b| a.label.cmp(&b.label));

    Ok(Json(EventTypeFilterOptions {
        event_types: event_type_options,
        applications,
        subdomains,
    }))
}

/// Get subscription filter options
#[utoipa::path(
    get,
    path = "/subscriptions",
    tag = "filter-options",
    operation_id = "getApiBffFilterOptionsSubscriptions",
    responses(
        (status = 200, description = "Subscription filter options", body = SubscriptionFilterOptions)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_subscription_options(
    State(state): State<FilterOptionsState>,
    auth: Authenticated,
) -> Result<Json<SubscriptionFilterOptions>, PlatformError> {
    let subscriptions = state.subscription_repo.find_active().await?;

    // Filter by access
    let options: Vec<FilterOption> = subscriptions.into_iter()
        .filter(|s| {
            match &s.client_id {
                Some(cid) => auth.0.is_anchor() || auth.0.can_access_client(cid),
                None => auth.0.is_anchor(),
            }
        })
        .map(|s| FilterOption {
            value: s.id,
            label: s.name,
        })
        .collect();

    Ok(Json(SubscriptionFilterOptions { subscriptions: options }))
}

/// Get dispatch pool filter options
#[utoipa::path(
    get,
    path = "/dispatch-pools",
    tag = "filter-options",
    operation_id = "getApiBffFilterOptionsDispatchPools",
    responses(
        (status = 200, description = "Dispatch pool filter options", body = DispatchPoolFilterOptions)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_dispatch_pool_options(
    State(state): State<FilterOptionsState>,
    auth: Authenticated,
) -> Result<Json<DispatchPoolFilterOptions>, PlatformError> {
    let pools = state.dispatch_pool_repo.find_active().await?;

    // Filter by access
    let options: Vec<FilterOption> = pools.into_iter()
        .filter(|p| {
            match &p.client_id {
                Some(cid) => auth.0.is_anchor() || auth.0.can_access_client(cid),
                None => true, // Anchor-level pools visible to all
            }
        })
        .map(|p| FilterOption {
            value: p.id,
            label: p.name,
        })
        .collect();

    Ok(Json(DispatchPoolFilterOptions { dispatch_pools: options }))
}

/// Get all filter options at once
#[utoipa::path(
    get,
    path = "",
    tag = "filter-options",
    operation_id = "getApiBffFilterOptions",
    responses(
        (status = 200, description = "All filter options", body = AllFilterOptions)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_all_options(
    State(state): State<FilterOptionsState>,
    auth: Authenticated,
) -> Result<Json<AllFilterOptions>, PlatformError> {
    // Get clients
    let clients = state.client_repo.find_active().await?;
    let client_options: Vec<FilterOption> = clients.into_iter()
        .filter(|c| auth.0.is_anchor() || auth.0.can_access_client(&c.id))
        .map(|c| FilterOption {
            value: c.id,
            label: c.name,
        })
        .collect();

    // Get event types
    let event_types = state.event_type_repo.find_active().await?;
    let event_type_options: Vec<FilterOption> = event_types.iter()
        .map(|et| FilterOption {
            value: et.code.clone(),
            label: et.name.clone(),
        })
        .collect();

    // Get applications
    let apps = state.application_repo.find_active().await?;
    let app_options: Vec<FilterOption> = apps.into_iter()
        .map(|a| FilterOption {
            value: a.code,
            label: a.name,
        })
        .collect();

    // Get subscriptions
    let subscriptions = state.subscription_repo.find_active().await?;
    let subscription_options: Vec<FilterOption> = subscriptions.into_iter()
        .filter(|s| {
            match &s.client_id {
                Some(cid) => auth.0.is_anchor() || auth.0.can_access_client(cid),
                None => auth.0.is_anchor(),
            }
        })
        .map(|s| FilterOption {
            value: s.id,
            label: s.name,
        })
        .collect();

    // Get dispatch pools
    let pools = state.dispatch_pool_repo.find_active().await?;
    let pool_options: Vec<FilterOption> = pools.into_iter()
        .filter(|p| {
            match &p.client_id {
                Some(cid) => auth.0.is_anchor() || auth.0.can_access_client(cid),
                None => true,
            }
        })
        .map(|p| FilterOption {
            value: p.id,
            label: p.name,
        })
        .collect();

    Ok(Json(AllFilterOptions {
        clients: client_options,
        event_types: event_type_options,
        applications: app_options,
        subscriptions: subscription_options,
        dispatch_pools: pool_options,
    }))
}

/// Events filter options response (for events list page)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EventsFilterOptions {
    pub clients: Vec<FilterOption>,
    pub event_types: Vec<FilterOption>,
    pub applications: Vec<FilterOption>,
    pub subdomains: Vec<FilterOption>,
}

/// Dispatch jobs filter options response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DispatchJobsFilterOptions {
    pub clients: Vec<FilterOption>,
    pub event_types: Vec<FilterOption>,
    pub subscriptions: Vec<FilterOption>,
    pub statuses: Vec<FilterOption>,
}

/// Cascading filter query parameters
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct CascadingFilterQuery {
    /// Filter by application(s)
    #[serde(default, rename = "application[]")]
    pub applications: Vec<String>,
    /// Filter by subdomain(s)
    #[serde(default, rename = "subdomain[]")]
    pub subdomains: Vec<String>,
}

/// Get events filter options (cascading)
#[utoipa::path(
    get,
    path = "/events",
    tag = "filter-options",
    operation_id = "getApiBffEventsFilterOptions",
    responses(
        (status = 200, description = "Events filter options", body = EventsFilterOptions)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_events_filter_options(
    State(state): State<FilterOptionsState>,
    auth: Authenticated,
) -> Result<Json<EventsFilterOptions>, PlatformError> {
    // Get clients the user can access
    let clients = state.client_repo.find_active().await?;
    let client_options: Vec<FilterOption> = clients.into_iter()
        .filter(|c| auth.0.is_anchor() || auth.0.can_access_client(&c.id))
        .map(|c| FilterOption {
            value: c.id,
            label: c.name,
        })
        .collect();

    // Get event types
    let event_types = state.event_type_repo.find_active().await?;

    let event_type_options: Vec<FilterOption> = event_types.iter()
        .map(|et| FilterOption {
            value: et.code.clone(),
            label: et.name.clone(),
        })
        .collect();

    // Extract unique applications
    let mut applications: Vec<FilterOption> = event_types.iter()
        .map(|et| et.application.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .map(|app| FilterOption {
            value: app.clone(),
            label: app,
        })
        .collect();
    applications.sort_by(|a, b| a.label.cmp(&b.label));

    // Extract unique subdomains
    let mut subdomains: Vec<FilterOption> = event_types.iter()
        .map(|et| et.subdomain.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .map(|sub| FilterOption {
            value: sub.clone(),
            label: sub,
        })
        .collect();
    subdomains.sort_by(|a, b| a.label.cmp(&b.label));

    Ok(Json(EventsFilterOptions {
        clients: client_options,
        event_types: event_type_options,
        applications,
        subdomains,
    }))
}

/// Dispatch job status options
const DISPATCH_JOB_STATUSES: &[(&str, &str)] = &[
    ("PENDING", "Pending"),
    ("SCHEDULED", "Scheduled"),
    ("IN_PROGRESS", "In Progress"),
    ("COMPLETED", "Completed"),
    ("FAILED", "Failed"),
    ("CANCELLED", "Cancelled"),
];

/// Get dispatch jobs filter options
#[utoipa::path(
    get,
    path = "/dispatch-jobs",
    tag = "filter-options",
    operation_id = "getApiBffDispatchJobsFilterOptions",
    responses(
        (status = 200, description = "Dispatch jobs filter options", body = DispatchJobsFilterOptions)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_dispatch_jobs_filter_options(
    State(state): State<FilterOptionsState>,
    auth: Authenticated,
) -> Result<Json<DispatchJobsFilterOptions>, PlatformError> {
    // Get clients the user can access
    let clients = state.client_repo.find_active().await?;
    let client_options: Vec<FilterOption> = clients.into_iter()
        .filter(|c| auth.0.is_anchor() || auth.0.can_access_client(&c.id))
        .map(|c| FilterOption {
            value: c.id,
            label: c.name,
        })
        .collect();

    // Get event types
    let event_types = state.event_type_repo.find_active().await?;
    let event_type_options: Vec<FilterOption> = event_types.iter()
        .map(|et| FilterOption {
            value: et.code.clone(),
            label: et.name.clone(),
        })
        .collect();

    // Get subscriptions the user can access
    let subscriptions = state.subscription_repo.find_active().await?;
    let subscription_options: Vec<FilterOption> = subscriptions.into_iter()
        .filter(|s| {
            match &s.client_id {
                Some(cid) => auth.0.is_anchor() || auth.0.can_access_client(cid),
                None => auth.0.is_anchor(),
            }
        })
        .map(|s| FilterOption {
            value: s.id,
            label: s.name,
        })
        .collect();

    // Status options
    let status_options: Vec<FilterOption> = DISPATCH_JOB_STATUSES.iter()
        .map(|(value, label)| FilterOption {
            value: value.to_string(),
            label: label.to_string(),
        })
        .collect();

    Ok(Json(DispatchJobsFilterOptions {
        clients: client_options,
        event_types: event_type_options,
        subscriptions: subscription_options,
        statuses: status_options,
    }))
}

/// Applications list response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationsResponse {
    pub applications: Vec<FilterOption>,
}

/// Subdomains list response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubdomainsResponse {
    pub subdomains: Vec<FilterOption>,
}

/// Aggregates list response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AggregatesResponse {
    pub aggregates: Vec<FilterOption>,
}

/// Get applications for event type cascading filter
#[utoipa::path(
    get,
    path = "/event-types/filters/applications",
    tag = "filter-options",
    operation_id = "getApiBffFilterOptionsEventTypesFiltersApplications",
    responses(
        (status = 200, description = "Application filter options", body = ApplicationsResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_event_type_applications(
    State(state): State<FilterOptionsState>,
    _auth: Authenticated,
) -> Result<Json<ApplicationsResponse>, PlatformError> {
    let event_types = state.event_type_repo.find_active().await?;

    let mut applications: Vec<FilterOption> = event_types.iter()
        .map(|et| et.application.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .map(|app| FilterOption {
            value: app.clone(),
            label: app,
        })
        .collect();
    applications.sort_by(|a, b| a.label.cmp(&b.label));

    Ok(Json(ApplicationsResponse { applications }))
}

/// Get subdomains for event type cascading filter (filtered by application)
#[utoipa::path(
    get,
    path = "/event-types/filters/subdomains",
    tag = "filter-options",
    operation_id = "getApiBffFilterOptionsEventTypesFiltersSubdomains",
    params(CascadingFilterQuery),
    responses(
        (status = 200, description = "Subdomain filter options", body = SubdomainsResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_event_type_subdomains(
    State(state): State<FilterOptionsState>,
    Query(query): Query<CascadingFilterQuery>,
    _auth: Authenticated,
) -> Result<Json<SubdomainsResponse>, PlatformError> {
    let event_types = state.event_type_repo.find_active().await?;

    // Filter by applications if specified
    let filtered = if query.applications.is_empty() {
        event_types
    } else {
        event_types.into_iter()
            .filter(|et| query.applications.contains(&et.application))
            .collect()
    };

    let mut subdomains: Vec<FilterOption> = filtered.iter()
        .map(|et| et.subdomain.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .map(|sub| FilterOption {
            value: sub.clone(),
            label: sub,
        })
        .collect();
    subdomains.sort_by(|a, b| a.label.cmp(&b.label));

    Ok(Json(SubdomainsResponse { subdomains }))
}

/// Get aggregates for event type cascading filter (filtered by application and subdomain)
#[utoipa::path(
    get,
    path = "/event-types/filters/aggregates",
    tag = "filter-options",
    operation_id = "getApiBffFilterOptionsEventTypesFiltersAggregates",
    params(CascadingFilterQuery),
    responses(
        (status = 200, description = "Aggregate filter options", body = AggregatesResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_event_type_aggregates(
    State(state): State<FilterOptionsState>,
    Query(query): Query<CascadingFilterQuery>,
    _auth: Authenticated,
) -> Result<Json<AggregatesResponse>, PlatformError> {
    let event_types = state.event_type_repo.find_active().await?;

    // Filter by applications and subdomains if specified
    let filtered: Vec<_> = event_types.into_iter()
        .filter(|et| {
            let app_match = query.applications.is_empty() || query.applications.contains(&et.application);
            let sub_match = query.subdomains.is_empty() || query.subdomains.contains(&et.subdomain);
            app_match && sub_match
        })
        .collect();

    let mut aggregates: Vec<FilterOption> = filtered.iter()
        .map(|et| et.aggregate.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .map(|agg| FilterOption {
            value: agg.clone(),
            label: agg,
        })
        .collect();
    aggregates.sort_by(|a, b| a.label.cmp(&b.label));

    Ok(Json(AggregatesResponse { aggregates }))
}

/// Create filter options router
pub fn filter_options_router(state: FilterOptionsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(get_all_options))
        .routes(routes!(get_client_options))
        .routes(routes!(get_event_type_options))
        .routes(routes!(get_subscription_options))
        .routes(routes!(get_dispatch_pool_options))
        .routes(routes!(get_events_filter_options))
        .routes(routes!(get_dispatch_jobs_filter_options))
        .routes(routes!(get_event_type_applications))
        .routes(routes!(get_event_type_subdomains))
        .routes(routes!(get_event_type_aggregates))
        .with_state(state)
}

/// Create event-type filters router (for mounting at /bff/event-types/filters)
/// This provides the same endpoints as filter_options_router but at a different path
/// to maintain backwards compatibility with frontend expectations.
pub fn event_type_filters_router(state: FilterOptionsState) -> Router {
    Router::new()
        .route("/applications", get(get_event_type_applications))
        .route("/subdomains", get(get_event_type_subdomains))
        .route("/aggregates", get(get_event_type_aggregates))
        .with_state(state)
}
