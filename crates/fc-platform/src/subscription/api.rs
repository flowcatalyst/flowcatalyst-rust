//! Subscriptions Admin API
//!
//! REST endpoints for subscription management.

use axum::{
    extract::{State, Path, Query},
    Json,
};
use utoipa_axum::{router::OpenApiRouter, routes};
use utoipa::{ToSchema, IntoParams};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{Subscription, EventTypeBinding, DispatchMode};
use crate::SubscriptionRepository;
use crate::shared::error::PlatformError;
use crate::shared::api_common::{PaginationParams, CreatedResponse, SuccessResponse};
use crate::shared::middleware::Authenticated;

/// Event type binding request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EventTypeBindingRequest {
    /// Event type code (with optional wildcards)
    pub event_type_code: String,

    /// Optional filter expression
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
}

/// Create subscription request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateSubscriptionRequest {
    /// Unique code
    pub code: String,

    /// Human-readable name
    pub name: String,

    /// Description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Target URL for webhook delivery
    pub target: String,

    /// Event types to listen to
    #[serde(default)]
    pub event_types: Vec<EventTypeBindingRequest>,

    /// Client ID (optional, null = anchor-level)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Dispatch pool ID for rate limiting
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatch_pool_id: Option<String>,

    /// Service account ID for authentication
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account_id: Option<String>,

    /// Dispatch mode
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,

    /// Timeout in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u32>,

    /// Maximum retry attempts
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,

    /// Send raw event data only
    #[serde(default)]
    pub data_only: bool,
}

/// Update subscription request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSubscriptionRequest {
    /// Human-readable name
    pub name: Option<String>,

    /// Description
    pub description: Option<String>,

    /// Target URL
    pub target: Option<String>,

    /// Timeout in seconds
    pub timeout_seconds: Option<u32>,

    /// Maximum retry attempts
    pub max_retries: Option<u32>,
}

/// Event type binding response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EventTypeBindingResponse {
    pub event_type_code: String,
    pub filter: Option<String>,
}

impl From<&EventTypeBinding> for EventTypeBindingResponse {
    fn from(b: &EventTypeBinding) -> Self {
        Self {
            event_type_code: b.event_type_code.clone(),
            filter: b.filter.clone(),
        }
    }
}

/// Config entry response (matches Java ConfigEntry)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigEntryResponse {
    pub key: String,
    pub value: String,
}

impl From<&crate::subscription::entity::ConfigEntry> for ConfigEntryResponse {
    fn from(c: &crate::subscription::entity::ConfigEntry) -> Self {
        Self {
            key: c.key.clone(),
            value: c.value.clone(),
        }
    }
}

/// Subscription response DTO (matches Java SubscriptionDto)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionResponse {
    pub id: String,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub client_id: Option<String>,
    pub client_identifier: Option<String>,
    pub event_types: Vec<EventTypeBindingResponse>,
    pub target: String,
    pub queue: Option<String>,
    pub custom_config: Vec<ConfigEntryResponse>,
    pub source: Option<String>,
    pub status: String,
    pub max_age_seconds: u32,
    pub dispatch_pool_id: Option<String>,
    pub dispatch_pool_code: Option<String>,
    pub delay_seconds: u32,
    pub sequence: i32,
    pub mode: String,
    pub timeout_seconds: u32,
    pub max_retries: u32,
    pub service_account_id: Option<String>,
    pub data_only: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl From<Subscription> for SubscriptionResponse {
    fn from(s: Subscription) -> Self {
        Self {
            id: s.id,
            code: s.code,
            name: s.name,
            description: s.description,
            client_id: s.client_id,
            client_identifier: None, // Denormalized, populated by projection
            event_types: s.event_types.iter().map(|e| e.into()).collect(),
            target: s.target,
            queue: s.queue,
            custom_config: s.custom_config.iter().map(|c| c.into()).collect(),
            source: None, // Not tracked in Rust domain yet
            status: format!("{:?}", s.status).to_uppercase(),
            max_age_seconds: 86400, // Default 24 hours
            dispatch_pool_id: s.dispatch_pool_id,
            dispatch_pool_code: None, // Denormalized, populated by projection
            delay_seconds: s.delay_seconds,
            sequence: s.sequence,
            mode: format!("{:?}", s.mode).to_uppercase(),
            timeout_seconds: s.timeout_seconds,
            max_retries: s.max_retries,
            service_account_id: s.service_account_id,
            data_only: s.data_only,
            created_at: s.created_at.to_rfc3339(),
            updated_at: s.updated_at.to_rfc3339(),
        }
    }
}

