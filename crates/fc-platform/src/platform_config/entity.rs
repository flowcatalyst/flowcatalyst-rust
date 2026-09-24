//! PlatformConfig Entity

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConfigScope {
    Global,
    Client,
}

crate::shared::enum_str::str_enum!(ConfigScope, "config scope", {
    Global => "GLOBAL",
    Client => "CLIENT",
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConfigValueType {
    Plain,
    Secret,
}

crate::shared::enum_str::str_enum!(ConfigValueType, "config value type", {
    Plain => "PLAIN",
    Secret => "SECRET",
});

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformConfig {
    pub id: String,
    pub application_code: String,
    pub section: String,
    pub property: String,
    pub scope: ConfigScope,
    pub client_id: Option<String>,
    pub value_type: ConfigValueType,
    pub value: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl PlatformConfig {
    pub fn new(
        application_code: impl Into<String>,
        section: impl Into<String>,
        property: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: crate::shared::tsid::generate(crate::EntityType::PlatformConfig),
            application_code: application_code.into(),
            section: section.into(),
            property: property.into(),
            scope: ConfigScope::Global,
            client_id: None,
            value_type: ConfigValueType::Plain,
            value: value.into(),
            description: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn masked_value(&self) -> &str {
        if self.value_type == ConfigValueType::Secret {
            "***"
        } else {
            &self.value
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_new_platform_config() {
        let config = PlatformConfig::new("my-app", "email", "smtp_host", "smtp.example.com");

        assert!(!config.id.is_empty());
        assert!(
            config.id.starts_with("pcf_"),
            "ID should have pcf_ prefix, got: {}",
            config.id
        );
        assert_eq!(
            config.id.len(),
            17,
            "Typed ID should be 17 chars, got: {}",
            config.id.len()
        );
        assert_eq!(config.application_code, "my-app");
        assert_eq!(config.section, "email");
        assert_eq!(config.property, "smtp_host");
        assert_eq!(config.value, "smtp.example.com");
        assert_eq!(config.scope, ConfigScope::Global);
        assert!(config.client_id.is_none());
        assert_eq!(config.value_type, ConfigValueType::Plain);
        assert!(config.description.is_none());
        assert_eq!(config.created_at, config.updated_at);
    }

    #[test]
    fn test_platform_config_unique_ids() {
        let c1 = PlatformConfig::new("a", "s", "p", "v1");
        let c2 = PlatformConfig::new("a", "s", "p", "v2");
        assert_ne!(c1.id, c2.id);
    }

    // --- ConfigScope ---

    #[test]
    fn test_config_scope_as_str() {
        assert_eq!(ConfigScope::Global.as_str(), "GLOBAL");
        assert_eq!(ConfigScope::Client.as_str(), "CLIENT");
    }

    #[test]
    fn test_config_scope_from_str() {
        assert_eq!(ConfigScope::from_str("GLOBAL"), Ok(ConfigScope::Global));
        assert_eq!(ConfigScope::from_str("CLIENT"), Ok(ConfigScope::Client));
        assert!(ConfigScope::from_str("unknown").is_err());
    }

    #[test]
    fn test_config_scope_roundtrip() {
        for s in [ConfigScope::Global, ConfigScope::Client] {
            assert_eq!(
                ConfigScope::from_str(s.as_str()),
                Ok(s),
                "Roundtrip failed for {:?}",
                s
            );
        }
    }

    // --- ConfigValueType ---

    #[test]
    fn test_config_value_type_as_str() {
        assert_eq!(ConfigValueType::Plain.as_str(), "PLAIN");
        assert_eq!(ConfigValueType::Secret.as_str(), "SECRET");
    }

    #[test]
    fn test_config_value_type_from_str() {
        assert_eq!(
            ConfigValueType::from_str("PLAIN"),
            Ok(ConfigValueType::Plain)
        );
        assert_eq!(
            ConfigValueType::from_str("SECRET"),
            Ok(ConfigValueType::Secret)
        );
        assert!(ConfigValueType::from_str("unknown").is_err());
    }

    #[test]
    fn test_config_value_type_roundtrip() {
        for t in [ConfigValueType::Plain, ConfigValueType::Secret] {
            assert_eq!(
                ConfigValueType::from_str(t.as_str()),
                Ok(t),
                "Roundtrip failed for {:?}",
                t
            );
        }
    }

    // --- masked_value ---

    #[test]
    fn test_masked_value_plain() {
        let config = PlatformConfig::new("app", "s", "p", "my-value");
        assert_eq!(config.masked_value(), "my-value");
    }

    #[test]
    fn test_masked_value_secret() {
        let mut config = PlatformConfig::new("app", "s", "p", "super-secret");
        config.value_type = ConfigValueType::Secret;
        assert_eq!(config.masked_value(), "***");
    }
}
