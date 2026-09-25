//! Java `function/FunctionAddressPattern.java`.

use std::fmt;

use crate::dns_label::DnsLabel;
use crate::function_address::FunctionAddress;
use crate::ValidationError;

/// What a permission grant, list filter or status view names: an exact
/// address, `app.service.*` or `app.*`. "Everything" is the absence of a
/// filter, not a pattern. Matching compares whole segments, never a prefix.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FunctionAddressPattern {
    Exact(FunctionAddress),
    Service {
        application: DnsLabel,
        service: DnsLabel,
    },
    Application(DnsLabel),
}

impl FunctionAddressPattern {
    /// `ADDRESS_PATTERN_INVALID` for a bare `*`, a wildcard anywhere but
    /// last, a partial-segment wildcard, or a four-segment pattern.
    pub fn parse(raw: &str) -> Result<FunctionAddressPattern, ValidationError> {
        let invalid = || {
            ValidationError::new(
                "ADDRESS_PATTERN_INVALID",
                "address pattern must be app.service.function, app.service.*, or app.*",
            )
        };
        let parts: Vec<&str> = raw.split('.').collect();
        if parts[..parts.len() - 1].contains(&"*") {
            return Err(invalid());
        }
        let last = parts[parts.len() - 1];
        match parts.len() {
            3 if last == "*" => {
                if !DnsLabel::is_valid(parts[0]) || !DnsLabel::is_valid(parts[1]) {
                    return Err(invalid());
                }
                Ok(FunctionAddressPattern::Service {
                    application: DnsLabel::new_unchecked(parts[0]),
                    service: DnsLabel::new_unchecked(parts[1]),
                })
            }
            3 => FunctionAddress::parse(raw)
                .map(FunctionAddressPattern::Exact)
                .map_err(|_| invalid()),
            2 if last == "*" && DnsLabel::is_valid(parts[0]) => Ok(
                FunctionAddressPattern::Application(DnsLabel::new_unchecked(parts[0])),
            ),
            _ => Err(invalid()),
        }
    }

    /// The pattern's `.`-joined wire form.
    pub fn render(&self) -> String {
        match self {
            FunctionAddressPattern::Exact(address) => address.render(),
            FunctionAddressPattern::Service {
                application,
                service,
            } => format!("{application}.{service}.*"),
            FunctionAddressPattern::Application(application) => format!("{application}.*"),
        }
    }

    /// Whether `address` falls under this pattern, by whole segments.
    pub fn matches(&self, address: &FunctionAddress) -> bool {
        match self {
            FunctionAddressPattern::Exact(exact) => exact == address,
            FunctionAddressPattern::Service {
                application,
                service,
            } => {
                application.value() == address.application() && service.value() == address.service()
            }
            FunctionAddressPattern::Application(application) => {
                application.value() == address.application()
            }
        }
    }
}

impl fmt::Display for FunctionAddressPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render())
    }
}

/// Java `FunctionAddressPatternTest`.
#[cfg(test)]
mod tests {
    use super::*;

    fn address(raw: &str) -> FunctionAddress {
        FunctionAddress::parse(raw).unwrap()
    }

    #[test]
    fn matches_table() {
        for (rule, pattern, addr, expected) in [
            (
                "exact",
                "billing.invoices.create",
                "billing.invoices.create",
                true,
            ),
            (
                "exact",
                "billing.invoices.create",
                "billing.invoices.created",
                false,
            ),
            (
                "service",
                "billing.invoices.*",
                "billing.invoices.create",
                true,
            ),
            (
                "service whole segment",
                "billing.invoices.*",
                "billing.invoices-v2.create",
                false,
            ),
            (
                "service other app",
                "billing.invoices.*",
                "billing2.invoices.create",
                false,
            ),
            ("application", "billing.*", "billing.invoices.create", true),
            ("application", "billing.*", "billing.payments.refund", true),
            (
                "application whole segment",
                "billing.*",
                "billing-eu.invoices.create",
                false,
            ),
        ] {
            assert_eq!(
                FunctionAddressPattern::parse(pattern)
                    .unwrap()
                    .matches(&address(addr)),
                expected,
                "{rule}: {pattern} vs {addr}"
            );
        }
    }

    #[test]
    fn parse_shapes_and_render() {
        assert_eq!(
            FunctionAddressPattern::parse("billing.invoices.create").unwrap(),
            FunctionAddressPattern::Exact(address("billing.invoices.create"))
        );
        for raw in ["billing.invoices.create", "billing.invoices.*", "billing.*"] {
            assert_eq!(FunctionAddressPattern::parse(raw).unwrap().render(), raw);
        }
        assert!(matches!(
            FunctionAddressPattern::parse("billing.invoices.*").unwrap(),
            FunctionAddressPattern::Service { .. }
        ));
        assert!(matches!(
            FunctionAddressPattern::parse("billing.*").unwrap(),
            FunctionAddressPattern::Application(_)
        ));
    }

    #[test]
    fn rejected_patterns() {
        for raw in [
            "*",
            "a.*.c",
            "*.b.c",
            "billing.inv*",
            "a.b.c.*",
            "",
            "billing",
            "Billing.*",
            "a.B.*",
            "a.b.C",
        ] {
            assert_eq!(
                FunctionAddressPattern::parse(raw).unwrap_err().code(),
                "ADDRESS_PATTERN_INVALID",
                "{raw:?}"
            );
        }
    }
}