/// Subscription list response (matches Java SubscriptionListResponse)
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionListResponse {
    pub subscriptions: Vec<SubscriptionResponse>,
    pub total: usize,
}

/// Query parameters for subscriptions list
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct SubscriptionsQuery {
    #[serde(flatten)]
    pub pagination: PaginationParams,

    /// Filter by client ID
    pub client_id: Option<String>,

    /// Filter by status
    pub status: Option<String>,
}

/// Subscriptions service state
#[derive(Clone)]
pub struct SubscriptionsState {
    pub subscription_repo: Arc<SubscriptionRepository>,
}

fn parse_mode(s: &str) -> Result<DispatchMode, PlatformError> {
    match s.to_uppercase().as_str() {
        "IMMEDIATE" => Ok(DispatchMode::Immediate),
        "NEXT_ON_ERROR" => Ok(DispatchMode::NextOnError),
        "BLOCK_ON_ERROR" => Ok(DispatchMode::BlockOnError),
        _ => Err(PlatformError::validation(format!("Invalid mode: {}. Valid options: IMMEDIATE, NEXT_ON_ERROR, BLOCK_ON_ERROR", s))),
    }
}

/// Create a new subscription
#[utoipa::path(
    post,
    path = "",
    tag = "subscriptions",
    operation_id = "postApiAdminPlatformSubscriptions",
    request_body = CreateSubscriptionRequest,
    responses(
        (status = 201, description = "Subscription created", body = CreatedResponse),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Duplicate code")
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_subscription(
    State(state): State<SubscriptionsState>,
    auth: Authenticated,
    Json(req): Json<CreateSubscriptionRequest>,
) -> Result<Json<CreatedResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_write_subscriptions(&auth.0)?;

    // Validate client access if specified
    if let Some(ref cid) = req.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden(format!("No access to client: {}", cid)));
        }
    } else if !auth.0.is_anchor() {
        return Err(PlatformError::forbidden("Only anchor users can create anchor-level subscriptions"));
    }

    // Check for duplicate code
    if let Some(_) = state.subscription_repo.find_by_code(&req.code).await? {
        return Err(PlatformError::duplicate("Subscription", "code", &req.code));
    }

    let mut subscription = Subscription::new(&req.code, &req.name, &req.target);

    if let Some(desc) = req.description {
        subscription = subscription.with_description(desc);
    }
    if let Some(cid) = req.client_id {
        subscription = subscription.with_client_id(cid);
    }
    if let Some(pool_id) = req.dispatch_pool_id {
        subscription = subscription.with_dispatch_pool_id(pool_id);
    }
    if let Some(account_id) = req.service_account_id {
        subscription = subscription.with_service_account_id(account_id);
    }
    if let Some(mode_str) = req.mode {
        subscription = subscription.with_mode(parse_mode(&mode_str)?);
    }

    subscription = subscription.with_data_only(req.data_only);

    if let Some(timeout) = req.timeout_seconds {
        subscription.timeout_seconds = timeout;
    }
    if let Some(retries) = req.max_retries {
        subscription.max_retries = retries;
    }

    // Add event type bindings
    for binding in req.event_types {
        let mut eb = EventTypeBinding::new(&binding.event_type_code);
        if let Some(filter) = binding.filter {
            eb = eb.with_filter(filter);
        }
        subscription = subscription.with_event_type_binding(eb);
    }

    let id = subscription.id.clone();
    state.subscription_repo.insert(&subscription).await?;

    Ok(Json(CreatedResponse::new(id)))
}

