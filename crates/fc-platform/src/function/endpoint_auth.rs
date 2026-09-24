//! Java `function/EndpointAuth.java`.

use crate::shared::enum_str::str_enum;
use crate::usecase::UseCaseError;

/// How the host authenticates a call before it reaches the function:
///
/// - `webhook`: the platform's signature, under the function's
///   application's signing secret (subscriptions, dispatch jobs, schedules);
/// - `platform`: a platform bearer token, verified against the platform's
///   JWKS, with the principal passed to the function;
/// - `none`: the host checks nothing.
///
/// There is no default: an absent `auth` is the manifest's
/// `ENDPOINT_AUTH_REQUIRED`, distinct from an unrecognised one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EndpointAuth {
    Webhook,
    Platform,
    None,
}

str_enum!(EndpointAuth, "endpoint auth", {
    Webhook => "WEBHOOK",
    Platform => "PLATFORM",
    None => "NONE",
});

impl EndpointAuth {
    pub const INVALID_MESSAGE: &'static str = "auth must be webhook, platform or none";

    /// The lower-case spelling used in the manifest's JSON.
    pub fn wire_value(self) -> &'static str {
        match self {
            EndpointAuth::Webhook => "webhook",
            EndpointAuth::Platform => "platform",
            EndpointAuth::None => "none",
        }
    }

    /// The manifest reader: case-insensitive.
    pub fn try_parse_strict(raw: &str) -> Option<EndpointAuth> {
        match raw.to_lowercase().as_str() {
            "webhook" => Some(EndpointAuth::Webhook),
            "platform" => Some(EndpointAuth::Platform),
            "none" => Some(EndpointAuth::None),
            _ => None,
        }
    }

    /// `ENDPOINT_INVALID` for an unrecognised value.
    pub fn parse_strict(raw: &str) -> Result<EndpointAuth, UseCaseError> {
        Self::try_parse_strict(raw)
            .ok_or_else(|| UseCaseError::validation("ENDPOINT_INVALID", Self::INVALID_MESSAGE))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spellings() {
        assert_eq!(
            "WEBHOOK".parse::<EndpointAuth>().unwrap(),
            EndpointAuth::Webhook
        );
        assert!("webhook".parse::<EndpointAuth>().is_err());
        assert_eq!(
            EndpointAuth::parse_strict("Platform").unwrap(),
            EndpointAuth::Platform
        );
        assert_eq!(EndpointAuth::None.wire_value(), "none");
        let err = EndpointAuth::parse_strict("bearer").unwrap_err();
        assert_eq!(err.code(), "ENDPOINT_INVALID");
        assert_eq!(err.message(), "auth must be webhook, platform or none");
    }
}
