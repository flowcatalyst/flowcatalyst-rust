//! Java `function/Hostname.java`.

use std::fmt;

use super::dns_label::DnsLabel;
use crate::usecase::UseCaseError;

/// A public route's hostname: lower-cased, at most 253 characters, at least
/// two [`DnsLabel`]s, with no trailing dot, wildcard, port or IP literal (an
/// all-numeric last label).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Hostname(String);

const MAX_LENGTH: usize = 253;

impl Hostname {
    /// The message every rejection carries.
    pub const INVALID_MESSAGE: &'static str =
        "hostname must be a lower-cased DNS name of at least two labels, \
         with no trailing dot, wildcard, port or IP literal";

    /// `HOSTNAME_INVALID` for anything that is not a hostname.
    pub fn parse(raw: &str) -> Result<Hostname, UseCaseError> {
        Self::try_parse(raw)
            .ok_or_else(|| UseCaseError::validation("HOSTNAME_INVALID", Self::INVALID_MESSAGE))
    }

    /// [`Hostname::parse`] without the error.
    pub fn try_parse(raw: &str) -> Option<Hostname> {
        if raw.is_empty() {
            return None;
        }
        // Java lower-cases with Locale.ROOT before checking; any character
        // that is not ASCII afterwards fails the label rule anyway.
        let lower = raw.to_lowercase();
        if lower.len() > MAX_LENGTH {
            return None;
        }
        let labels: Vec<&str> = lower.split('.').collect();
        if labels.len() < 2 || !labels.iter().all(|l| DnsLabel::is_valid(l)) {
            return None;
        }
        let last = labels[labels.len() - 1];
        if last.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        Some(Hostname(lower))
    }

    pub fn value(&self) -> &str {
        &self.0
    }

    /// The zone apexes that could cover this hostname, most specific first:
    /// the hostname itself, then each shorter suffix down to two labels.
    /// `qa-myapp.acme.com` gives `["qa-myapp.acme.com", "acme.com"]`.
    pub fn zone_candidates(&self) -> Vec<String> {
        let labels: Vec<&str> = self.0.split('.').collect();
        (0..=labels.len() - 2)
            .map(|start| labels[start..].join("."))
            .collect()
    }
}

impl fmt::Display for Hostname {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Java `HostnameTest`.
#[cfg(test)]
mod tests {
    use super::*;

    fn assert_rejected(raw: &str) {
        let err = Hostname::parse(raw).unwrap_err();
        assert_eq!(err.code(), "HOSTNAME_INVALID", "{raw:?}");
        assert_eq!(err.message(), Hostname::INVALID_MESSAGE);
    }

    #[test]
    fn case_is_lowered() {
        assert_eq!(
            Hostname::parse("API.Acme.com").unwrap().value(),
            "api.acme.com"
        );
    }

    #[test]
    fn accepted() {
        for raw in ["a.io", "x-1.acme.co.za"] {
            assert_eq!(Hostname::parse(raw).unwrap().value(), raw);
        }
    }

    #[test]
    fn rejected() {
        for raw in [
            "localhost",
            "a..com",
            "-a.com",
            "a_b.com",
            "acme.com.",
            "*.acme.com",
            "acme.com:443",
            "https://acme.com",
            "10.0.0.1",
            "",
        ] {
            assert_rejected(raw);
        }
        assert_rejected(&format!("{}.com", "a".repeat(64)));
    }

    #[test]
    fn length_limit_is_253() {
        let label63 = "a".repeat(63);
        let ok = format!("{label63}.{label63}.{label63}.{}", "a".repeat(61));
        assert_eq!(ok.len(), 253);
        assert_eq!(Hostname::parse(&ok).unwrap().value(), ok);
        let too_long = format!("{label63}.{label63}.{label63}.{}", "a".repeat(62));
        assert_eq!(too_long.len(), 254);
        assert_rejected(&too_long);
    }

    #[test]
    fn zone_candidates() {
        let z = |raw: &str| Hostname::parse(raw).unwrap().zone_candidates();
        assert_eq!(z("qa-myapp.acme.com"), ["qa-myapp.acme.com", "acme.com"]);
        assert_eq!(z("acme.com"), ["acme.com"]);
        assert_eq!(
            z("a.b.c.acme.com"),
            ["a.b.c.acme.com", "b.c.acme.com", "c.acme.com", "acme.com"]
        );
    }
}
