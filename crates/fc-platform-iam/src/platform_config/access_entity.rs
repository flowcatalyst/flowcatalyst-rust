//! PlatformConfigAccess Entity

use chrono::{DateTime, Utc};
use fc_platform_core::shared::tsid;
use fc_platform_core::shared::tsid::EntityType;
use serde::{Deserialize, Serialize};

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
            id: tsid::generate(EntityType::ConfigAccess),
            application_code: application_code.into(),
            role_code: role_code.into(),
            can_read: true,
            can_write: false,
            created_at: Utc::now(),
        }
    }
}
