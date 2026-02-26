//! FlowCatalyst Platform
//!
//! Core platform providing:
//! - Event management (CloudEvents spec)
//! - Event type definitions with schema versioning
//! - Dispatch job lifecycle management
//! - Subscription-based event routing
//! - Multi-tenant identity and access control
//! - Service account management for webhooks
//! - Use Case pattern with guaranteed audit logging
//!
//! ## Module Organization (Aggregate-based)
//!
//! Each aggregate contains:
//! - `entity` - Domain entities
//! - `repository` - Data access
//! - `api` - REST endpoints
//! - `operations` - Use case operations (where applicable)

// Core aggregates
pub mod client;
pub mod principal;
pub mod role;
pub mod application;
pub mod service_account;

// Event platform aggregates
pub mod event;
pub mod event_type;
pub mod subscription;
pub mod dispatch_pool;
pub mod dispatch_job;

// Authentication & authorization
pub mod auth;
pub mod audit;

// Shared infrastructure
pub mod shared;

// SeaORM entity models (database table mappings)
pub mod entities;

// Cross-cutting concerns
pub mod usecase;
pub mod seed;
pub mod idp;

// Re-export common types from shared
pub use shared::error::{PlatformError, Result};
pub use shared::tsid::TsidGenerator;

// Re-export use case infrastructure
pub use usecase::{
    UseCaseResult, UseCaseError, DomainEvent, ExecutionContext,
    TracingContext, UnitOfWork, MongoUnitOfWork,
};
// Note: impl_domain_event! macro is automatically exported at crate root via #[macro_export]

// Re-export main entity types for convenience
pub use client::entity::{Client, ClientStatus};
pub use principal::entity::{Principal, PrincipalType, UserScope, UserIdentity, ExternalIdentity};
pub use role::entity::{Permission, AuthRole, RoleSource, permissions};
pub use application::entity::{Application, ApplicationType};
pub use application::client_config::ApplicationClientConfig;
pub use service_account::entity::{ServiceAccount, RoleAssignment, WebhookCredentials, WebhookAuthType};
pub use event::entity::{Event, EventRead, ContextData};
pub use event_type::entity::{EventType, EventTypeStatus, SpecVersion};
pub use subscription::entity::{Subscription, SubscriptionStatus, EventTypeBinding};
pub use dispatch_pool::entity::{DispatchPool, DispatchPoolStatus};
pub use dispatch_job::entity::{DispatchJob, DispatchJobRead, DispatchStatus, DispatchMode, DispatchKind, DispatchAttempt, RetryStrategy, DispatchMetadata, ErrorType};
pub use audit::entity::{AuditLog, AuditAction};
pub use auth::config_entity::ClientAuthConfig;

// Re-export repositories
pub use client::repository::ClientRepository;
pub use principal::repository::PrincipalRepository;
pub use role::repository::RoleRepository;
pub use application::repository::ApplicationRepository;
pub use application::client_config_repository::ApplicationClientConfigRepository;
pub use service_account::repository::ServiceAccountRepository;
pub use event::repository::EventRepository;
pub use event_type::repository::EventTypeRepository;
pub use subscription::repository::SubscriptionRepository;
pub use dispatch_pool::repository::DispatchPoolRepository;
pub use dispatch_job::repository::DispatchJobRepository;
pub use audit::repository::AuditLogRepository;

// Re-export services
pub use audit::service::AuditService;
pub use auth::password_service::PasswordService;
pub use auth::auth_service::{AuthService, AccessTokenClaims};
pub use auth::oidc_service::OidcService;
pub use auth::oidc_sync_service::OidcSyncService;
pub use shared::authorization_service::{AuthorizationService, AuthContext, checks};

// Re-export auth repositories
pub use auth::config_repository::{ClientAuthConfigRepository, AnchorDomainRepository, IdpRoleMappingRepository, ClientAccessGrantRepository};
pub use auth::refresh_token_repository::RefreshTokenRepository;
pub use auth::oauth_client_repository::OAuthClientRepository;
pub use auth::authorization_code_repository::AuthorizationCodeRepository;
pub use auth::oidc_login_state_repository::OidcLoginStateRepository;

