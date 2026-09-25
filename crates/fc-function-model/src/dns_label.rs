//! Java `function/DnsLabel.java`.

use std::fmt;

use crate::ValidationError;

/// A DNS label: 1-63 characters of lower-case letters, digits and `-`, not
/// starting or ending with `-`. No normalisation: the input is neither
/// trimmed nor lower-cased, because an address is an identity used in
/// permissions and metrics, and two spellings of one identity become two
/// grants.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DnsLabel(String);

impl DnsLabel {
    /// Whether `raw` is already a valid label (Java's
    /// `^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$` under `matches()`).
    pub fn is_valid(raw: &str) -> bool {
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

    /// `LABEL_INVALID`, naming `field`, when `raw` is not a label.
    pub fn parse(field: &str, raw: &str) -> Result<DnsLabel, ValidationError> {
        if !Self::is_valid(raw) {
            return Err(ValidationError::new(
                "LABEL_INVALID",
                format!(
                    "{field} must be a DNS label: 1-63 characters of a-z, 0-9 and '-', \
                     not starting or ending with '-'"
                ),
            ));
        }
        Ok(DnsLabel(raw.to_string()))
    }

    /// A label already known to be valid (a constant, or text that passed
    /// [`DnsLabel::is_valid`]).
    pub(crate) fn new_unchecked(raw: impl Into<String>) -> DnsLabel {
        let raw = raw.into();
        debug_assert!(Self::is_valid(&raw), "{raw:?} is not a DNS label");
        DnsLabel(raw)
    }

    pub fn value(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DnsLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Java `DnsLabelTest`: every row of the spec's Accepted/Rejected table.
#[cfg(test)]
mod tests {
    use super::*;

    fn assert_rejected(raw: &str) {
        let err = DnsLabel::parse("field", raw).unwrap_err();
        assert_eq!(err.code(), "LABEL_INVALID", "{raw:?}");
    }

    #[test]
    fn accepted() {
        for raw in ["billing", "a", "0", "inv-2", "9lives", "a-b", "a--b"] {
            assert_eq!(DnsLabel::parse("field", raw).unwrap().value(), raw);
        }
        let max = "a".repeat(63);
        assert_eq!(DnsLabel::parse("field", &max).unwrap().value(), max);
    }

    #[test]
    fn rejected() {
        for raw in [
            "Billing",
            "in_voices",
            "a.b",
            "a b",
            "é",
            "",
            "-a",
            "a-",
            "-",
            " a",
            "a ",
            "a\n",
        ] {
            assert_rejected(raw);
        }
        assert_rejected(&"a".repeat(64));
    }

    #[test]
    fn message_names_the_field() {
        let err = DnsLabel::parse("application", "Bad_Label").unwrap_err();
        assert!(err.message().contains("application must be a DNS label"));
    }
}