/// Get subscription by ID
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "subscriptions",
    operation_id = "getApiAdminPlatformSubscriptionsById",
    params(
        ("id" = String, Path, description = "Subscription ID")
    ),
    responses(
        (status = 200, description = "Subscription found", body = SubscriptionResponse),
        (status = 404, description = "Subscription not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_subscription(
    State(state): State<SubscriptionsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<SubscriptionResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_subscriptions(&auth.0)?;

    let subscription = state.subscription_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("Subscription", &id))?;

    // Check client access
    if let Some(ref cid) = subscription.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access to this subscription"));
        }
    }

    Ok(Json(subscription.into()))
}

/// List subscriptions
#[utoipa::path(
    get,
    path = "",
    tag = "subscriptions",
    operation_id = "getApiAdminPlatformSubscriptions",
    params(SubscriptionsQuery),
    responses(
        (status = 200, description = "List of subscriptions", body = SubscriptionListResponse)
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_subscriptions(
    State(state): State<SubscriptionsState>,
    auth: Authenticated,
    Query(query): Query<SubscriptionsQuery>,
) -> Result<Json<SubscriptionListResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_read_subscriptions(&auth.0)?;

    let subscriptions = if let Some(ref client_id) = query.client_id {
        if !auth.0.can_access_client(client_id) {
            return Err(PlatformError::forbidden(format!("No access to client: {}", client_id)));
        }
        state.subscription_repo.find_by_client(Some(client_id)).await?
    } else {
        state.subscription_repo.find_active().await?
    };

    // Filter by client access
    let filtered: Vec<SubscriptionResponse> = subscriptions.into_iter()
        .filter(|s| {
            match &s.client_id {
                Some(cid) => auth.0.can_access_client(cid),
                None => auth.0.is_anchor(),
            }
        })
        .map(|s| s.into())
        .collect();

    let total = filtered.len();
    Ok(Json(SubscriptionListResponse { subscriptions: filtered, total }))
}

/// Update subscription
#[utoipa::path(
    put,
    path = "/{id}",
    tag = "subscriptions",
    operation_id = "putApiAdminPlatformSubscriptionsById",
    params(
        ("id" = String, Path, description = "Subscription ID")
    ),
    request_body = UpdateSubscriptionRequest,
    responses(
        (status = 200, description = "Subscription updated", body = SubscriptionResponse),
        (status = 404, description = "Subscription not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_subscription(
    State(state): State<SubscriptionsState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateSubscriptionRequest>,
) -> Result<Json<SubscriptionResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_write_subscriptions(&auth.0)?;

    let mut subscription = state.subscription_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("Subscription", &id))?;

    // Check client access
    if let Some(ref cid) = subscription.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access to this subscription"));
        }
    } else if !auth.0.is_anchor() {
        return Err(PlatformError::forbidden("Only anchor users can modify anchor-level subscriptions"));
    }

    // Update fields
    if let Some(name) = req.name {
        subscription.name = name;
    }
    if let Some(desc) = req.description {
        subscription.description = Some(desc);
    }
    if let Some(target) = req.target {
        subscription.target = target;
    }
    if let Some(timeout) = req.timeout_seconds {
        subscription.timeout_seconds = timeout;
    }
    if let Some(retries) = req.max_retries {
        subscription.max_retries = retries;
    }

    subscription.updated_at = chrono::Utc::now();
    state.subscription_repo.update(&subscription).await?;

    Ok(Json(subscription.into()))
}

/// Pause subscription
#[utoipa::path(
    post,
    path = "/{id}/pause",
    tag = "subscriptions",
    operation_id = "postApiAdminPlatformSubscriptionsByIdPause",
    params(
        ("id" = String, Path, description = "Subscription ID")
    ),
    responses(
        (status = 200, description = "Subscription paused", body = SubscriptionResponse),
        (status = 404, description = "Subscription not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn pause_subscription(
    State(state): State<SubscriptionsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<SubscriptionResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_write_subscriptions(&auth.0)?;

    let mut subscription = state.subscription_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("Subscription", &id))?;

    // Check client access
    if let Some(ref cid) = subscription.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access to this subscription"));
        }
    }

    subscription.pause();
    state.subscription_repo.update(&subscription).await?;

    Ok(Json(subscription.into()))
}

