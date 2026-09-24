//! The function registry (Java `server/.../platform/function/`, pinned at
//! `0118cdca`): the value types and the manifest. The management interface
//! for functions mirrors Java as closely as possible; each file names the
//! Java class it ports.
//!
//! `FunctionAddress` itself lives in `fc-function-abi`, shared with the host
//! and the guest PDK; [`function_address`] adds the platform's error for it.

pub mod digest;
pub mod dns_label;
pub mod domain_repository;
pub mod endpoint_auth;
pub mod entity;
pub mod function_address;
pub mod function_address_pattern;
pub mod function_limits;
pub mod function_owner;
pub mod host_repository;
pub mod hostname;
pub mod http_method;
pub mod json;
pub mod manifest;
pub mod operations;
pub mod policy_repository;
pub mod pool_url_template;
pub mod repository;
pub mod route_pattern;
pub mod route_repository;
pub mod runtime;
pub mod schema;
pub mod setting_key;
pub mod settings_repository;
pub mod trigger_object_repository;
pub mod version_repository;

pub use digest::Digest;
pub use dns_label::DnsLabel;
pub use endpoint_auth::EndpointAuth;
pub use function_address::{parse_address, FunctionAddress};
pub use function_address_pattern::FunctionAddressPattern;
pub use function_limits::{ClientCeilings, FunctionLimits, NonPositiveLimit};
pub use function_owner::{BlankClientId, FunctionOwner};
pub use hostname::Hostname;
pub use http_method::HttpMethod;
pub use json::{JsonNode, JsonNumber, JsonParseError};
pub use manifest::{
    Cors, DbRef, Endpoint, Limits, Manifest, ManifestProblem, ManifestRejected, PublicRoute,
    ScheduleSpec, SubscriptionSpec, UnreadableManifest,
};
pub use pool_url_template::{InvalidPoolUrl, PoolUrlTemplate};
pub use route_pattern::{RouteMatch, RoutePattern, Segment};
pub use runtime::{EntrypointRule, Runtime};
pub use setting_key::SettingKey;

/// The alias every function's promoted version answers under (Java
/// `Function.LIVE`); reserved, so never an alias prefix.
pub const LIVE_ALIAS: &str = "live";

/// Java's `String.isBlank`: empty, or only characters `Character.isWhitespace`
/// accepts (which excludes the no-break spaces U+00A0, U+2007 and U+202F,
/// and U+0085).
pub(crate) fn java_is_blank(s: &str) -> bool {
    s.chars().all(|c| {
        matches!(
            c,
            '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | '\u{1C}'..='\u{1F}' | ' ' | '\u{1680}'
                | '\u{2000}'..='\u{2006}' | '\u{2008}'..='\u{200A}' | '\u{2028}' | '\u{2029}'
                | '\u{205F}' | '\u{3000}'
        )
    })
}

#[cfg(test)]
mod tests {
    use super::java_is_blank;

    #[test]
    fn blank_follows_character_is_whitespace() {
        assert!(java_is_blank(""));
        assert!(java_is_blank(" \t\u{0B}\u{1C}\u{2003}\u{3000}"));
        assert!(!java_is_blank("\u{00A0}"));
        assert!(!java_is_blank("\u{0085}"));
        assert!(!java_is_blank("\u{2007}"));
        assert!(!java_is_blank("\u{202F}"));
        assert!(!java_is_blank(" x "));
    }
}
