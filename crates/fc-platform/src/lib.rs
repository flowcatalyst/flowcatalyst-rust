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
pub mod application;
pub mod application_openapi_spec;
pub mod client;
pub mod principal;
pub mod role;
pub mod service_account;

// Event platform aggregates
pub mod dispatch_job;
pub mod function;
pub mod dispatch_pool;
pub mod event;
pub mod event_type;
pub mod process;
pub mod scheduled_job;
pub mod subscription;

// Authentication & authorization
pub mod audit;
pub mod auth;
pub mod mfa;
pub mod webauthn;

// New domains (TS alignment)
pub mod connection;
pub mod cors;
pub mod email_domain_mapping;
pub mod identity_provider;
pub mod login_attempt;
pub mod password_reset;
pub mod platform_config;

// Shared infrastructure
pub mod shared;

// Cross-cutting concerns
pub mod seed;
pub mod usecase;

// Dispatch scheduler (polls PENDING jobs → queue → router → webhook)
pub mod scheduler;

// Centralized router builder
pub mod router;

// Re-export common types from shared
pub use shared::error::{PlatformError, Result};
pub use shared::tsid::EntityType;

// Re-export use case infrastructure
pub use usecase::{
    DbTx, DomainEvent, ExecutionContext, HasId, Persist, PgUnitOfWork, UnitOfWork, UseCaseError,
    UseCaseResult,
};
// Note: impl_domain_event! macro is automatically exported at crate root via #[macro_export]

// Re-export main entity types for convenience
pub use application::client_config::ApplicationClientConfig;
pub use application::entity::{Application, ApplicationType};
pub use application_openapi_spec::entity::{ChangeNotes, OpenApiSpec, OpenApiSpecStatus};
pub use application_openapi_spec::repository::OpenApiSpecRepository;
pub use audit::entity::AuditLog;
pub use auth::config_entity::ClientAuthConfig;
pub use client::entity::{Client, ClientStatus};
pub use connection::entity::{Connection, ConnectionStatus};
pub use cors::entity::CorsAllowedOrigin;
pub use dispatch_job::entity::{
    DispatchAttempt, DispatchJob, DispatchJobRead, DispatchKind, DispatchMetadata, DispatchMode,
    DispatchStatus, ErrorType, RetryStrategy,
};
pub use dispatch_pool::entity::{DispatchPool, DispatchPoolStatus};
pub use email_domain_mapping::entity::{EmailDomainMapping, ScopeType};
pub use event::entity::{ContextData, Event, EventRead};
pub use event_type::entity::{EventType, EventTypeStatus, SpecVersion};
pub use identity_provider::entity::{IdentityProvider, IdentityProviderType};
pub use login_attempt::entity::{AttemptType, LoginAttempt, LoginOutcome};
pub use password_reset::entity::PasswordResetToken;
pub use platform_config::access_entity::PlatformConfigAccess;
pub use platform_config::entity::{ConfigScope, ConfigValueType, PlatformConfig};
pub use principal::entity::{ExternalIdentity, Principal, PrincipalType, UserIdentity, UserScope};
pub use process::entity::{Process, ProcessSource, ProcessStatus};
pub use role::entity::{permissions, AuthRole, Permission, RoleSource};
pub use scheduled_job::entity::{
    CompletionStatus, InstanceStatus, LogLevel, ScheduledJob, ScheduledJobInstance,
    ScheduledJobInstanceLog, ScheduledJobStatus, TriggerKind,
};
pub use service_account::entity::{
    AssignmentSource, RoleAssignment, ServiceAccount, SigningAlgorithm, WebhookAuthType,
    WebhookCredentials,
};
pub use subscription::entity::{EventTypeBinding, Subscription, SubscriptionStatus};

