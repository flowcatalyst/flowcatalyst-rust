//! Every platform enum's `str_enum!` spellings, against serde and against
//! the values production stores (`shared::enum_str`, fc-platform-core; the
//! enums are the domain crates').

use crate::application::entity::ApplicationType;
use crate::application_openapi_spec::entity::OpenApiSpecStatus;
use crate::auth::config_entity::AuthConfigType;
use crate::auth::config_entity::AuthProvider;
use crate::auth::oauth_entity::GrantType;
use crate::auth::oauth_entity::OAuthClientType;
use crate::client::entity::ClientStatus;
use crate::connection::entity::ConnectionStatus;
use crate::dispatch_job::entity::DispatchAttemptStatus;
use crate::dispatch_job::entity::DispatchKind;
use crate::dispatch_job::entity::DispatchProtocol;
use crate::dispatch_job::entity::ErrorType;
use crate::dispatch_job::entity::RetryStrategy;
use crate::dispatch_pool::entity::DispatchPoolStatus;
use crate::email_domain_mapping::entity::ScopeType;
use crate::event_type::entity::EventTypeSource;
use crate::event_type::entity::EventTypeStatus;
use crate::event_type::entity::SchemaType;
use crate::event_type::entity::SpecVersionStatus;
use crate::identity_provider::entity::IdentityProviderType;
use crate::login_attempt::entity::AttemptType;
use crate::login_attempt::entity::LoginOutcome;
use crate::platform_config::entity::ConfigScope;
use crate::platform_config::entity::ConfigValueType;
use crate::portal::entity::{IdentitySource, IdentityStatus};
use crate::principal::entity::PrincipalType;
use crate::principal::entity::UserScope;
use crate::process::entity::ProcessSource;
use crate::process::entity::ProcessStatus;
use crate::role::entity::RoleSource;
use crate::scheduled_job::entity::CompletionStatus;
use crate::scheduled_job::entity::InstanceStatus;
use crate::scheduled_job::entity::LogLevel;
use crate::scheduled_job::entity::ScheduledJobStatus;
use crate::scheduled_job::entity::TriggerKind;
use crate::service_account::entity::AssignmentSource;
use crate::service_account::entity::SigningAlgorithm;
use crate::service_account::entity::WebhookAuthType;
use crate::subscription::entity::SubscriptionSource;
use crate::subscription::entity::SubscriptionStatus;
use fc_platform_core::shared::enum_str::{assert_str_enum, UnknownEnumValue};
use std::str::FromStr;

/// Every domain enum's `as_str` agrees with its serde spelling, so typed
/// fields and hand-built strings put the same value on the wire.
#[test]
fn every_domain_enum_agrees_with_serde() {
    assert_str_enum(PrincipalType::ALL, PrincipalType::as_str);
    assert_str_enum(UserScope::ALL, UserScope::as_str);
    assert_str_enum(EventTypeStatus::ALL, EventTypeStatus::as_str);
    assert_str_enum(EventTypeSource::ALL, EventTypeSource::as_str);
    assert_str_enum(SpecVersionStatus::ALL, SpecVersionStatus::as_str);
    assert_str_enum(SchemaType::ALL, SchemaType::as_str);
    assert_str_enum(RoleSource::ALL, RoleSource::as_str);
    assert_str_enum(ConnectionStatus::ALL, ConnectionStatus::as_str);
    assert_str_enum(DispatchKind::ALL, DispatchKind::as_str);
    assert_str_enum(DispatchProtocol::ALL, DispatchProtocol::as_str);
    assert_str_enum(RetryStrategy::ALL, RetryStrategy::as_str);
    assert_str_enum(ErrorType::ALL, ErrorType::as_str);
    assert_str_enum(DispatchAttemptStatus::ALL, DispatchAttemptStatus::as_str);
    assert_str_enum(AuthProvider::ALL, AuthProvider::as_str);
    assert_str_enum(AuthConfigType::ALL, AuthConfigType::as_str);
    assert_str_enum(OAuthClientType::ALL, OAuthClientType::as_str);
    assert_str_enum(GrantType::ALL, GrantType::as_str);
    assert_str_enum(IdentityProviderType::ALL, IdentityProviderType::as_str);
    assert_str_enum(ScopeType::ALL, ScopeType::as_str);
    assert_str_enum(SubscriptionStatus::ALL, SubscriptionStatus::as_str);
    assert_str_enum(SubscriptionSource::ALL, SubscriptionSource::as_str);
    assert_str_enum(ScheduledJobStatus::ALL, ScheduledJobStatus::as_str);
    assert_str_enum(TriggerKind::ALL, TriggerKind::as_str);
    assert_str_enum(InstanceStatus::ALL, InstanceStatus::as_str);
    assert_str_enum(CompletionStatus::ALL, CompletionStatus::as_str);
    assert_str_enum(LogLevel::ALL, LogLevel::as_str);
    assert_str_enum(DispatchPoolStatus::ALL, DispatchPoolStatus::as_str);
    assert_str_enum(ConfigScope::ALL, ConfigScope::as_str);
    assert_str_enum(ConfigValueType::ALL, ConfigValueType::as_str);
    assert_str_enum(WebhookAuthType::ALL, WebhookAuthType::as_str);
    assert_str_enum(SigningAlgorithm::ALL, SigningAlgorithm::as_str);
    assert_str_enum(AssignmentSource::ALL, AssignmentSource::as_str);
    assert_str_enum(ApplicationType::ALL, ApplicationType::as_str);
    assert_str_enum(OpenApiSpecStatus::ALL, OpenApiSpecStatus::as_str);
    assert_str_enum(AttemptType::ALL, AttemptType::as_str);
    assert_str_enum(LoginOutcome::ALL, LoginOutcome::as_str);
    assert_str_enum(ClientStatus::ALL, ClientStatus::as_str);
    assert_str_enum(ProcessStatus::ALL, ProcessStatus::as_str);
    assert_str_enum(ProcessSource::ALL, ProcessSource::as_str);
    assert_str_enum(IdentityStatus::ALL, IdentityStatus::as_str);
    assert_str_enum(IdentitySource::ALL, IdentitySource::as_str);
}

