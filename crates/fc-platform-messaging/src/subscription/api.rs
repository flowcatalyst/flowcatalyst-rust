//! Subscriptions Admin API
//!
//! REST endpoints for subscription management.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use fc_platform_core::shared::id::ConnectionId;
use fc_platform_core::shared::id::DispatchPoolId;
use fc_platform_core::shared::id::EventTypeId;
use fc_platform_core::shared::id::SubscriptionId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};

use crate::dispatch_job::entity;
use crate::subscription::access;
use crate::subscription::entity::ConfigEntry;
use crate::subscription::entity::SubscriptionStatus;
use crate::subscription::entity::{EventTypeBinding, Subscription};
use crate::subscription::operations::CreateSubscriptionUseCase;
use crate::subscription::operations::DeleteSubscriptionUseCase;
use crate::subscription::operations::EventTypeBindingInput;
use crate::subscription::operations::PauseSubscriptionUseCase;
use crate::subscription::operations::ResumeSubscriptionUseCase;
use crate::subscription::operations::UpdateSubscriptionUseCase;
use crate::subscription::repository::SubscriptionRepository;
use fc_platform_core::shared::api_common::CreatedResponse;
use fc_platform_core::shared::api_common::PaginationParams;
use fc_platform_core::shared::authorization_service::checks;
use fc_platform_core::shared::caller_reach;
use fc_platform_core::shared::enum_str;
use fc_platform_core::shared::error::PlatformError;
use fc_platform_core::shared::id::ClientId;
use fc_platform_core::shared::id::OptionIdExt;
use fc_platform_core::shared::middleware::Authenticated;
use fc_platform_core::usecase::PgUnitOfWork;

/// Event type binding request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = EventTypeBindingDTO)]
pub struct EventTypeBindingRequest {
    /// Event type code (with optional wildcards)
    pub event_type_code: String,

    /// Optional filter expression
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,

    /// Event type id (optional; stored as sent)
    #[serde(default)]
    pub event_type_id: Option<String>,

    /// Spec version bound to (optional; stored as sent)
    #[serde(default)]
    pub spec_version: Option<String>,
}

impl EventTypeBindingRequest {
    fn into_input(self) -> EventTypeBindingInput {
        EventTypeBindingInput {
            event_type_code: self.event_type_code,
            filter: self.filter,
            event_type_id: self.event_type_id.map(EventTypeId::from_wire),
            spec_version: self.spec_version,
        }
    }
}

/// Config entry request (Go `ConfigEntryDTO`)
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = ConfigEntryDTO)]
pub struct ConfigEntryRequest {
    pub key: String,
    pub value: String,
}

fn config_entries(entries: Option<Vec<ConfigEntryRequest>>) -> Option<Vec<ConfigEntry>> {
    entries.map(|v| {
        v.into_iter()
            .map(|c| ConfigEntry {
                key: c.key,
                value: c.value,
            })
            .collect()
    })
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

    /// Webhook endpoint URL
    pub endpoint: String,

    /// Connection ID (references msg_connections, optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<String>,

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
    #[schema(value_type = Option<i32>)]
    pub timeout_seconds: Option<u32>,

    /// Maximum retry attempts
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<i32>)]
    pub max_retries: Option<u32>,

    /// Send raw event data only (absent: true, as Go's default)
    #[serde(default)]
    pub data_only: Option<bool>,

    /// Dispatch priority: DEFAULT or HIGH_PRIORITY (any case)
    #[serde(default)]
    pub queue: Option<String>,

    /// Delivery delay in seconds
    #[serde(default)]
    pub delay_seconds: Option<i32>,

    /// Maximum message age in seconds
    #[serde(default)]
    pub max_age_seconds: Option<i32>,

    /// Custom configuration entries
    #[serde(default)]
    pub custom_config: Option<Vec<ConfigEntryRequest>>,
}

/// Update subscription request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSubscriptionRequest {
    /// Human-readable name
    pub name: Option<String>,

    /// Description
    pub description: Option<String>,

    /// Webhook endpoint URL
    pub endpoint: Option<String>,

    /// Connection ID
    pub connection_id: Option<String>,

    /// Timeout in seconds
    #[schema(value_type = Option<i32>)]
    pub timeout_seconds: Option<u32>,

    /// Maximum retry attempts
    #[schema(value_type = Option<i32>)]
    pub max_retries: Option<u32>,

    /// Event types (replace the existing bindings when given)
    #[serde(default)]
    pub event_types: Option<Vec<EventTypeBindingRequest>>,

    /// Custom configuration (replaces the existing entries when given)
    #[serde(default)]
    pub custom_config: Option<Vec<ConfigEntryRequest>>,

    /// Dispatch mode (absent: unchanged; unknown: NEXT_ON_ERROR, X-01)
    #[serde(default)]
    pub mode: Option<String>,

    /// Delivery delay in seconds
    #[serde(default)]
    pub delay_seconds: Option<i32>,

    /// Maximum message age in seconds
    #[serde(default)]
    pub max_age_seconds: Option<i32>,

    /// Dispatch pool id
    #[serde(default)]
    pub dispatch_pool_id: Option<String>,

    /// Service account id
    #[serde(default)]
    pub service_account_id: Option<String>,

    /// Send raw event data only
    #[serde(default)]
    pub data_only: Option<bool>,

    /// Dispatch priority; an explicit blank clears it
    #[serde(default)]
    pub queue: Option<String>,
}