// Re-export repositories
pub use application::client_config_repository::ApplicationClientConfigRepository;
pub use application::repository::ApplicationRepository;
pub use audit::repository::AuditLogRepository;
pub use client::repository::ClientRepository;
pub use connection::repository::ConnectionRepository;
pub use cors::repository::CorsOriginRepository;
pub use dispatch_job::repository::DispatchJobRepository;
pub use dispatch_pool::repository::DispatchPoolRepository;
pub use email_domain_mapping::repository::EmailDomainMappingRepository;
pub use event::repository::EventRepository;
pub use event_type::repository::EventTypeRepository;
pub use identity_provider::repository::IdentityProviderRepository;
pub use login_attempt::repository::LoginAttemptRepository;
pub use password_reset::repository::PasswordResetTokenRepository;
pub use platform_config::access_repository::PlatformConfigAccessRepository;
pub use platform_config::repository::PlatformConfigRepository;
pub use principal::repository::PrincipalRepository;
pub use process::repository::ProcessRepository;
pub use role::repository::RoleRepository;
pub use scheduled_job::instance_repository::{InstanceListFilters, ScheduledJobInstanceRepository};
pub use scheduled_job::repository::ScheduledJobRepository;
pub use service_account::repository::ServiceAccountRepository;
pub use subscription::repository::SubscriptionRepository;

// Re-export services
pub use audit::service::AuditService;
pub use auth::auth_service::{AccessTokenClaims, AuthService, IdTokenClaims};
pub use auth::oidc_sync_service::OidcSyncService;
pub use auth::password_service::PasswordService;
pub use shared::authorization_service::{checks, AuthContext, AuthorizationService};

// Re-export auth repositories
pub use auth::authorization_code_repository::AuthorizationCodeRepository;
pub use auth::config_repository::{
    AnchorDomainRepository, ClientAccessGrantRepository, ClientAuthConfigRepository,
    IdpRoleMappingRepository,
};
pub use auth::oauth_client_repository::OAuthClientRepository;
pub use auth::oidc_login_state_repository::OidcLoginStateRepository;
pub use auth::pending_auth_repository::PendingAuthRepository;
pub use auth::refresh_token_repository::RefreshTokenRepository;

// Re-export auth entities
pub use auth::authorization_code::AuthorizationCode;
pub use auth::config_entity::{AnchorDomain, AuthProvider, IdpRoleMapping};
pub use auth::oauth_entity::OAuthClient;
pub use auth::oidc_login_state::OidcLoginState;
pub use auth::refresh_token::RefreshToken;
pub use principal::entity::ClientAccessGrant;

// =============================================================================
// Backward Compatibility Facades
// =============================================================================
// These modules provide backward-compatible paths for existing code.
// New code should import from the aggregate modules directly.

/// Backward-compatible repository re-exports
pub mod repository {
    pub use crate::application::client_config_repository::ApplicationClientConfigRepository;
    pub use crate::application::repository::ApplicationRepository;
    pub use crate::audit::repository::AuditLogRepository;
    pub use crate::auth::authorization_code_repository::AuthorizationCodeRepository;
    pub use crate::auth::config_repository::{
        AnchorDomainRepository, ClientAccessGrantRepository, ClientAuthConfigRepository,
        IdpRoleMappingRepository,
    };
    pub use crate::auth::oauth_client_repository::OAuthClientRepository;
    pub use crate::auth::oidc_login_state_repository::OidcLoginStateRepository;
    pub use crate::auth::pending_auth_repository::PendingAuthRepository;
    pub use crate::auth::refresh_token_repository::RefreshTokenRepository;
    pub use crate::client::repository::ClientRepository;
    pub use crate::connection::repository::ConnectionRepository;
    pub use crate::cors::repository::CorsOriginRepository;
    pub use crate::dispatch_job::repository::DispatchJobRepository;
    pub use crate::dispatch_pool::repository::DispatchPoolRepository;
    pub use crate::email_domain_mapping::repository::EmailDomainMappingRepository;
    pub use crate::event::repository::EventRepository;
    pub use crate::event_type::repository::EventTypeRepository;
    pub use crate::identity_provider::repository::IdentityProviderRepository;
    pub use crate::login_attempt::repository::LoginAttemptRepository;
    pub use crate::password_reset::repository::PasswordResetTokenRepository;
    pub use crate::platform_config::access_repository::PlatformConfigAccessRepository;
    pub use crate::platform_config::repository::PlatformConfigRepository;
    pub use crate::principal::repository::PrincipalRepository;
    pub use crate::process::repository::ProcessRepository;
    pub use crate::role::repository::RoleRepository;
    pub use crate::scheduled_job::instance_repository::ScheduledJobInstanceRepository;
    pub use crate::scheduled_job::repository::ScheduledJobRepository;
    pub use crate::service_account::repository::ServiceAccountRepository;
    pub use crate::subscription::repository::SubscriptionRepository;

