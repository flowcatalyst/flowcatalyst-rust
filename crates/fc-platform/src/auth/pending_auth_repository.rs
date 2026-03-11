//! Pending Auth State Repository — PostgreSQL via SeaORM
//!
//! Stores OAuth pending authorization states in `oauth_oidc_payloads` (type = "PendingAuth")
//! to survive server restarts. Replaces the in-memory HashMap that was used previously.

use sea_orm::*;
use chrono::{Utc, Duration};
use serde::{Deserialize, Serialize};
use serde_json::json;
use crate::entities::oauth_oidc_payloads;
use crate::shared::error::Result;

const PAYLOAD_TYPE: &str = "PendingAuth";
/// Pending auth states expire after 10 minutes
const EXPIRY_SECONDS: i64 = 600;

/// Pending authorization state (between /authorize and callback)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingAuth {
    pub client_id: String,
    pub redirect_uri: String,
    pub scope: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    pub nonce: Option<String>,
    pub created_at: chrono::DateTime<Utc>,
}

/// Repository for pending OAuth authorization states.
pub struct PendingAuthRepository {
    db: DatabaseConnection,
}

impl PendingAuthRepository {
    pub fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
    }

    /// Build the composite ID: "PendingAuth:{state}"
    fn make_id(state: &str) -> String {
        format!("{}:{}", PAYLOAD_TYPE, state)
    }

    /// Store a pending auth state keyed by the state parameter.
    pub async fn insert(&self, state_param: &str, pending: &PendingAuth) -> Result<()> {
        let now = Utc::now();
        let expires_at = now + Duration::seconds(EXPIRY_SECONDS);

        let payload = json!({
            "clientId": pending.client_id,
            "redirectUri": pending.redirect_uri,
            "scope": pending.scope,
            "codeChallenge": pending.code_challenge,
            "codeChallengeMethod": pending.code_challenge_method,
            "nonce": pending.nonce,
            "createdAt": pending.created_at.to_rfc3339(),
        });

        let model = oauth_oidc_payloads::ActiveModel {
            id: Set(Self::make_id(state_param)),
            r#type: Set(PAYLOAD_TYPE.to_string()),
            payload: Set(payload),
            grant_id: Set(None),
            user_code: Set(None),
            uid: Set(None),
            expires_at: Set(Some(expires_at.into())),
            consumed_at: Set(None),
            created_at: Set(now.into()),
        };
        oauth_oidc_payloads::Entity::insert(model)
            .on_conflict(
                sea_query::OnConflict::column(oauth_oidc_payloads::Column::Id)
                    .update_columns([
                        oauth_oidc_payloads::Column::Payload,
                        oauth_oidc_payloads::Column::ExpiresAt,
                    ])
                    .to_owned()
            )
            .exec(&self.db)
            .await?;
        Ok(())
    }

    /// Find and remove a pending auth state atomically (single-use).
    /// Returns None if state doesn't exist or has expired.
    pub async fn find_and_consume(&self, state_param: &str) -> Result<Option<PendingAuth>> {
        let composite_id = Self::make_id(state_param);
        let now: chrono::DateTime<chrono::FixedOffset> = Utc::now().into();

        // Find valid (non-expired, non-consumed) state
        let result = oauth_oidc_payloads::Entity::find()
            .filter(oauth_oidc_payloads::Column::Id.eq(&composite_id))
            .filter(oauth_oidc_payloads::Column::ConsumedAt.is_null())
            .filter(oauth_oidc_payloads::Column::ExpiresAt.gt(now))
            .one(&self.db)
            .await?;

        let model = match result {
            Some(m) => m,
            None => return Ok(None),
        };

        // Delete it (single-use)
        oauth_oidc_payloads::Entity::delete_by_id(&composite_id)
            .exec(&self.db)
            .await?;

        Ok(Some(Self::from_payload(&model.payload)))
    }

    fn from_payload(p: &serde_json::Value) -> PendingAuth {
        let created_at = p.get("createdAt")
            .and_then(|v| v.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(Utc::now);

        PendingAuth {
            client_id: p.get("clientId").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            redirect_uri: p.get("redirectUri").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            scope: p.get("scope").and_then(|v| v.as_str()).map(String::from),
            code_challenge: p.get("codeChallenge").and_then(|v| v.as_str()).map(String::from),
            code_challenge_method: p.get("codeChallengeMethod").and_then(|v| v.as_str()).map(String::from),
            nonce: p.get("nonce").and_then(|v| v.as_str()).map(String::from),
            created_at,
        }
    }

    /// Delete all expired pending auth states (cleanup).
    pub async fn delete_expired(&self) -> Result<u64> {
        let now: chrono::DateTime<chrono::FixedOffset> = Utc::now().into();
        let result = oauth_oidc_payloads::Entity::delete_many()
            .filter(oauth_oidc_payloads::Column::Type.eq(PAYLOAD_TYPE))
            .filter(oauth_oidc_payloads::Column::ExpiresAt.lt(now))
            .exec(&self.db)
            .await?;
        Ok(result.rows_affected)
    }
}
