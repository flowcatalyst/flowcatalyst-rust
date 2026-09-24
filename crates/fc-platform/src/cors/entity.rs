//! CorsAllowedOrigin Entity

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorsAllowedOrigin {
    pub id: String,
    pub origin: String,
    pub description: Option<String>,
    pub created_by: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl CorsAllowedOrigin {
    pub fn new(
        origin: impl Into<String>,
        description: Option<String>,
        created_by: Option<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: crate::shared::tsid::generate(crate::EntityType::CorsOrigin),
            origin: origin.into(),
            description,
            created_by,
            created_at: now,
            updated_at: now,
        }
    }
}
