//! PlatformConfigAccess Entity

use chrono::{DateTime, Utc};
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
            id: crate::shared::tsid::generate(crate::EntityType::ConfigAccess),
            application_code: application_code.into(),
            role_code: role_code.into(),
            can_read: true,
            can_write: false,
            created_at: Utc::now(),
        }
    }
}
