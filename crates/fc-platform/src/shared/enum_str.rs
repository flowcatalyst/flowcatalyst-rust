//! String-backed domain enums.
//!
//! Every enum that crosses the wire or sits in a text column has exactly one
//! canonical spelling per variant (`as_str`), and parsing is strict: an
//! unknown value is an error, never a silent default (owner ruling X-06).
//! Request input that fails to parse becomes a 400; a stored row that fails
//! to parse becomes a loud read error naming the table, column, value and
//! row id (see [`decode`]).
//!
//! The one exemption is dispatch mode (ruling X-01), which stays lenient:
//! see `dispatch_job::entity::parse_dispatch_mode`.

use crate::shared::error::PlatformError;

/// A string that names no variant of the enum it was parsed as.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown {kind} {value:?} (expected one of: {})", expected.join(", "))]
pub struct UnknownEnumValue {
    /// What was being parsed, e.g. "client status".
    pub kind: &'static str,
    /// The rejected input.
    pub value: String,
    /// The canonical spellings that would have been accepted.
    pub expected: &'static [&'static str],
}

impl UnknownEnumValue {
    pub fn new(
        kind: &'static str,
        value: impl Into<String>,
        expected: &'static [&'static str],
    ) -> Self {
        Self {
            kind,
            value: value.into(),
            expected,
        }
    }
}

/// Request input: an unknown enum value is the caller's mistake, so it maps to
/// the standard 400 validation response.
impl From<UnknownEnumValue> for PlatformError {
    fn from(e: UnknownEnumValue) -> Self {
        PlatformError::validation(e.to_string())
    }
}

/// Parse an optional request field. `None` stays `None`; a present but unknown
/// value is a 400.
pub fn parse_opt<T>(value: Option<&str>) -> Result<Option<T>, PlatformError>
where
    T: std::str::FromStr<Err = UnknownEnumValue>,
{
    value
        .map(str::parse)
        .transpose()
        .map_err(PlatformError::from)
}

/// Treats an empty string like an absent one, for optional request fields
/// where `""` has always meant "unspecified".
pub fn non_empty(value: Option<&str>) -> Option<&str> {
    value.filter(|v| !v.is_empty())
}

/// Decode an enum column of a stored row. An unknown value means the row is
/// corrupt (or was written by something that doesn't share this vocabulary):
/// it is logged and surfaced as an internal error naming where it came from,
/// rather than being coerced to a default.
pub fn decode<T>(value: &str, table: &str, column: &str, row_id: &str) -> Result<T, PlatformError>
where
    T: std::str::FromStr<Err = UnknownEnumValue>,
{
    value
        .parse()
        .map_err(|_| corrupt_value(table, column, value, row_id))
}

/// [`decode`] for a nullable column.
pub fn decode_opt<T>(
    value: Option<&str>,
    table: &str,
    column: &str,
    row_id: &str,
) -> Result<Option<T>, PlatformError>
where
    T: std::str::FromStr<Err = UnknownEnumValue>,
{
    value.map(|v| decode(v, table, column, row_id)).transpose()
}

/// The error for a stored value that doesn't parse. Public so decoders for
/// enums this crate doesn't own (fc-common's) can report the same way.
pub fn corrupt_value(table: &str, column: &str, value: &str, row_id: &str) -> PlatformError {
    tracing::error!(
        table,
        column,
        value,
        row_id,
        "stored row holds an unknown enum value"
    );
    PlatformError::internal(format!(
        "{table}.{column} of row {row_id} holds unknown value {value:?}"
    ))
}