// Re-export auth entities
pub use auth::config_entity::{AnchorDomain, ClientAccessGrant, IdpRoleMapping, AuthProvider};
pub use auth::refresh_token::RefreshToken;
pub use auth::oauth_entity::OAuthClient;
pub use auth::authorization_code::AuthorizationCode;
pub use auth::oidc_login_state::OidcLoginState;

// =============================================================================
// Backward Compatibility Facades
// =============================================================================
// These modules provide backward-compatible paths for existing code.
// New code should import from the aggregate modules directly.

/// Backward-compatible repository re-exports
pub mod repository {
    pub use crate::client::repository::ClientRepository;
    pub use crate::principal::repository::PrincipalRepository;
    pub use crate::role::repository::RoleRepository;
    pub use crate::application::repository::ApplicationRepository;
    pub use crate::application::client_config_repository::ApplicationClientConfigRepository;
    pub use crate::service_account::repository::ServiceAccountRepository;
    pub use crate::event::repository::EventRepository;
    pub use crate::event_type::repository::EventTypeRepository;
    pub use crate::subscription::repository::SubscriptionRepository;
    pub use crate::dispatch_pool::repository::DispatchPoolRepository;
    pub use crate::dispatch_job::repository::DispatchJobRepository;
    pub use crate::audit::repository::AuditLogRepository;
    pub use crate::auth::config_repository::{ClientAuthConfigRepository, AnchorDomainRepository, IdpRoleMappingRepository, ClientAccessGrantRepository};
    pub use crate::auth::refresh_token_repository::RefreshTokenRepository;
    pub use crate::auth::oauth_client_repository::OAuthClientRepository;
    pub use crate::auth::authorization_code_repository::AuthorizationCodeRepository;
    pub use crate::auth::oidc_login_state_repository::OidcLoginStateRepository;
}

/// Backward-compatible service re-exports
pub mod service {
    pub use crate::audit::service::AuditService;
    pub use crate::auth::password_service::PasswordService;
    pub use crate::auth::auth_service::{AuthService, AuthConfig, AccessTokenClaims};
    pub use crate::auth::oidc_service::OidcService;
    pub use crate::auth::oidc_sync_service::OidcSyncService;
    pub use crate::shared::authorization_service::{AuthorizationService, AuthContext, checks};
    pub use crate::shared::role_sync_service::RoleSyncService;
    pub use crate::shared::projections_service::{EventProjectionWriter, DispatchJobProjectionWriter};
    pub use crate::shared::dispatch_service::{DispatchScheduler, DispatchSchedulerConfig, EventDispatcher};
}

/// Backward-compatible API re-exports
pub mod api {
    // Middleware
    pub use crate::shared::middleware::{Authenticated, AppState, AuthLayer, OptionalAuth};
    pub use crate::shared::api_common::{PaginationParams, PaginatedResponse, SuccessResponse, CreatedResponse, ApiError};

    // API state and router exports from each aggregate
    pub use crate::event::api::{events_router, EventsState};
    pub use crate::event_type::api::{event_types_router, EventTypesState};
    pub use crate::dispatch_job::api::{dispatch_jobs_router, DispatchJobsState};
    pub use crate::dispatch_pool::api::{dispatch_pools_router, DispatchPoolsState};
    pub use crate::subscription::api::{subscriptions_router, SubscriptionsState};
    pub use crate::client::api::{clients_router, ClientsState};
    pub use crate::principal::api::{principals_router, PrincipalsState};
    pub use crate::role::api::{roles_router, RolesState};
    pub use crate::application::api::{applications_router, ApplicationsState};
    pub use crate::service_account::api::{service_accounts_router, ServiceAccountsState};
    pub use crate::audit::api::{audit_logs_router, AuditLogsState};
    pub use crate::auth::oauth_clients_api::{oauth_clients_router, OAuthClientsState};
    pub use crate::auth::oauth_api::{oauth_router, OAuthState};
    pub use crate::auth::{anchor_domains_router, client_auth_configs_router, idp_role_mappings_router, AuthConfigState};
    pub use crate::auth::auth_api::{auth_router, AuthState};
    pub use crate::auth::oidc_login_api::{oidc_login_router, OidcLoginApiState};

