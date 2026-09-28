//! Every platform enum's `str_enum!` spellings, against serde and against
//! the values production stores (`shared::enum_str`, fc-platform-core; the
//! enums are the domain crates').

use fc_platform_core::shared::enum_str::{assert_str_enum, UnknownEnumValue};

/// Every domain enum's `as_str` agrees with its serde spelling, so typed
/// fields and hand-built strings put the same value on the wire.
#[test]
fn every_domain_enum_agrees_with_serde() {
    assert_str_enum(
        crate::principal::entity::PrincipalType::ALL,
        crate::principal::entity::PrincipalType::as_str,
    );
    assert_str_enum(
        crate::principal::entity::UserScope::ALL,
        crate::principal::entity::UserScope::as_str,
    );
    assert_str_enum(
        crate::event_type::entity::EventTypeStatus::ALL,
        crate::event_type::entity::EventTypeStatus::as_str,
    );
    assert_str_enum(
        crate::event_type::entity::EventTypeSource::ALL,
        crate::event_type::entity::EventTypeSource::as_str,
    );
    assert_str_enum(
        crate::event_type::entity::SpecVersionStatus::ALL,
        crate::event_type::entity::SpecVersionStatus::as_str,
    );
    assert_str_enum(
        crate::event_type::entity::SchemaType::ALL,
        crate::event_type::entity::SchemaType::as_str,
    );
    assert_str_enum(
        crate::role::entity::RoleSource::ALL,
        crate::role::entity::RoleSource::as_str,
    );
    assert_str_enum(
        crate::connection::entity::ConnectionStatus::ALL,
        crate::connection::entity::ConnectionStatus::as_str,
    );
    assert_str_enum(
        crate::dispatch_job::entity::DispatchKind::ALL,
        crate::dispatch_job::entity::DispatchKind::as_str,
    );
    assert_str_enum(
        crate::dispatch_job::entity::DispatchProtocol::ALL,
        crate::dispatch_job::entity::DispatchProtocol::as_str,
    );
    assert_str_enum(
        crate::dispatch_job::entity::RetryStrategy::ALL,
        crate::dispatch_job::entity::RetryStrategy::as_str,
    );
    assert_str_enum(
        crate::dispatch_job::entity::ErrorType::ALL,
        crate::dispatch_job::entity::ErrorType::as_str,
    );
    assert_str_enum(
        crate::dispatch_job::entity::DispatchAttemptStatus::ALL,
        crate::dispatch_job::entity::DispatchAttemptStatus::as_str,
    );
    assert_str_enum(
        crate::auth::config_entity::AuthProvider::ALL,
        crate::auth::config_entity::AuthProvider::as_str,
    );
    assert_str_enum(
        crate::auth::config_entity::AuthConfigType::ALL,
        crate::auth::config_entity::AuthConfigType::as_str,
    );
    assert_str_enum(
        crate::auth::oauth_entity::OAuthClientType::ALL,
        crate::auth::oauth_entity::OAuthClientType::as_str,
    );
    assert_str_enum(
        crate::auth::oauth_entity::GrantType::ALL,
        crate::auth::oauth_entity::GrantType::as_str,
    );
    assert_str_enum(
        crate::identity_provider::entity::IdentityProviderType::ALL,
        crate::identity_provider::entity::IdentityProviderType::as_str,
    );
    assert_str_enum(
        crate::email_domain_mapping::entity::ScopeType::ALL,
        crate::email_domain_mapping::entity::ScopeType::as_str,
    );
    assert_str_enum(
        crate::subscription::entity::SubscriptionStatus::ALL,
        crate::subscription::entity::SubscriptionStatus::as_str,
    );
    assert_str_enum(
        crate::subscription::entity::SubscriptionSource::ALL,
        crate::subscription::entity::SubscriptionSource::as_str,
    );
    assert_str_enum(
        crate::scheduled_job::entity::ScheduledJobStatus::ALL,
        crate::scheduled_job::entity::ScheduledJobStatus::as_str,
    );
    assert_str_enum(
        crate::scheduled_job::entity::TriggerKind::ALL,
        crate::scheduled_job::entity::TriggerKind::as_str,
    );
    assert_str_enum(
        crate::scheduled_job::entity::InstanceStatus::ALL,
        crate::scheduled_job::entity::InstanceStatus::as_str,
    );
    assert_str_enum(
        crate::scheduled_job::entity::CompletionStatus::ALL,
        crate::scheduled_job::entity::CompletionStatus::as_str,
    );
    assert_str_enum(
        crate::scheduled_job::entity::LogLevel::ALL,
        crate::scheduled_job::entity::LogLevel::as_str,
    );
    assert_str_enum(
        crate::dispatch_pool::entity::DispatchPoolStatus::ALL,
        crate::dispatch_pool::entity::DispatchPoolStatus::as_str,
    );
    assert_str_enum(
        crate::platform_config::entity::ConfigScope::ALL,
        crate::platform_config::entity::ConfigScope::as_str,
    );
    assert_str_enum(
        crate::platform_config::entity::ConfigValueType::ALL,
        crate::platform_config::entity::ConfigValueType::as_str,
    );
    assert_str_enum(
        crate::service_account::entity::WebhookAuthType::ALL,
        crate::service_account::entity::WebhookAuthType::as_str,
    );
    assert_str_enum(
        crate::service_account::entity::SigningAlgorithm::ALL,
        crate::service_account::entity::SigningAlgorithm::as_str,
    );
    assert_str_enum(
        crate::service_account::entity::AssignmentSource::ALL,
        crate::service_account::entity::AssignmentSource::as_str,
    );
    assert_str_enum(
        crate::application::entity::ApplicationType::ALL,
        crate::application::entity::ApplicationType::as_str,
    );
    assert_str_enum(
        crate::application_openapi_spec::entity::OpenApiSpecStatus::ALL,
        crate::application_openapi_spec::entity::OpenApiSpecStatus::as_str,
    );
    assert_str_enum(
        crate::login_attempt::entity::AttemptType::ALL,
        crate::login_attempt::entity::AttemptType::as_str,
    );
    assert_str_enum(
        crate::login_attempt::entity::LoginOutcome::ALL,
        crate::login_attempt::entity::LoginOutcome::as_str,
    );
    assert_str_enum(
        crate::client::entity::ClientStatus::ALL,
        crate::client::entity::ClientStatus::as_str,
    );
    assert_str_enum(
        crate::process::entity::ProcessStatus::ALL,
        crate::process::entity::ProcessStatus::as_str,
    );
    assert_str_enum(
        crate::process::entity::ProcessSource::ALL,
        crate::process::entity::ProcessSource::as_str,
    );
}

/// Every value the production database held at the 2026-09-24 audit
/// decodes under the enum its column is read as. A failure here means a
/// deploy would turn live rows into read errors.
#[test]
fn production_stored_values_all_decode() {
    fn all<T: std::str::FromStr<Err = UnknownEnumValue>>(values: &[&str]) {
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