    use sqlx::PgPool;
    use std::sync::Arc;

    /// Holds all Arc-wrapped repository instances. Replaces the ~30 lines of
    /// `Arc::new(XRepository::new(&pool))` duplicated across binaries.
    ///
    /// ```rust,ignore
    /// let repos = Repositories::new(&pool);
    /// // then use repos.event_repo, repos.client_repo, etc.
    /// ```
    pub struct Repositories {
        pub event_repo: Arc<EventRepository>,
        pub dispatch_job_repo: Arc<DispatchJobRepository>,
        pub scheduled_job_repo: Arc<ScheduledJobRepository>,
        pub scheduled_job_instance_repo: Arc<ScheduledJobInstanceRepository>,
        pub event_type_repo: Arc<EventTypeRepository>,
        pub process_repo: Arc<ProcessRepository>,
        pub role_repo: Arc<RoleRepository>,
        pub service_account_repo: Arc<ServiceAccountRepository>,
        pub dispatch_pool_repo: Arc<DispatchPoolRepository>,
        pub subscription_repo: Arc<SubscriptionRepository>,
        pub principal_repo: Arc<PrincipalRepository>,
        pub client_repo: Arc<ClientRepository>,
        pub application_repo: Arc<ApplicationRepository>,
        pub oauth_client_repo: Arc<OAuthClientRepository>,
        pub anchor_domain_repo: Arc<AnchorDomainRepository>,
        pub client_auth_config_repo: Arc<ClientAuthConfigRepository>,
        pub client_access_grant_repo: Arc<ClientAccessGrantRepository>,
        pub idp_role_mapping_repo: Arc<IdpRoleMappingRepository>,
        pub audit_log_repo: Arc<AuditLogRepository>,
        pub application_client_config_repo: Arc<ApplicationClientConfigRepository>,
        pub oidc_login_state_repo: Arc<OidcLoginStateRepository>,
        pub refresh_token_repo: Arc<RefreshTokenRepository>,
        pub auth_code_repo: Arc<AuthorizationCodeRepository>,
        pub connection_repo: Arc<ConnectionRepository>,
        pub cors_repo: Arc<CorsOriginRepository>,
        pub idp_repo: Arc<IdentityProviderRepository>,
        pub edm_repo: Arc<EmailDomainMappingRepository>,
        pub platform_config_repo: Arc<PlatformConfigRepository>,
        pub platform_config_access_repo: Arc<PlatformConfigAccessRepository>,
        pub login_attempt_repo: Arc<LoginAttemptRepository>,
        pub password_reset_repo: Arc<PasswordResetTokenRepository>,
        pub pending_auth_repo: Arc<PendingAuthRepository>,
        // The function registry. Its settings repository needs the app key,
        // so the route setup builds that one itself.
        pub function_repo: Arc<crate::function::repository::FunctionRepository>,
        pub function_version_repo:
            Arc<crate::function::version_repository::FunctionVersionRepository>,
        pub function_host_repo: Arc<crate::function::host_repository::FunctionHostRepository>,
        pub function_policy_repo: Arc<crate::function::policy_repository::ClientPolicyRepository>,
        pub function_domain_repo: Arc<crate::function::domain_repository::FunctionDomainRepository>,
        pub function_route_repo: Arc<crate::function::route_repository::FunctionRouteRepository>,
        pub function_trigger_object_repo:
            Arc<crate::function::trigger_object_repository::TriggerObjectRepository>,
        /// Raw pool — exposed so callers (e.g. the BFF dashboard stats
        /// endpoint) can run ad-hoc queries that don't fit a single
        /// repository. Cloning is cheap; sqlx already Arcs internally.
        pub pool: PgPool,
    }