/// Event type binding response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = EventTypeBindingDTO)]
pub struct EventTypeBindingResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_type_id: Option<String>,
    pub event_type_code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
}

impl From<&EventTypeBinding> for EventTypeBindingResponse {
    fn from(b: &EventTypeBinding) -> Self {
        Self {
            event_type_id: b.event_type_id.as_ref().map(EventTypeId::to_string),
            event_type_code: b.event_type_code.clone(),
            spec_version: b.spec_version.clone(),
            filter: b.filter.clone(),
        }
    }
}

/// Config entry response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(as = ConfigEntryDTO)]
pub struct ConfigEntryResponse {
    pub key: String,
    pub value: String,
}

impl From<&ConfigEntry> for ConfigEntryResponse {
    fn from(c: &ConfigEntry) -> Self {
        Self {
            key: c.key.clone(),
            value: c.value.clone(),
        }
    }
}

/// Subscription response DTO (Go `SubscriptionResponse`: optional members
/// are omitted when unset, never `null`).
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionResponse {
    pub id: String,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_code: Option<String>,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_identifier: Option<String>,
    pub client_scoped: bool,
    pub event_types: Vec<EventTypeBindingResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<String>,
    pub endpoint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue: Option<String>,
    pub custom_config: Vec<ConfigEntryResponse>,
    pub source: String,
    pub status: String,
    pub max_age_seconds: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatch_pool_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatch_pool_code: Option<String>,
    pub delay_seconds: i32,
    pub sequence: i32,
    pub mode: String,
    pub timeout_seconds: i32,
    pub max_retries: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account_id: Option<String>,
    pub data_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[schema(format = DateTime)]
    pub created_at: String,
    #[schema(format = DateTime)]
    pub updated_at: String,
}

impl From<Subscription> for SubscriptionResponse {
    fn from(s: Subscription) -> Self {
        Self {
            id: s.id.to_string(),
            code: s.code,
            application_code: s.application_code,
            name: s.name,
            description: s.description,
            client_id: s.client_id.map(ClientId::into_string),
            client_identifier: s.client_identifier,
            client_scoped: s.client_scoped,
            event_types: s.event_types.iter().map(|e| e.into()).collect(),
            connection_id: s.connection_id.map(ConnectionId::into_string),
            endpoint: s.endpoint,
            queue: s.queue,
            custom_config: s.custom_config.iter().map(|c| c.into()).collect(),
            source: s.source.as_str().to_string(),
            status: s.status.as_str().to_string(),
            max_age_seconds: s.max_age_seconds,
            dispatch_pool_id: s.dispatch_pool_id.map(DispatchPoolId::into_string),
            dispatch_pool_code: s.dispatch_pool_code,
            delay_seconds: s.delay_seconds,
            sequence: s.sequence,
            mode: s.mode.as_str().to_string(),
            timeout_seconds: s.timeout_seconds,
            max_retries: s.max_retries,
            service_account_id: s.service_account_id,
            data_only: s.data_only,
            created_by: s.created_by,
            created_at: s.created_at.to_rfc3339(),
            updated_at: s.updated_at.to_rfc3339(),
        }
    }
}

/// Subscription list response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionListResponse {
    pub subscriptions: Vec<SubscriptionResponse>,
    #[schema(value_type = i64)]
    pub total: usize,
}

/// Query parameters for subscriptions list
#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
#[into_params(parameter_in = Query)]
pub struct SubscriptionsQuery {
    /// Filter by status
    pub status: Option<String>,

    /// Filter by client ID
    pub client_id: Option<String>,

    #[serde(flatten)]
    #[param(ignore)]
    pub pagination: PaginationParams,
}

/// Subscriptions service state
#[derive(Clone)]
pub struct SubscriptionsState {
    pub subscription_repo: Arc<SubscriptionRepository>,
    pub create_use_case: Arc<CreateSubscriptionUseCase<PgUnitOfWork>>,
    pub update_use_case: Arc<UpdateSubscriptionUseCase<PgUnitOfWork>>,
    pub delete_use_case: Arc<DeleteSubscriptionUseCase<PgUnitOfWork>>,
    pub pause_use_case: Arc<PauseSubscriptionUseCase<PgUnitOfWork>>,
    pub resume_use_case: Arc<ResumeSubscriptionUseCase<PgUnitOfWork>>,
}

