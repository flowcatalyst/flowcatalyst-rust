use std::fmt;
use std::str::FromStr;

/// A function's fully-qualified address, `app.service.name`.
///
/// Mirrors Java `function-api/src/main/java/io/flowcatalyst/function/FunctionAddress.java`,
/// which is itself a deliberate copy of the server's parser, pinned by the
/// shared `function-address-table.csv` (copied byte-identical into
/// `tests/data/`).
///
/// Each segment is a DNS label: 1-63 characters of lower-case letters, digits
/// and `-`, not starting or ending with `-`. No normalisation: the input is
/// not trimmed and not lower-cased, so an accepted address always renders
/// back to exactly its input.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FunctionAddress {
    application: String,
    service: String,
    name: String,
}

/// An address or segment was not three DNS labels separated by `.`. The one
/// message is Java's, whatever the cause.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("address must be app.service.function: three DNS labels separated by '.'")]
pub struct InvalidAddress;

impl FunctionAddress {
    /// Builds an address from its three segments, each checked as a DNS label
    /// (Java's canonical constructor).
    pub fn new(
        application: impl Into<String>,
        service: impl Into<String>,
        name: impl Into<String>,
    ) -> Result<Self, InvalidAddress> {
        let (application, service, name) = (application.into(), service.into(), name.into());
        if !(is_valid_label(&application) && is_valid_label(&service) && is_valid_label(&name)) {
            return Err(InvalidAddress);
        }
        Ok(Self {
            application,
            service,
            name,
        })
    }

    /// Splits on `.` and requires exactly three segments, each a valid DNS
    /// label. Never supplies a default service segment.
    pub fn parse(raw: &str) -> Result<Self, InvalidAddress> {
        let mut parts = raw.split('.');
        match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(application), Some(service), Some(name), None) => {
                Self::new(application, service, name)
            }
            _ => Err(InvalidAddress),
        }
    }

    /// The owning application's DNS label.
    pub fn application(&self) -> &str {
        &self.application
    }

    /// The service grouping's DNS label.
    pub fn service(&self) -> &str {
        &self.service
    }

    /// The function's own DNS label.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// `app.service.name`.
    pub fn render(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for FunctionAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.application, self.service, self.name)
    }
}

impl FromStr for FunctionAddress {
    type Err = InvalidAddress;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Java's `^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$`, applied with `matches()`
/// (so a trailing newline is not accepted).
fn is_valid_label(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    let alnum = |b: &u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    match bytes {
        [] => false,
        [only] => alnum(only),
        [first, middle @ .., last] => {
            bytes.len() <= 63
                && alnum(first)
                && alnum(last)
                && middle.iter().all(|b| alnum(b) || *b == b'-')
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Java FunctionAddressTest.threePartsAccepted
    #[test]
    fn three_parts_accepted() {
        let address = FunctionAddress::parse("billing.invoices.create").unwrap();
        assert_eq!(address.application(), "billing");
        assert_eq!(address.service(), "invoices");
        assert_eq!(address.name(), "create");
        assert_eq!(address.render(), "billing.invoices.create");
        assert_eq!(address.to_string(), "billing.invoices.create");
    }

    // Java FunctionAddressTest.ofDoesNotReparse
    #[test]
    fn new_applies_the_same_rule_per_segment() {
        assert_eq!(
            FunctionAddress::new("billing", "invoices", "create").unwrap(),
            FunctionAddress::parse("billing.invoices.create").unwrap()
        );
        assert_eq!(FunctionAddress::new("a.b", "c", "d"), Err(InvalidAddress));
    }

    #[test]
    fn label_length_bounds() {
        let max = "a".repeat(63);
        assert!(is_valid_label(&max));
        assert!(!is_valid_label(&"a".repeat(64)));
        assert!(!is_valid_label("a-"));
        assert!(is_valid_label("a-b"));
        assert!(!is_valid_label("a\n"));
    }

    #[test]
    fn error_message_is_javas() {
        assert_eq!(
            InvalidAddress.to_string(),
            "address must be app.service.function: three DNS labels separated by '.'"
        );
    }
}