    impl Repositories {
        pub fn new(pool: &PgPool) -> Self {
            Self {
                event_repo: Arc::new(EventRepository::new(pool)),
                dispatch_job_repo: Arc::new(DispatchJobRepository::new(pool)),
                scheduled_job_repo: Arc::new(ScheduledJobRepository::new(pool)),
                scheduled_job_instance_repo: Arc::new(ScheduledJobInstanceRepository::new(pool)),
                cors_repo: Arc::new(CorsOriginRepository::new(pool)),
                password_reset_repo: Arc::new(PasswordResetTokenRepository::new(pool)),
                platform_config_access_repo: Arc::new(PlatformConfigAccessRepository::new(pool)),
                login_attempt_repo: Arc::new(LoginAttemptRepository::new(pool)),
                platform_config_repo: Arc::new(PlatformConfigRepository::new(pool)),
                audit_log_repo: Arc::new(AuditLogRepository::new(pool)),
                connection_repo: Arc::new(ConnectionRepository::new(pool)),
                dispatch_pool_repo: Arc::new(DispatchPoolRepository::new(pool)),
                client_repo: Arc::new(ClientRepository::new(pool)),
                application_repo: Arc::new(ApplicationRepository::new(pool)),
                application_client_config_repo: Arc::new(ApplicationClientConfigRepository::new(
                    pool,
                )),
                event_type_repo: Arc::new(EventTypeRepository::new(pool)),
                process_repo: Arc::new(ProcessRepository::new(pool)),
                role_repo: Arc::new(RoleRepository::new(pool)),
                service_account_repo: Arc::new(ServiceAccountRepository::new(pool)),
                subscription_repo: Arc::new(SubscriptionRepository::new(pool)),
                principal_repo: Arc::new(PrincipalRepository::new(pool)),
                anchor_domain_repo: Arc::new(AnchorDomainRepository::new(pool)),
                client_auth_config_repo: Arc::new(ClientAuthConfigRepository::new(pool)),
                client_access_grant_repo: Arc::new(ClientAccessGrantRepository::new(pool)),
                idp_role_mapping_repo: Arc::new(IdpRoleMappingRepository::new(pool)),
                oauth_client_repo: Arc::new(OAuthClientRepository::new(pool)),
                oidc_login_state_repo: Arc::new(OidcLoginStateRepository::new(pool)),
                refresh_token_repo: Arc::new(RefreshTokenRepository::new(pool)),
                auth_code_repo: Arc::new(AuthorizationCodeRepository::new(pool)),
                idp_repo: Arc::new(IdentityProviderRepository::new(pool)),
                edm_repo: Arc::new(EmailDomainMappingRepository::new(pool)),
                pending_auth_repo: Arc::new(PendingAuthRepository::new(pool)),
                function_repo: Arc::new(crate::function::repository::FunctionRepository::new(pool)),
                function_version_repo: Arc::new(
                    crate::function::version_repository::FunctionVersionRepository::new(pool),
                ),
                function_host_repo: Arc::new(
                    crate::function::host_repository::FunctionHostRepository::new(pool),
                ),
                function_policy_repo: Arc::new(
                    crate::function::policy_repository::ClientPolicyRepository::new(pool),
                ),
                function_domain_repo: Arc::new(
                    crate::function::domain_repository::FunctionDomainRepository::new(pool),
                ),
                function_route_repo: Arc::new(
                    crate::function::route_repository::FunctionRouteRepository::new(pool),
                ),
                function_trigger_object_repo: Arc::new(
                    crate::function::trigger_object_repository::TriggerObjectRepository::new(pool),
                ),
                pool: pool.clone(),
            }
        }
    }
}

/// Backward-compatible service re-exports
pub mod service {
    pub use crate::audit::service::AuditService;
    pub use crate::auth::auth_service::{
        AccessTokenClaims, AuthConfig, AuthService, IdTokenClaims,
    };
    pub use crate::auth::oidc_sync_service::OidcSyncService;
    pub use crate::auth::password_service::PasswordService;
    pub use crate::scheduler::{DispatchScheduler, SchedulerConfig, SchedulerError};
    pub use crate::shared::authorization_service::{checks, AuthContext, AuthorizationService};
    pub use crate::shared::projections_service::{
        DispatchJobProjectionWriter, EventProjectionWriter,
    };
    pub use crate::shared::role_sync_service::RoleSyncService;
}