/// Create a new subscription
#[utoipa::path(
    post,
    path = "",
    tag = "subscriptions",
    operation_id = "createSubscription",
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
) -> Result<(StatusCode, Json<CreatedResponse>), PlatformError> {
    use crate::subscription::operations::CreateSubscriptionCommand;
    use fc_platform_core::usecase::{ExecutionContext, UseCase};

    checks::can_write_subscriptions(&auth.0)?;

    // Ruling X-01: absent or unrecognised means NEXT_ON_ERROR (with a warning
    // for the unrecognised case), never a rejection.
    let mode = Some(entity::parse_dispatch_mode(req.mode.as_deref()));

    // The use case checks, in Go's order: the input (400), then the
    // caller's reach into the requested client (403 SCOPE_FORBIDDEN; a
    // platform-wide subscription needs anchor scope), then the signers.
    let cmd = CreateSubscriptionCommand {
        code: req.code,
        name: req.name,
        description: req.description,
        client_id: req.client_id,
        endpoint: req.endpoint,
        connection_id: req.connection_id.map(ConnectionId::from_wire),
        event_types: req
            .event_types
            .into_iter()
            .map(EventTypeBindingRequest::into_input)
            .collect(),
        dispatch_pool_id: req.dispatch_pool_id.map(DispatchPoolId::from_wire),
        service_account_id: req.service_account_id,
        mode,
        max_retries: req.max_retries,
        timeout_seconds: req.timeout_seconds,
        // Go's default: absent means data only.
        data_only: req.data_only.unwrap_or(true),
        queue: req.queue,
        delay_seconds: req.delay_seconds,
        max_age_seconds: req.max_age_seconds,
        custom_config: config_entries(req.custom_config),
    };
    let ctx = ExecutionContext::from_auth(&auth.0);
    let event = state.create_use_case.run(cmd, ctx).await.into_result()?;

    Ok((
        StatusCode::CREATED,
        Json(CreatedResponse::new(event.subscription_id)),
    ))
}

/// Get subscription by ID
#[utoipa::path(
    get,
    path = "/{id}",
    tag = "subscriptions",
    operation_id = "getSubscription",
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
    let id = SubscriptionId::from_wire(id);
    checks::can_read_subscriptions(&auth.0)?;

    let subscription = state
        .subscription_repo
        .find_by_id(&id)
        .await?
        .ok_or_else(|| PlatformError::not_found("Subscription", &id))?;

    // Check client access
    access::ensure_visible(&auth.0, &subscription)?;

    Ok(Json(subscription.into()))
}

/// List subscriptions
#[utoipa::path(
    get,
    path = "",
    tag = "subscriptions",
    operation_id = "listSubscriptions",
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
    checks::can_read_subscriptions(&auth.0)?;
    let status: Option<SubscriptionStatus> = enum_str::parse_opt(query.status.as_deref())?;

    // Go: the filters as given (no status filter means every status), then
    // `FilterClientScoped` — platform subscriptions to every holder of the
    // read permission, a client's only to callers reaching that client.
    let subscriptions = state
        .subscription_repo
        .find_with_filters(
            status.map(|s| s.as_str()),
            query.client_id.as_deref().filter(|c| !c.is_empty()),
        )
        .await?;
    let filtered: Vec<SubscriptionResponse> = subscriptions
        .into_iter()
        .filter(|s| {
            s.client_id
                .as_id_str()
                .is_none_or(|cid| caller_reach::reaches_client(&auth.0, cid))
        })
        .map(|s| s.into())
        .collect();

    let total = filtered.len();
    Ok(Json(SubscriptionListResponse {
        subscriptions: filtered,
        total,
    }))
}

