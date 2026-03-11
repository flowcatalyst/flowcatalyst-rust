//! CorsAllowedOrigin Entity

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

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
    pub fn new(origin: impl Into<String>, description: Option<String>, created_by: Option<String>) -> Self {
        let now = Utc::now();
        Self {
            id: crate::TsidGenerator::generate(crate::EntityType::CorsOrigin),
            origin: origin.into(),
            description,
            created_by,
            created_at: now,
            updated_at: now,
        }
    }
}

impl From<crate::entities::tnt_cors_allowed_origins::Model> for CorsAllowedOrigin {
    fn from(m: crate::entities::tnt_cors_allowed_origins::Model) -> Self {
        Self {
            id: m.id,
            origin: m.origin,
            description: m.description,
            created_by: m.created_by,
            created_at: m.created_at.with_timezone(&Utc),
            updated_at: m.updated_at.with_timezone(&Utc),
        }
    }
}