/// Backward-compatible API re-exports
pub mod api {
    // Middleware
    pub use crate::shared::api_common::{
        ApiError, CreatedResponse, PaginatedResponse, PaginationParams, SuccessResponse,
    };
    pub use crate::shared::middleware::{AppState, AuthLayer, Authenticated, OptionalAuth};

    // API state and router exports from each aggregate
    pub use crate::application::api::{applications_router, ApplicationsState};
    pub use crate::audit::api::{audit_logs_router, AuditLogsState};
    pub use crate::auth::auth_api::{auth_router, AuthState};
    pub use crate::auth::oauth_api::{oauth_router, OAuthState};
    pub use crate::auth::oauth_clients_api::{oauth_clients_router, OAuthClientsState};
    pub use crate::auth::oidc_login_api::{oidc_login_router, OidcLoginApiState};
    pub use crate::auth::password_reset_api::{password_reset_router, PasswordResetApiState};
    pub use crate::auth::{
        anchor_domains_router, client_auth_configs_router, idp_role_mappings_router,
        AuthConfigState,
    };
    pub use crate::client::api::{clients_router, ClientsState};
    pub use crate::dispatch_job::api::{
        dispatch_jobs_api_router, dispatch_jobs_router, DispatchJobsState,
    };
    pub use crate::dispatch_pool::api::{dispatch_pools_router, DispatchPoolsState};
    pub use crate::event::api::{events_api_router, events_router, EventsState};
    pub use crate::event_type::api::{event_types_router, EventTypesState};
    pub use crate::principal::api::{principals_router, PrincipalsState};
    pub use crate::process::api::{processes_router, ProcessesState};
    pub use crate::role::api::{roles_router, RolesState};
    pub use crate::scheduled_job::api::{scheduled_jobs_router, ScheduledJobsState};
    pub use crate::service_account::api::{service_accounts_router, ServiceAccountsState};
    pub use crate::subscription::api::{subscriptions_router, SubscriptionsState};

    // New domain APIs
    pub use crate::connection::api::{connections_router, ConnectionsState};
    pub use crate::cors::api::{cors_router, CorsState};
    pub use crate::email_domain_mapping::api::{
        email_domain_mappings_router, EmailDomainMappingsState,
    };
    pub use crate::identity_provider::api::{identity_providers_router, IdentityProvidersState};
    pub use crate::login_attempt::api::{login_attempts_router, LoginAttemptsState};
    pub use crate::platform_config::access_api::{config_access_router, ConfigAccessState};
    pub use crate::platform_config::api::{admin_platform_config_router, PlatformConfigState};
    pub use crate::shared::batch_api::{sdk_events_batch_router, SdkEventsState};
    pub use crate::shared::bff_dashboard_api::{bff_dashboard_router, BffDashboardState};
    pub use crate::shared::bff_event_types_api::{bff_event_types_router, BffEventTypesState};
    pub use crate::shared::bff_roles_api::{bff_roles_router, BffRolesState};
    pub use crate::shared::bff_scheduled_jobs_api::{
        bff_scheduled_jobs_router, BffScheduledJobsState,
    };
    pub use crate::shared::dispatch_process_api::{dispatch_process_router, DispatchProcessState};
    pub use crate::shared::me_api::{me_router, MeState};
    pub use crate::shared::public_api::{public_router, PublicApiState};
    pub use crate::shared::sdk_audit_batch_api::{sdk_audit_batch_router, SdkAuditBatchState};
    pub use crate::shared::sdk_dispatch_jobs_api::{
        sdk_dispatch_jobs_batch_router, SdkDispatchJobsState,
    };
    pub use crate::shared::sdk_sync_api::{sdk_sync_router, SdkSyncState};