/// Update subscription
#[utoipa::path(
    put,
    path = "/{id}",
    tag = "subscriptions",
    operation_id = "updateSubscription",
    params(
        ("id" = String, Path, description = "Subscription ID")
    ),
    request_body = UpdateSubscriptionRequest,
    responses(
        (status = 204, description = "Subscription updated"),
        (status = 404, description = "Subscription not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_subscription(
    State(state): State<SubscriptionsState>,
    auth: Authenticated,
    Path(id): Path<String>,
    Json(req): Json<UpdateSubscriptionRequest>,
) -> Result<StatusCode, PlatformError> {
    let id = SubscriptionId::from_wire(id);
    use crate::subscription::operations::UpdateSubscriptionCommand;
    use fc_platform_core::usecase::{ExecutionContext, UseCase};

    checks::can_write_subscriptions(&auth.0)?;

    // The use case validates, loads (404) and checks the caller's scope on
    // the loaded row (403 SCOPE_FORBIDDEN), in Go's order.
    let cmd = UpdateSubscriptionCommand {
        subscription_id: id,
        name: req.name,
        description: req.description,
        endpoint: req.endpoint,
        connection_id: req.connection_id.map(ConnectionId::from_wire),
        event_types: req.event_types.map(|v| {
            v.into_iter()
                .map(EventTypeBindingRequest::into_input)
                .collect()
        }),
        dispatch_pool_id: req.dispatch_pool_id.map(DispatchPoolId::from_wire),
        service_account_id: req.service_account_id,
        // X-01: an unknown mode is NEXT_ON_ERROR, never a rejection.
        mode: req
            .mode
            .as_deref()
            .map(|m| entity::parse_dispatch_mode(Some(m))),
        max_retries: req.max_retries,
        timeout_seconds: req.timeout_seconds,
        data_only: req.data_only,
        queue: req.queue,
        delay_seconds: req.delay_seconds,
        max_age_seconds: req.max_age_seconds,
        custom_config: config_entries(req.custom_config),
    };
    let ctx = ExecutionContext::from_auth(&auth.0);
    state.update_use_case.run(cmd, ctx).await.into_result()?;

    Ok(StatusCode::NO_CONTENT)
}

/// Pause subscription
#[utoipa::path(
    post,
    path = "/{id}/pause",
    tag = "subscriptions",
    operation_id = "pauseSubscription",
    params(
        ("id" = String, Path, description = "Subscription ID")
    ),
    responses(
        (status = 204, description = "Subscription paused"),
        (status = 404, description = "Subscription not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn pause_subscription(
    State(state): State<SubscriptionsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<StatusCode, PlatformError> {
    let id = SubscriptionId::from_wire(id);
    use crate::subscription::operations::PauseSubscriptionCommand;
    use fc_platform_core::usecase::{ExecutionContext, UseCase};

    checks::can_write_subscriptions(&auth.0)?;

    // The use case answers 404 for a missing subscription, then checks the
    // caller's scope on it (Go `CheckScopeAccess`).

    let cmd = PauseSubscriptionCommand {
        subscription_id: id.clone(),
    };
    let ctx = ExecutionContext::from_auth(&auth.0);
    state.pause_use_case.run(cmd, ctx).await.into_result()?;

    // Unconditional and idempotent, as Go: 204 however often it is sent.
    Ok(StatusCode::NO_CONTENT)
}

/// Resume subscription
#[utoipa::path(
    post,
    path = "/{id}/resume",
    tag = "subscriptions",
    operation_id = "resumeSubscription",
    params(
        ("id" = String, Path, description = "Subscription ID")
    ),
    responses(
        (status = 204, description = "Subscription resumed"),
        (status = 404, description = "Subscription not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn resume_subscription(
    State(state): State<SubscriptionsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<StatusCode, PlatformError> {
    let id = SubscriptionId::from_wire(id);
    use crate::subscription::operations::ResumeSubscriptionCommand;
    use fc_platform_core::usecase::{ExecutionContext, UseCase};

    checks::can_write_subscriptions(&auth.0)?;

    // The use case answers 404 for a missing subscription, then checks the
    // caller's scope on it (Go `CheckScopeAccess`).

    let cmd = ResumeSubscriptionCommand {
        subscription_id: id.clone(),
    };
    let ctx = ExecutionContext::from_auth(&auth.0);
    state.resume_use_case.run(cmd, ctx).await.into_result()?;

    // Unconditional and idempotent, as Go: 204 however often it is sent.
    Ok(StatusCode::NO_CONTENT)
}

/// Delete subscription (archive)
#[utoipa::path(
    delete,
    path = "/{id}",
    tag = "subscriptions",
    operation_id = "deleteSubscription",
    params(
        ("id" = String, Path, description = "Subscription ID")
    ),
    responses(
        (status = 204, description = "Subscription deleted"),
        (status = 404, description = "Subscription not found")
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_subscription(
    State(state): State<SubscriptionsState>,
    auth: Authenticated,
    Path(id): Path<String>,
) -> Result<StatusCode, PlatformError> {
    let id = SubscriptionId::from_wire(id);
    use crate::subscription::operations::DeleteSubscriptionCommand;
    use fc_platform_core::usecase::{ExecutionContext, UseCase};

    checks::can_delete_subscriptions(&auth.0)?;

    // The use case answers 404 for a missing subscription, then checks the
    // caller's scope on it (Go `CheckScopeAccess`).

    let cmd = DeleteSubscriptionCommand {
        subscription_id: id,
    };
    let ctx = ExecutionContext::from_auth(&auth.0);
    state.delete_use_case.run(cmd, ctx).await.into_result()?;

    Ok(StatusCode::NO_CONTENT)
}