/// Implements `as_str`, `ALL`, `FromStr` and `Display` for a fieldless enum.
///
/// Each variant maps to its canonical spelling, optionally followed by
/// `| "ALIAS"` spellings that are accepted on parse but never produced
/// (legacy values still present in stored rows). Anything else is an
/// [`UnknownEnumValue`].
///
/// ```ignore
/// str_enum!(ClientStatus, "client status", {
///     Active => "ACTIVE",
///     Inactive => "INACTIVE",
/// });
/// ```
macro_rules! str_enum {
    (
        $ty:ident, $kind:literal,
        { $( $variant:ident => $s:literal $( | $alias:literal )* ),+ $(,)? }
    ) => {
        impl $ty {
            /// Every variant, in declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// The canonical wire and storage spelling.
            pub fn as_str(&self) -> &'static str {
                match self {
                    $(Self::$variant => $s,)+
                }
            }
        }

        impl ::std::str::FromStr for $ty {
            type Err = $crate::shared::enum_str::UnknownEnumValue;

            fn from_str(s: &str) -> ::std::result::Result<Self, Self::Err> {
                match s {
                    $( $s $( | $alias )* => Ok(Self::$variant), )+
                    _ => Err($crate::shared::enum_str::UnknownEnumValue::new(
                        $kind,
                        s,
                        &[$($s),+],
                    )),
                }
            }
        }

        impl ::std::fmt::Display for $ty {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}
pub(crate) use str_enum;

/// Asserts, for every variant, that `as_str` round-trips through `FromStr` and
/// matches the serde spelling, so the hand-listed strings and the serde
/// derive can't drift apart. Call as `assert_str_enum(X::ALL, X::as_str)`.
#[cfg(test)]
pub(crate) fn assert_str_enum<T>(all: &[T], as_str: fn(&T) -> &'static str)
where
    T: PartialEq
        + std::fmt::Debug
        + serde::Serialize
        + serde::de::DeserializeOwned
        + std::str::FromStr<Err = UnknownEnumValue>,
{
    for v in all {
        let s = as_str(v);
        assert_eq!(s.parse::<T>().ok().as_ref(), Some(v), "round trip of {s}");
        assert_eq!(
            serde_json::to_value(v).unwrap(),
            serde_json::Value::String(s.to_string()),
            "serde spelling of {s}"
        );
        assert_eq!(
            serde_json::from_value::<T>(serde_json::Value::String(s.to_string()))
                .ok()
                .as_ref(),
            Some(v),
            "serde parse of {s}"
        );
    }
    assert!("not-a-variant".parse::<T>().is_err());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
    enum Sample {
        One,
        TwoWords,
    }
    str_enum!(Sample, "sample", {
        One => "ONE",
        TwoWords => "TWO_WORDS" | "TWOWORDS",
    });

    #[test]
    fn strict_parse_with_aliases() {
        assert_str_enum(Sample::ALL, Sample::as_str);
        assert_eq!("TWOWORDS".parse::<Sample>(), Ok(Sample::TwoWords));
        assert_eq!(Sample::TwoWords.as_str(), "TWO_WORDS");
        let err = "one".parse::<Sample>().unwrap_err();
        assert_eq!(
            err.to_string(),
            "unknown sample \"one\" (expected one of: ONE, TWO_WORDS)"
        );
    }

    #[test]
    fn unknown_request_value_is_a_validation_error() {
        let err: PlatformError = "x".parse::<Sample>().unwrap_err().into();
        assert!(matches!(err, PlatformError::Validation { .. }));
    }

    #[test]
    fn corrupt_stored_value_names_its_origin() {
        let err = decode::<Sample>("BAD", "t_things", "kind", "thg_1").unwrap_err();
        let msg = err.to_string();
        for part in ["t_things.kind", "thg_1", "\"BAD\""] {
            assert!(msg.contains(part), "{msg} should mention {part}");
        }
        assert!(matches!(err, PlatformError::Internal { .. }));
        assert_eq!(decode_opt::<Sample>(None, "t", "c", "r").unwrap(), None);
    }

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
            parse_dispatch_status, DispatchAttemptStatus, DispatchKind, DispatchProtocol,
            ErrorType, RetryStrategy,
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
}
