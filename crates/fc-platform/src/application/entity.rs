//! Application Entity — matches TypeScript Application domain

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[derive(Default)]
pub enum ApplicationType {
    #[default]
    Application,
    Integration,
}

crate::shared::enum_str::str_enum!(ApplicationType, "application type", {
    Application => "APPLICATION",
    Integration => "INTEGRATION",
});

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Application {
    pub id: String,
    #[serde(rename = "type")]
    pub application_type: ApplicationType,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub icon_url: Option<String>,
    pub website: Option<String>,
    pub logo: Option<String>,
    pub logo_mime_type: Option<String>,
    pub default_base_url: Option<String>,
    pub service_account_id: Option<String>,
    pub active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Application {
    pub fn new(code: impl Into<String>, name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: crate::shared::tsid::generate(crate::EntityType::Application),
            application_type: ApplicationType::Application,
            code: code.into(),
            name: name.into(),
            description: None,
            icon_url: None,
            website: None,
            logo: None,
            logo_mime_type: None,
            default_base_url: None,
            service_account_id: None,
            active: true,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn integration(code: impl Into<String>, name: impl Into<String>) -> Self {
        let mut app = Self::new(code, name);
        app.application_type = ApplicationType::Integration;
        app
    }

    pub fn is_integration(&self) -> bool {
        self.application_type == ApplicationType::Integration
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.default_base_url = Some(url.into());
        self
    }

    pub fn with_icon_url(mut self, url: impl Into<String>) -> Self {
        self.icon_url = Some(url.into());
        self
    }

    pub fn activate(&mut self) {
        self.active = true;
        self.updated_at = Utc::now();
    }

    pub fn deactivate(&mut self) {
        self.active = false;
        self.updated_at = Utc::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_new_application() {
        let app = Application::new("my-app", "My Application");

        assert!(!app.id.is_empty());
        assert!(
            app.id.starts_with("app_"),
            "ID should have app_ prefix, got: {}",
            app.id
        );
        assert_eq!(
            app.id.len(),
            17,
            "Typed ID should be 17 chars, got: {}",
            app.id.len()
        );
        assert_eq!(app.code, "my-app");
        assert_eq!(app.name, "My Application");
        assert_eq!(app.application_type, ApplicationType::Application);
        assert!(app.description.is_none());
        assert!(app.icon_url.is_none());
        assert!(app.website.is_none());
        assert!(app.logo.is_none());
        assert!(app.logo_mime_type.is_none());
        assert!(app.default_base_url.is_none());
        assert!(app.service_account_id.is_none());
        assert!(app.active);
        assert_eq!(app.created_at, app.updated_at);
    }

    #[test]
    fn test_application_unique_ids() {
        let a1 = Application::new("a", "A");
        let a2 = Application::new("b", "B");
        assert_ne!(a1.id, a2.id);
    }

    #[test]
    fn test_application_type_as_str() {
        assert_eq!(ApplicationType::Application.as_str(), "APPLICATION");
        assert_eq!(ApplicationType::Integration.as_str(), "INTEGRATION");
    }

    #[test]
    fn test_application_type_from_str() {
        assert_eq!(
            ApplicationType::from_str("APPLICATION"),
            Ok(ApplicationType::Application)
        );
        assert_eq!(
            ApplicationType::from_str("INTEGRATION"),
            Ok(ApplicationType::Integration)
        );
        // Unknown values are rejected (X-06)
        assert!(ApplicationType::from_str("unknown").is_err());
        assert!(ApplicationType::from_str("").is_err());
    }

    #[test]
    fn test_application_type_default() {
        assert_eq!(ApplicationType::default(), ApplicationType::Application);
    }

    #[test]
    fn test_application_type_roundtrip() {
        for t in [ApplicationType::Application, ApplicationType::Integration] {
            let s = t.as_str();
            assert_eq!(
                ApplicationType::from_str(s),
                Ok(t),
                "Roundtrip failed for {:?}",
                t
            );
        }
    }

    #[test]
    fn test_application_integration_constructor() {
        let app = Application::integration("my-int", "My Integration");
        assert_eq!(app.application_type, ApplicationType::Integration);
        assert!(app.is_integration());
        assert_eq!(app.code, "my-int");
        assert_eq!(app.name, "My Integration");
    }

    #[test]
    fn test_application_is_not_integration_by_default() {
        let app = Application::new("app", "App");
        assert!(!app.is_integration());
    }

    #[test]
    fn test_application_builder_methods() {
        let app = Application::new("app", "App")
            .with_description("A test app")
            .with_base_url("https://example.com")
            .with_icon_url("https://example.com/icon.png");

        assert_eq!(app.description, Some("A test app".to_string()));
        assert_eq!(
            app.default_base_url,
            Some("https://example.com".to_string())
        );
        assert_eq!(
            app.icon_url,
            Some("https://example.com/icon.png".to_string())
        );
    }

    #[test]
    fn test_application_activate_deactivate() {
        let mut app = Application::new("app", "App");
        assert!(app.active);

        app.deactivate();
        assert!(!app.active);

        app.activate();
        assert!(app.active);
    }

    #[test]
    fn test_application_deactivate_updates_timestamp() {
        let mut app = Application::new("app", "App");
        let before = app.updated_at;

        app.deactivate();
        assert!(app.updated_at >= before);
    }
}
