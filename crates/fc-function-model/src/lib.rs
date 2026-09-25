//! The FlowCatalyst function model (Java `server/.../platform/function/`,
//! pinned at `0118cdca`): the value types and the manifest, shared by the
//! platform and the function host.
//!
//! Java has one set of these classes, and its host links the server's. This
//! crate is that one set for Rust: the platform re-exports it under
//! `fc_platform::function::*` and maps [`ValidationError`] into its
//! `UseCaseError`; `fc-fnhost-core` reads a version's stored manifest with
//! [`Manifest::read_stored`] and routes requests with [`RoutePattern`], so
//! the host and the platform cannot disagree about either. It is pure: no
//! database, no async runtime, no HTTP, and it builds for
//! `wasm32-unknown-unknown`.
//!
//! Every error code and message is Java's, and
//! `tests/manifest_golden.rs` holds the port to Java's own answers.
//!
//! | Module | Java source (`platform/function/`) |
//! |---|---|
//! | [`manifest`] | `Manifest.java` |
//! | [`json`] | Jackson's `readTree` / `Json.MAPPER` as the manifest uses them |
//! | [`route_pattern`] | `RoutePattern.java` |
//! | [`dns_label`], [`hostname`], [`setting_key`] | `DnsLabel.java`, `Hostname.java`, `SettingKey.java` |
//! | [`digest`] | `Digest.java`, `SignerIdentity.java` |
//! | [`function_address`], [`function_address_pattern`] | `FunctionAddress.java` (the type is `fc_function_abi`'s), `FunctionAddressPattern.java` |
//! | [`function_limits`], [`function_owner`] | `FunctionLimits.java`, `ClientCeilings.java`, `FunctionOwner.java` |
//! | [`runtime`], [`endpoint_auth`], [`http_method`], [`subscription_mode`] | `Runtime.java`, `EndpointAuth.java`, `HttpMethod.java`, `dispatch/DispatchMode.java` |
//! | [`pool_url_template`] | `PoolUrlTemplate.java` |
//! | [`java`] | JDK / Jackson behaviour (`String.isBlank`, `JsonNode.asString`, …) |

mod enum_str;
mod error;

pub mod digest;
pub mod dns_label;
pub mod endpoint_auth;
pub mod function_address;
pub mod function_address_pattern;
pub mod function_limits;
pub mod function_owner;
pub mod hostname;
pub mod http_method;
pub mod java;
pub mod json;
pub mod manifest;
pub mod pool_url_template;
pub mod route_pattern;
pub mod runtime;
pub mod setting_key;
pub mod subscription_mode;

pub use digest::{Digest, SignerIdentity};
pub use dns_label::DnsLabel;
pub use endpoint_auth::EndpointAuth;
pub use error::{UnknownEnumValue, ValidationError};
pub use function_address::{parse_address, FunctionAddress, InvalidAddress};
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
pub use route_pattern::{PathParams, RouteMatch, RoutePattern, Segment};
pub use runtime::{EntrypointRule, Runtime};
pub use setting_key::SettingKey;
pub use subscription_mode::SubscriptionMode;

/// The alias every function's promoted version answers under (Java
/// `Function.LIVE`); reserved, so never an alias prefix.
pub const LIVE_ALIAS: &str = "live";
