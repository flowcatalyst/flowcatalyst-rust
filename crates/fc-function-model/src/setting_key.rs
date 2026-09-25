//! Java `function/SettingKey.java`.

use std::fmt;

use crate::ValidationError;

/// A config or secret key name, shared by `fn_config` / `fn_secrets` and by
/// a manifest's `config`, `secrets` and `db[].secretRef` entries:
/// `^[A-Za-z][A-Za-z0-9_./-]{0,99}$`, the same rule as the tables' CHECK.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SettingKey(String);

const PATTERN: &str = "^[A-Za-z][A-Za-z0-9_./-]{0,99}$";

impl SettingKey {
    pub fn is_valid(value: &str) -> bool {
        let bytes = value.as_bytes();
        match bytes.split_first() {
            Some((first, rest)) => {
                first.is_ascii_alphabetic()
                    && rest.len() <= 99
                    && rest.iter().all(|b| {
                        b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'/' | b'-')
                    })
            }
            None => false,
        }
    }

    /// The message a rejection carries, for callers that report the rule
    /// under their own code (the manifest's `CONFIG_INVALID` / `DB_INVALID`).
    pub fn invalid_message(value: &str) -> String {
        format!("'{value}' is not a valid setting key: expected {PATTERN}")
    }

    /// `SETTING_KEY_INVALID` unless `value` follows the rule.
    pub fn parse(value: &str) -> Result<SettingKey, ValidationError> {
        if !Self::is_valid(value) {
            return Err(ValidationError::new(
                "SETTING_KEY_INVALID",
                Self::invalid_message(value),
            ));
        }
        Ok(SettingKey(value.to_string()))
    }

    pub fn value(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SettingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted() {
        for raw in [
            "A",
            "INVOICE_PREFIX",
            "billing/stripe-key",
            "a.b-c_d/e",
            "Z9",
        ] {
            assert_eq!(SettingKey::parse(raw).unwrap().value(), raw);
        }
        let max = format!("a{}", "b".repeat(99));
        assert!(SettingKey::is_valid(&max));
    }

    #[test]
    fn rejected() {
        for raw in ["", "1BAD", "BAD KEY", "_x", "-x", "a:b", "é", "a\n"] {
            let err = SettingKey::parse(raw).unwrap_err();
            assert_eq!(err.code(), "SETTING_KEY_INVALID", "{raw:?}");
            assert_eq!(
                err.message(),
                format!("'{raw}' is not a valid setting key: expected ^[A-Za-z][A-Za-z0-9_./-]{{0,99}}$")
            );
        }
        assert!(!SettingKey::is_valid(&format!("a{}", "b".repeat(100))));
    }
}