/// Resume subscription
#[utoipa::path(
    post,
    path = "/{id}/resume",
    tag = "subscriptions",
    operation_id = "postApiAdminPlatformSubscriptionsByIdResume",
    params(
        ("id" = String, Path, description = "Subscription ID")
    ),
    responses(
        (status = 200, description = "Subscription resumed", body = SubscriptionResponse),
        (status = 404, description = "Subscription not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn resume_subscription(
    State(state): State<SubscriptionsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<SubscriptionResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_write_subscriptions(&auth.0)?;

    let mut subscription = state.subscription_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("Subscription", &id))?;

    // Check client access
    if let Some(ref cid) = subscription.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access to this subscription"));
        }
    }

    subscription.resume();
    state.subscription_repo.update(&subscription).await?;

    Ok(Json(subscription.into()))
}

/// Delete subscription (archive)
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "subscriptions",
    operation_id = "deleteApiAdminPlatformSubscriptionsById",
    params(
        ("id" = String, Path, description = "Subscription ID")
    ),
    responses(
        (status = 200, description = "Subscription archived", body = SuccessResponse),
        (status = 404, description = "Subscription not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_subscription(
    State(state): State<SubscriptionsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<SuccessResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_delete_subscriptions(&auth.0)?;

    let mut subscription = state.subscription_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("Subscription", &id))?;

    // Check client access
    if let Some(ref cid) = subscription.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access to this subscription"));
        }
    } else if !auth.0.is_anchor() {
        return Err(PlatformError::forbidden("Only anchor users can delete anchor-level subscriptions"));
    }

    subscription.archive();
    state.subscription_repo.update(&subscription).await?;

    Ok(Json(SuccessResponse::ok()))
}

/// Reactivate an archived subscription
///
/// Reactivates a previously archived subscription, setting it back to active status.
#[utoipa::path(
    post,
    path = "/{id}/reactivate",
    tag = "subscriptions",
    operation_id = "postApiAdminPlatformSubscriptionsByIdReactivate",
    params(
        ("id" = String, Path, description = "Subscription ID")
    ),
    responses(
        (status = 200, description = "Subscription reactivated", body = SubscriptionResponse),
        (status = 404, description = "Subscription not found"),
        (status = 400, description = "Subscription is not archived")
    ),
    security(("bearer_auth" = []))
)]
pub async fn reactivate_subscription(
    State(state): State<SubscriptionsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<Json<SubscriptionResponse>, PlatformError> {
    crate::shared::authorization_service::checks::can_write_subscriptions(&auth.0)?;

    let mut subscription = state.subscription_repo.find_by_id(&id).await?
        .ok_or_else(|| PlatformError::not_found("Subscription", &id))?;

    // Check client access
    if let Some(ref cid) = subscription.client_id {
        if !auth.0.can_access_client(cid) {
            return Err(PlatformError::forbidden("No access to this subscription"));
        }
    } else if !auth.0.is_anchor() {
        return Err(PlatformError::forbidden("Only anchor users can reactivate anchor-level subscriptions"));
    }

    // Check that subscription is archived
    if subscription.status != crate::SubscriptionStatus::Archived {
        return Err(PlatformError::validation(
            "Only archived subscriptions can be reactivated. Use /resume for paused subscriptions."
        ));
    }

    subscription.resume(); // This sets status back to Active
    state.subscription_repo.update(&subscription).await?;

    tracing::info!(subscription_id = %id, principal_id = %auth.0.principal_id, "Subscription reactivated");

    Ok(Json(subscription.into()))
}

/// Create subscriptions router
pub fn subscriptions_router(state: SubscriptionsState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(create_subscription, list_subscriptions))
        .routes(routes!(get_subscription, update_subscription, delete_subscription))
        .routes(routes!(pause_subscription))
        .routes(routes!(resume_subscription))
        .routes(routes!(reactivate_subscription))
        .with_state(state)
}
