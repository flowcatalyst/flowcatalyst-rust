//! Refresh Token Repository — PostgreSQL via SeaORM
//!
//! Stores refresh tokens in `oauth_oidc_payloads` (type = "RefreshToken")
//! for compatibility with the TypeScript oidc-provider implementation.

use sea_orm::*;
use sea_orm::prelude::Expr;
use chrono::{Utc, Duration};
use serde_json::json;
use crate::RefreshToken;
use crate::entities::oauth_oidc_payloads;
use crate::shared::error::Result;

const PAYLOAD_TYPE: &str = "RefreshToken";

/// Repository for refresh token management via oauth_oidc_payloads
pub struct RefreshTokenRepository {
    db: DatabaseConnection,
}

impl RefreshTokenRepository {
    pub fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
    }

    /// Build the composite ID: "RefreshToken:{id}"
    fn make_id(id: &str) -> String {
        format!("{}:{}", PAYLOAD_TYPE, id)
    }

    /// Build JSONB payload from domain entity
    fn to_payload(token: &RefreshToken) -> serde_json::Value {
        json!({
            "accountId": token.principal_id,
            "clientId": token.oauth_client_id,
            "tokenHash": token.token_hash,
            "scope": token.scopes.join(" "),
            "accessibleClients": token.accessible_clients,
            "revoked": token.revoked,
            "revokedAt": token.revoked_at.map(|dt| dt.to_rfc3339()),
            "tokenFamily": token.token_family,
            "replacedBy": token.replaced_by,
            "lastUsedAt": token.last_used_at.map(|dt| dt.to_rfc3339()),
            "createdFromIp": token.created_from_ip,
            "userAgent": token.user_agent,
            "iat": token.created_at.timestamp(),
            "exp": token.expires_at.timestamp(),
            "kind": PAYLOAD_TYPE,
        })
    }

    /// Convert from SeaORM model back to domain entity
    fn from_model(m: oauth_oidc_payloads::Model) -> RefreshToken {
        let p = &m.payload;
        let id = m.id.strip_prefix("RefreshToken:").unwrap_or(&m.id).to_string();

        let scopes: Vec<String> = p.get("scope")
            .and_then(|v| v.as_str())
            .map(|s| s.split_whitespace().filter(|v| !v.is_empty()).map(String::from).collect())
            .unwrap_or_default();

        let accessible_clients: Vec<String> = p.get("accessibleClients")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        let revoked = p.get("revoked").and_then(|v| v.as_bool()).unwrap_or(false);

        let revoked_at = p.get("revokedAt")
            .and_then(|v| v.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        let token_family = p.get("tokenFamily").and_then(|v| v.as_str()).map(String::from);
        let replaced_by = p.get("replacedBy").and_then(|v| v.as_str()).map(String::from);

        let last_used_at = p.get("lastUsedAt")
            .and_then(|v| v.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        let created_from_ip = p.get("createdFromIp").and_then(|v| v.as_str()).map(String::from);
        let user_agent = p.get("userAgent").and_then(|v| v.as_str()).map(String::from);

        let created_at = m.created_at.with_timezone(&Utc);
        let expires_at = m.expires_at
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|| created_at + Duration::days(30));

        RefreshToken {
            id,
            token_hash: p.get("tokenHash").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            principal_id: p.get("accountId").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            oauth_client_id: p.get("clientId").and_then(|v| v.as_str()).map(String::from),
            scopes,
            accessible_clients,
            revoked,
            revoked_at,
            token_family,
            replaced_by,
            created_at,
            expires_at,
            last_used_at,
            created_from_ip,
            user_agent,
        }
    }

    /// Insert a new refresh token
    pub async fn insert(&self, token: &RefreshToken) -> Result<()> {
        let model = oauth_oidc_payloads::ActiveModel {
            id: Set(Self::make_id(&token.id)),
            r#type: Set(PAYLOAD_TYPE.to_string()),
            payload: Set(Self::to_payload(token)),
            grant_id: Set(token.token_family.clone()),
            user_code: Set(None),
            uid: Set(None),
            expires_at: Set(Some(token.expires_at.into())),
            consumed_at: Set(None),
            created_at: Set(token.created_at.into()),
        };
        oauth_oidc_payloads::Entity::insert(model)
            .on_conflict(
                sea_query::OnConflict::column(oauth_oidc_payloads::Column::Id)
                    .update_columns([
                        oauth_oidc_payloads::Column::Payload,
                        oauth_oidc_payloads::Column::GrantId,
                        oauth_oidc_payloads::Column::ExpiresAt,
                    ])
                    .to_owned()
            )
            .exec(&self.db)
            .await?;
        Ok(())
    }

    /// Find a refresh token by its hash
    pub async fn find_by_hash(&self, token_hash: &str) -> Result<Option<RefreshToken>> {
        // Query payloads of type RefreshToken where payload->>'tokenHash' matches
        let result = oauth_oidc_payloads::Entity::find()
            .filter(oauth_oidc_payloads::Column::Type.eq(PAYLOAD_TYPE))
            .filter(Expr::cust_with_values(
                "payload->>'tokenHash' = $1",
                [token_hash.to_string()],
            ))
            .one(&self.db)
            .await?;
        Ok(result.map(Self::from_model))
    }

    /// Find a valid (non-expired, non-revoked) refresh token by its hash
    pub async fn find_valid_by_hash(&self, token_hash: &str) -> Result<Option<RefreshToken>> {
        let now: chrono::DateTime<chrono::FixedOffset> = Utc::now().into();
        let result = oauth_oidc_payloads::Entity::find()
            .filter(oauth_oidc_payloads::Column::Type.eq(PAYLOAD_TYPE))
            .filter(Expr::cust_with_values(
                "payload->>'tokenHash' = $1",
                [token_hash.to_string()],
            ))
            .filter(oauth_oidc_payloads::Column::ExpiresAt.gt(now))
            .filter(oauth_oidc_payloads::Column::ConsumedAt.is_null())
            .one(&self.db)
            .await?;
        match result {
            Some(m) => {
                let token = Self::from_model(m);
                if token.revoked { Ok(None) } else { Ok(Some(token)) }
            }
            None => Ok(None),
        }
    }

    /// Find all tokens for a principal
    pub async fn find_by_principal(&self, principal_id: &str) -> Result<Vec<RefreshToken>> {
        let rows = oauth_oidc_payloads::Entity::find()
            .filter(oauth_oidc_payloads::Column::Type.eq(PAYLOAD_TYPE))
            .filter(Expr::cust_with_values(
                "payload->>'accountId' = $1",
                [principal_id.to_string()],
            ))
            .all(&self.db)
            .await?;
        Ok(rows.into_iter().map(Self::from_model).collect())
    }

    /// Find all active tokens for a principal
    pub async fn find_active_by_principal(&self, principal_id: &str) -> Result<Vec<RefreshToken>> {
        let now: chrono::DateTime<chrono::FixedOffset> = Utc::now().into();
        let rows = oauth_oidc_payloads::Entity::find()
            .filter(oauth_oidc_payloads::Column::Type.eq(PAYLOAD_TYPE))
            .filter(Expr::cust_with_values(
                "payload->>'accountId' = $1",
                [principal_id.to_string()],
            ))
            .filter(oauth_oidc_payloads::Column::ExpiresAt.gt(now))
            .filter(oauth_oidc_payloads::Column::ConsumedAt.is_null())
            .all(&self.db)
            .await?;
        Ok(rows.into_iter().map(Self::from_model).filter(|t| !t.revoked).collect())
    }

    /// Revoke a token by its ID
    pub async fn revoke_by_id(&self, id: &str) -> Result<bool> {
        let composite_id = Self::make_id(id);
        if let Some(model) = oauth_oidc_payloads::Entity::find_by_id(&composite_id).one(&self.db).await? {
            let mut token = Self::from_model(model);
            token.revoke();
            let update = oauth_oidc_payloads::ActiveModel {
                id: Set(composite_id),
                payload: Set(Self::to_payload(&token)),
                consumed_at: Set(Some(Utc::now().into())),
                ..Default::default()
            };
            oauth_oidc_payloads::Entity::update(update).exec(&self.db).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Revoke a token by its hash
    pub async fn revoke_by_hash(&self, token_hash: &str) -> Result<bool> {
        if let Some(token) = self.find_by_hash(token_hash).await? {
            self.revoke_by_id(&token.id).await
        } else {
            Ok(false)
        }
    }

    /// Revoke all tokens for a principal (logout all devices)
    pub async fn revoke_all_for_principal(&self, principal_id: &str) -> Result<u64> {
        let tokens = self.find_active_by_principal(principal_id).await?;
        let count = tokens.len() as u64;
        for token in tokens {
            self.revoke_by_id(&token.id).await?;
        }
        Ok(count)
    }

    /// Find all tokens in a token family (by grant_id)
    pub async fn find_by_family(&self, family_id: &str) -> Result<Vec<RefreshToken>> {
        let rows = oauth_oidc_payloads::Entity::find()
            .filter(oauth_oidc_payloads::Column::Type.eq(PAYLOAD_TYPE))
            .filter(oauth_oidc_payloads::Column::GrantId.eq(family_id))
            .all(&self.db)
            .await?;
        Ok(rows.into_iter().map(Self::from_model).collect())
    }

    /// Revoke all tokens in a token family
    pub async fn revoke_all_in_family(&self, family_id: &str) -> Result<u64> {
        let tokens = self.find_by_family(family_id).await?;
        let count = tokens.iter().filter(|t| !t.revoked).count() as u64;
        for token in tokens {
            if !token.revoked {
                self.revoke_by_id(&token.id).await?;
            }
        }
        Ok(count)
    }

    /// Mark a token as replaced during token rotation
    pub async fn mark_as_replaced(&self, token_hash: &str, new_token_hash: &str) -> Result<bool> {
        if let Some(mut token) = self.find_by_hash(token_hash).await? {
            token.mark_replaced(new_token_hash);
            let composite_id = Self::make_id(&token.id);
            let update = oauth_oidc_payloads::ActiveModel {
                id: Set(composite_id),
                payload: Set(Self::to_payload(&token)),
                ..Default::default()
            };
            oauth_oidc_payloads::Entity::update(update).exec(&self.db).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Check if a token was replaced
    pub async fn was_replaced(&self, token_hash: &str) -> Result<bool> {
        if let Some(token) = self.find_by_hash(token_hash).await? {
            Ok(token.was_replaced())
        } else {
            Ok(false)
        }
    }

    /// Update last used timestamp
    pub async fn update_last_used(&self, id: &str) -> Result<bool> {
        let composite_id = Self::make_id(id);
        if let Some(model) = oauth_oidc_payloads::Entity::find_by_id(&composite_id).one(&self.db).await? {
            let mut token = Self::from_model(model);
            token.mark_used();
            let update = oauth_oidc_payloads::ActiveModel {
                id: Set(composite_id),
                payload: Set(Self::to_payload(&token)),
                ..Default::default()
            };
            oauth_oidc_payloads::Entity::update(update).exec(&self.db).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Delete expired tokens (cleanup job)
    pub async fn delete_expired(&self) -> Result<u64> {
        let now: chrono::DateTime<chrono::FixedOffset> = Utc::now().into();
        let result = oauth_oidc_payloads::Entity::delete_many()
            .filter(oauth_oidc_payloads::Column::Type.eq(PAYLOAD_TYPE))
            .filter(oauth_oidc_payloads::Column::ExpiresAt.lt(now))
            .exec(&self.db)
            .await?;
        Ok(result.rows_affected)
    }

    /// Delete revoked tokens older than a given date (cleanup job)
    pub async fn delete_revoked_before(&self, cutoff: chrono::DateTime<Utc>) -> Result<u64> {
        let cutoff_fixed: chrono::DateTime<chrono::FixedOffset> = cutoff.into();
        let result = oauth_oidc_payloads::Entity::delete_many()
            .filter(oauth_oidc_payloads::Column::Type.eq(PAYLOAD_TYPE))
            .filter(oauth_oidc_payloads::Column::ConsumedAt.is_not_null())
            .filter(oauth_oidc_payloads::Column::CreatedAt.lt(cutoff_fixed))
            .exec(&self.db)
            .await?;
        Ok(result.rows_affected)
    }

    /// Count active tokens for a principal
    pub async fn count_active_for_principal(&self, principal_id: &str) -> Result<u64> {
        let tokens = self.find_active_by_principal(principal_id).await?;
        Ok(tokens.len() as u64)
    }

    /// Count all refresh token payloads
    pub async fn count(&self) -> Result<u64> {
        let count = oauth_oidc_payloads::Entity::find()
            .filter(oauth_oidc_payloads::Column::Type.eq(PAYLOAD_TYPE))
            .count(&self.db)
            .await?;
        Ok(count)
    }

    /// Count expired refresh tokens
    pub async fn count_expired(&self) -> Result<u64> {
        let now: chrono::DateTime<chrono::FixedOffset> = Utc::now().into();
        let count = oauth_oidc_payloads::Entity::find()
            .filter(oauth_oidc_payloads::Column::Type.eq(PAYLOAD_TYPE))
            .filter(oauth_oidc_payloads::Column::ExpiresAt.lt(now))
            .count(&self.db)
            .await?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    // Repository tests require a PostgreSQL connection
    // These would typically be integration tests
}
