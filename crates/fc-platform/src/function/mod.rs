//! The function registry (Java `server/.../platform/function/`, pinned at
//! `0118cdca`). The management interface for functions mirrors Java as
//! closely as possible; each file names the Java class it ports.
//!
//! The value types and the manifest live in `fc-function-model`, shared with
//! the function host as Java's host links the server's classes; they are
//! re-exported here under their old paths (`function::manifest::Manifest`,
//! `function::DnsLabel`, …), and their [`ValidationError`] becomes a
//! [`UseCaseError`] with the same code and message. `FunctionAddress` itself
//! lives in `fc-function-abi`, shared with the guest PDK too.

pub mod api;
pub mod artifact;
pub mod control_api;
pub mod cron_dialect;
pub mod desired_state;
pub mod domain_api;
pub mod domain_repository;
pub mod entity;
pub mod host_repository;
pub mod openapi;
pub mod operations;
pub mod policy_api;
pub mod policy_repository;
pub mod repository;
pub mod route_repository;
pub mod schedule_check;
pub mod schema;
pub mod settings_repository;
pub mod trigger_object_repository;
pub mod version_api;
pub mod version_repository;
pub mod wire;

pub use fc_function_model::{
    digest, dns_label, endpoint_auth, function_address, function_address_pattern, function_limits,
    function_owner, hostname, http_method, json, manifest, pool_url_template, route_pattern,
    runtime, setting_key, subscription_mode,
};

pub use fc_function_model::{
    parse_address, BlankClientId, ClientCeilings, Cors, DbRef, Digest, DnsLabel, Endpoint,
    EndpointAuth, EntrypointRule, FunctionAddress, FunctionAddressPattern, FunctionLimits,
    FunctionOwner, Hostname, HttpMethod, InvalidPoolUrl, JsonNode, JsonNumber, JsonParseError,
    Limits, Manifest, ManifestProblem, ManifestRejected, NonPositiveLimit, PathParams,
    PoolUrlTemplate, PublicRoute, RouteMatch, RoutePattern, Runtime, ScheduleSpec, Segment,
    SettingKey, SubscriptionMode, SubscriptionSpec, UnreadableManifest, ValidationError,
    LIVE_ALIAS,
};

use crate::usecase::UseCaseError;

/// A model validation error is a use case's validation error (a 400), with
/// the same code and message; a manifest problem's pointer becomes
/// `details.pointer`, as the check route returns it.
impl From<ValidationError> for UseCaseError {
    fn from(e: ValidationError) -> Self {
        match e.pointer() {
            None => UseCaseError::validation(e.code(), e.message()),
            Some(pointer) => {
                let mut details = std::collections::HashMap::new();
                details.insert(
                    "pointer".to_string(),
                    serde_json::Value::String(pointer.to_string()),
                );
                UseCaseError::validation_with_details(e.code(), e.message(), details)
            }
        }
    }
}

/// At the HTTP layer, the same 400 a use case's validation error becomes.
impl From<ValidationError> for crate::shared::error::PlatformError {
    fn from(e: ValidationError) -> Self {
        UseCaseError::from(e).into()
    }
}

/// The router's dispatch mode for a manifest subscription's `mode`: the
/// model keeps its own enum so it need not depend on `fc-common`.
pub fn dispatch_mode(mode: SubscriptionMode) -> fc_common::DispatchMode {
    match mode {
        SubscriptionMode::Immediate => fc_common::DispatchMode::Immediate,
        SubscriptionMode::NextOnError => fc_common::DispatchMode::NextOnError,
        SubscriptionMode::BlockOnError => fc_common::DispatchMode::BlockOnError,
    }
}

/// Java's `String.isBlank`.
pub(crate) use fc_function_model::java::is_blank as java_is_blank;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_error_keeps_code_and_message() {
        let err = UseCaseError::from(DnsLabel::parse("pool", "Bad").unwrap_err());
        assert_eq!(err.code(), "LABEL_INVALID");
        assert_eq!(
            err.message(),
            "pool must be a DNS label: 1-63 characters of a-z, 0-9 and '-', \
             not starting or ending with '-'"
        );
        assert!(err.details().is_empty());
        assert_eq!(err.http_status_code(), 400);
    }

    /// The check route's entry: the problem's pointer in `details`.
    #[test]
    fn a_manifest_problem_carries_its_pointer_in_details() {
        let root = JsonNode::parse(r#"{"runtime":"jvm","entrypoint":"a.B","pool":"Bad"}"#).unwrap();
        let defaults = FunctionLimits::defaults();
        let rejected = Manifest::check(
            Some(&root),
            Runtime::Jvm,
            &defaults,
            &ClientCeilings::of(&defaults),
        )
        .unwrap_err();
        let entry = UseCaseError::from(rejected.problems()[0].to_validation_error());
        assert_eq!(entry.code(), "POOL_INVALID");
        assert_eq!(entry.message(), "pool must be a DNS label");
        assert_eq!(entry.details()["pointer"], "/pool");
        // Publish rejects with the first problem and no details.
        let first = UseCaseError::from(rejected.first_error());
        assert_eq!(first.code(), "POOL_INVALID");
        assert!(first.details().is_empty());
    }

    #[test]
    fn dispatch_mode_is_the_same_constant() {
        for mode in SubscriptionMode::ALL {
            assert_eq!(dispatch_mode(*mode).as_str(), mode.as_str());
        }
    }
}