/// Every value the production database held at the 2026-09-24 audit
/// decodes under the enum its column is read as. A failure here means a
/// deploy would turn live rows into read errors.
#[test]
fn production_stored_values_all_decode() {
    fn all<T: FromStr<Err = UnknownEnumValue>>(values: &[&str]) {
        for v in values {
            assert!(v.parse::<T>().is_ok(), "{v} must decode");
        }
    }
    use crate::application::entity::ApplicationType;
    use crate::application_openapi_spec::entity::OpenApiSpecStatus;
    use crate::auth::oauth_entity::{GrantType, OAuthClientType};
    use crate::client::entity::ClientStatus;
    use crate::connection::entity::ConnectionStatus;
    use crate::dispatch_job::entity::{
        parse_dispatch_status, DispatchAttemptStatus, DispatchKind, DispatchProtocol, ErrorType,
        RetryStrategy,
    };
    use crate::dispatch_pool::entity::DispatchPoolStatus;
    use crate::email_domain_mapping::entity::ScopeType;
    use crate::event_type::entity::{
        EventTypeSource, EventTypeStatus, SchemaType, SpecVersionStatus,
    };
    use crate::identity_provider::entity::IdentityProviderType;
    use crate::login_attempt::entity::{AttemptType, LoginOutcome};
    use crate::platform_config::entity::{ConfigScope, ConfigValueType};
    use crate::principal::entity::{PrincipalType, UserScope};
    use crate::process::entity::{ProcessSource, ProcessStatus};
    use crate::role::entity::RoleSource;
    use crate::service_account::entity::{AssignmentSource, SigningAlgorithm, WebhookAuthType};
    use crate::subscription::entity::{SubscriptionSource, SubscriptionStatus};

    all::<OpenApiSpecStatus>(&["ARCHIVED", "CURRENT"]);
    all::<ApplicationType>(&["APPLICATION"]);
    all::<ConfigScope>(&["GLOBAL", "CLIENT"]);
    all::<ConfigValueType>(&["PLAIN"]);
    all::<AttemptType>(&["SERVICE_ACCOUNT_TOKEN", "USER_LOGIN"]);
    all::<LoginOutcome>(&["SUCCESS", "FAILURE"]);
    all::<AssignmentSource>(&["ADMIN_ASSIGNED", "PROVISIONED"]);
    all::<IdentityProviderType>(&["OIDC", "INTERNAL"]);
    all::<UserScope>(&["CLIENT", "ANCHOR", "PARTNER"]);
    all::<PrincipalType>(&["USER", "SERVICE"]);
    all::<RoleSource>(&["SDK", "CODE", "DATABASE"]);
    all::<WebhookAuthType>(&["BEARER_TOKEN"]);
    all::<SigningAlgorithm>(&["HMAC_SHA256"]);
    all::<ConnectionStatus>(&["ACTIVE"]);
    all::<ErrorType>(&["HTTP_ERROR"]);
    all::<DispatchAttemptStatus>(&["SUCCESS", "FAILURE"]);
    all::<DispatchKind>(&["EVENT"]);
    all::<DispatchProtocol>(&["HTTP_WEBHOOK"]);
    all::<RetryStrategy>(&["exponential"]);
    assert!(parse_dispatch_status("COMPLETED").is_ok());
    all::<DispatchPoolStatus>(&["ACTIVE"]);
    all::<SchemaType>(&["JSON_SCHEMA"]);
    all::<SpecVersionStatus>(&["FINALISING", "CURRENT"]);
    all::<EventTypeSource>(&["API", "UI"]);
    all::<EventTypeStatus>(&["CURRENT"]);
    all::<ProcessSource>(&["API", "CODE"]);
    all::<ProcessStatus>(&["CURRENT"]);
    all::<SubscriptionSource>(&["UI"]);
    all::<SubscriptionStatus>(&["ACTIVE"]);
    all::<GrantType>(&["client_credentials", "refresh_token", "authorization_code"]);
    all::<OAuthClientType>(&["CONFIDENTIAL", "PUBLIC"]);
    all::<ClientStatus>(&["ACTIVE"]);
    all::<ScopeType>(&["CLIENT", "ANCHOR"]);
}
