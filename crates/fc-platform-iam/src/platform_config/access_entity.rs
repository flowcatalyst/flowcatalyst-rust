//! PlatformConfigAccess Entity

use chrono::{DateTime, Utc};
use fc_platform_core::shared::id::PlatformConfigAccessId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformConfigAccess {
    pub id: PlatformConfigAccessId,
    pub application_code: String,
    pub role_code: String,
    pub can_read: bool,
    pub can_write: bool,
    pub created_at: DateTime<Utc>,
}

impl PlatformConfigAccess {
    pub fn new(application_code: impl Into<String>, role_code: impl Into<String>) -> Self {
        Self {
            id: PlatformConfigAccessId::generate(),
            application_code: application_code.into(),
            role_code: role_code.into(),
            can_read: true,
            can_write: false,
            created_at: Utc::now(),
        }
    }
}
