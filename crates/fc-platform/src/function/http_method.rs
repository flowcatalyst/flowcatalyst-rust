//! Java `function/HttpMethod.java`.

use crate::shared::enum_str::str_enum;
use crate::usecase::UseCaseError;

/// An HTTP method an endpoint may accept. Upper-case on the wire and when
/// stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HttpMethod {
    Get,
    Head,
    Post,
    Put,
    Patch,
    Delete,
    Options,
}

str_enum!(HttpMethod, "HTTP method", {
    Get => "GET",
    Head => "HEAD",
    Post => "POST",
    Put => "PUT",
    Patch => "PATCH",
    Delete => "DELETE",
    Options => "OPTIONS",
});

impl HttpMethod {
    /// The manifest reader: case-insensitive (Java upper-cases with
    /// `Locale.ROOT`, then matches exactly).
    pub fn try_parse_strict(raw: &str) -> Option<HttpMethod> {
        raw.to_uppercase().parse().ok()
    }

    /// `ENDPOINT_INVALID` for an unknown method.
    pub fn parse_strict(raw: &str) -> Result<HttpMethod, UseCaseError> {
        Self::try_parse_strict(raw).ok_or_else(|| {
            UseCaseError::validation("ENDPOINT_INVALID", format!("unknown HTTP method: {raw}"))
        })
    }
}

/// Java `HttpMethodTest`.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_parse_is_exact() {
        for (raw, method) in [
            ("GET", HttpMethod::Get),
            ("HEAD", HttpMethod::Head),
            ("POST", HttpMethod::Post),
            ("PUT", HttpMethod::Put),
            ("PATCH", HttpMethod::Patch),
            ("DELETE", HttpMethod::Delete),
            ("OPTIONS", HttpMethod::Options),
        ] {
            assert_eq!(raw.parse::<HttpMethod>().unwrap(), method);
        }
        assert!("get".parse::<HttpMethod>().is_err());
        assert!("TRACE".parse::<HttpMethod>().is_err());
    }

    #[test]
    fn parse_strict_is_case_insensitive() {
        assert_eq!(HttpMethod::parse_strict("get").unwrap(), HttpMethod::Get);
        assert_eq!(HttpMethod::parse_strict("Post").unwrap(), HttpMethod::Post);
        assert_eq!(
            HttpMethod::parse_strict("DELETE").unwrap(),
            HttpMethod::Delete
        );
    }

    #[test]
    fn parse_strict_rejects_unknown_method() {
        let err = HttpMethod::parse_strict("TRACE").unwrap_err();
        assert_eq!(err.code(), "ENDPOINT_INVALID");
        assert_eq!(err.message(), "unknown HTTP method: TRACE");
    }
}
