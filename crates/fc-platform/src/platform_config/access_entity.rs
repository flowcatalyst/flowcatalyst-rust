//! PlatformConfigAccess Entity

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformConfigAccess {
    pub id: String,
    pub application_code: String,
    pub role_code: String,
    pub can_read: bool,
    pub can_write: bool,
    pub created_at: DateTime<Utc>,
}

impl PlatformConfigAccess {
    pub fn new(application_code: impl Into<String>, role_code: impl Into<String>) -> Self {
        Self {
            id: crate::TsidGenerator::generate(crate::EntityType::ConfigAccess),
            application_code: application_code.into(),
            role_code: role_code.into(),
            can_read: true,
            can_write: false,
            created_at: Utc::now(),
        }
    }
}

impl From<crate::entities::app_platform_config_access::Model> for PlatformConfigAccess {
    fn from(m: crate::entities::app_platform_config_access::Model) -> Self {
        Self {
            id: m.id,
            application_code: m.application_code,
            role_code: m.role_code,
            can_read: m.can_read,
            can_write: m.can_write,
            created_at: m.created_at.with_timezone(&Utc),
        }
    }
}