    // Shared APIs
    pub use crate::shared::application_roles_sdk_api::{
        application_roles_sdk_router, ApplicationRolesSdkState,
    };
    pub use crate::shared::client_selection_api::{client_selection_router, ClientSelectionState};
    pub use crate::shared::debug_api::{
        debug_dispatch_jobs_router, debug_events_router, DebugState,
    };
    pub use crate::shared::filter_options_api::{
        event_type_filters_router, filter_options_router, FilterOptionsState,
    };
    pub use crate::shared::health_api::health_router;
    pub use crate::shared::monitoring_api::{
        monitoring_router, CircuitBreakerRegistry, InFlightTracker, LeaderState, MonitoringState,
    };
    pub use crate::shared::platform_config_api::platform_config_router;
    pub use crate::shared::well_known_api::{well_known_router, WellKnownState};

    // Centralized router builder
    pub use crate::router::PlatformRoutes;

    // Re-export middleware module for direct access
    pub mod middleware {
        pub use crate::shared::middleware::*;
    }
}

/// Backward-compatible domain re-exports
pub mod domain {
    pub use crate::application::client_config::ApplicationClientConfig;
    pub use crate::application::entity::{Application, ApplicationType};
    pub use crate::audit::entity::AuditLog;
    pub use crate::auth::config_entity::{
        AnchorDomain, AuthProvider, ClientAuthConfig, IdpRoleMapping,
    };
    pub use crate::auth::oauth_entity::OAuthClient;
    pub use crate::auth::oidc_login_state::OidcLoginState;
    pub use crate::client::entity::{Client, ClientStatus};
    pub use crate::connection::entity::{Connection, ConnectionStatus};
    pub use crate::cors::entity::CorsAllowedOrigin;
    pub use crate::dispatch_job::entity::{
        DispatchAttempt, DispatchJob, DispatchJobRead, DispatchKind, DispatchMetadata,
        DispatchMode, DispatchStatus, ErrorType, RetryStrategy,
    };
    pub use crate::dispatch_pool::entity::{DispatchPool, DispatchPoolStatus};
    pub use crate::email_domain_mapping::entity::{EmailDomainMapping, ScopeType};
    pub use crate::event::entity::{ContextData, Event, EventRead};
    pub use crate::event_type::entity::{EventType, EventTypeStatus, SpecVersion};
    pub use crate::identity_provider::entity::{IdentityProvider, IdentityProviderType};
    pub use crate::login_attempt::entity::{AttemptType, LoginAttempt, LoginOutcome};
    pub use crate::password_reset::entity::PasswordResetToken;
    pub use crate::platform_config::access_entity::PlatformConfigAccess;
    pub use crate::platform_config::entity::{ConfigScope, ConfigValueType, PlatformConfig};
    pub use crate::principal::entity::ClientAccessGrant;
    pub use crate::principal::entity::{
        ExternalIdentity, Principal, PrincipalType, UserIdentity, UserScope,
    };
    pub use crate::role::entity::{permissions, AuthRole, Permission, RoleSource};
    pub use crate::service_account::entity::{
        AssignmentSource, RoleAssignment, ServiceAccount, SigningAlgorithm, WebhookAuthType,
        WebhookCredentials,
    };
    pub use crate::subscription::entity::{
        ConfigEntry, EventTypeBinding, Subscription, SubscriptionStatus,
    };

    // Re-export service_account module for nested imports
    pub mod service_account {
        pub use crate::service_account::entity::*;
    }
}

/// Backward-compatible operations re-exports
pub mod operations {
    // Flat re-exports for backward compatibility
    pub use crate::application::operations::{
        ActivateApplicationUseCase, CreateApplicationCommand, CreateApplicationUseCase,
        DeactivateApplicationUseCase, UpdateApplicationCommand, UpdateApplicationUseCase,
    };
    pub use crate::dispatch_pool::operations::{
        ArchiveDispatchPoolCommand, ArchiveDispatchPoolUseCase, CreateDispatchPoolCommand,
        CreateDispatchPoolUseCase, DeleteDispatchPoolCommand, DeleteDispatchPoolUseCase,
        UpdateDispatchPoolCommand, UpdateDispatchPoolUseCase,
    };
    pub use crate::service_account::operations::{
        AssignRolesCommand, AssignRolesUseCase, CreateServiceAccountCommand,
        CreateServiceAccountUseCase, DeleteServiceAccountUseCase, RegenerateAuthTokenUseCase,
        RegenerateSigningSecretUseCase, UpdateServiceAccountCommand, UpdateServiceAccountUseCase,
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