    // Shared APIs
    pub use crate::shared::filter_options_api::{filter_options_router, event_type_filters_router, FilterOptionsState};
    pub use crate::shared::monitoring_api::{monitoring_router, MonitoringState, LeaderState, CircuitBreakerRegistry, InFlightTracker};
    pub use crate::shared::debug_api::{debug_events_router, debug_dispatch_jobs_router, DebugState};
    pub use crate::shared::health_api::health_router;
    pub use crate::shared::well_known_api::well_known_router;
    pub use crate::shared::platform_config_api::platform_config_router;

    // Re-export middleware module for direct access
    pub mod middleware {
        pub use crate::shared::middleware::*;
    }
}

/// Backward-compatible domain re-exports
pub mod domain {
    pub use crate::client::entity::{Client, ClientStatus};
    pub use crate::principal::entity::{Principal, PrincipalType, UserScope, UserIdentity, ExternalIdentity};
    pub use crate::role::entity::{Permission, AuthRole, RoleSource, permissions};
    pub use crate::application::entity::{Application, ApplicationType};
    pub use crate::application::client_config::ApplicationClientConfig;
    pub use crate::service_account::entity::{ServiceAccount, RoleAssignment, WebhookCredentials, WebhookAuthType};
    pub use crate::event::entity::{Event, EventRead, ContextData};
    pub use crate::event_type::entity::{EventType, EventTypeStatus, SpecVersion};
    pub use crate::subscription::entity::{Subscription, SubscriptionStatus, EventTypeBinding, ConfigEntry};
    pub use crate::dispatch_pool::entity::{DispatchPool, DispatchPoolStatus};
    pub use crate::dispatch_job::entity::{DispatchJob, DispatchJobRead, DispatchStatus, DispatchMode, DispatchKind, DispatchAttempt, RetryStrategy, DispatchMetadata, ErrorType};
    pub use crate::audit::entity::{AuditLog, AuditAction};
    pub use crate::auth::config_entity::{ClientAuthConfig, AnchorDomain, ClientAccessGrant, IdpRoleMapping, AuthProvider};
    pub use crate::auth::oauth_entity::OAuthClient;
    pub use crate::auth::oidc_login_state::OidcLoginState;

    // Re-export service_account module for nested imports
    pub mod service_account {
        pub use crate::service_account::entity::*;
    }
}

/// Backward-compatible operations re-exports
pub mod operations {
    // Flat re-exports for backward compatibility
    pub use crate::application::operations::{
        CreateApplicationUseCase, UpdateApplicationUseCase,
        ActivateApplicationUseCase, DeactivateApplicationUseCase,
        CreateApplicationCommand, UpdateApplicationCommand,
    };
    pub use crate::service_account::operations::{
        CreateServiceAccountUseCase, UpdateServiceAccountUseCase, DeleteServiceAccountUseCase,
        AssignRolesUseCase, RegenerateAuthTokenUseCase, RegenerateSigningSecretUseCase,
        CreateServiceAccountCommand, UpdateServiceAccountCommand, AssignRolesCommand,
    };
    pub use crate::dispatch_pool::operations::{
        CreateDispatchPoolUseCase, UpdateDispatchPoolUseCase,
        ArchiveDispatchPoolUseCase, DeleteDispatchPoolUseCase,
        CreateDispatchPoolCommand, UpdateDispatchPoolCommand,
        ArchiveDispatchPoolCommand, DeleteDispatchPoolCommand,
    };
    // Note: role, client, event_type, subscription use explicit nested modules
    // to avoid naming conflicts (events, create, update, delete modules exist in multiple)

    // Nested modules for organized access
    pub mod application {
        pub use crate::application::operations::*;
    }
    pub mod service_account {
        pub use crate::service_account::operations::*;
    }
    pub mod role {
        pub use crate::role::operations::*;
    }
    pub mod client {
        pub use crate::client::operations::*;
    }
    pub mod event_type {
        pub use crate::event_type::operations::*;
    }
    pub mod subscription {
        pub use crate::subscription::operations::*;
    }
    pub mod dispatch_pool {
        pub use crate::dispatch_pool::operations::*;
    }
}
