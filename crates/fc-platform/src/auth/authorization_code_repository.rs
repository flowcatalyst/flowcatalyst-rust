//! Authorization Code Repository — PostgreSQL via SeaORM
//!
//! Stores authorization codes in `oauth_oidc_payloads` (type = "AuthorizationCode")
//! for compatibility with the TypeScript oidc-provider implementation.

use sea_orm::*;
use sea_orm::prelude::Expr;
use chrono::Utc;
use serde_json::json;
use crate::AuthorizationCode;
use crate::entities::oauth_oidc_payloads;
use crate::shared::error::Result;

const PAYLOAD_TYPE: &str = "AuthorizationCode";

/// Repository for authorization codes via oauth_oidc_payloads.
pub struct AuthorizationCodeRepository {
    db: DatabaseConnection,
}

impl AuthorizationCodeRepository {
    pub fn new(db: &DatabaseConnection) -> Self {
        Self { db: db.clone() }
    }

    /// Build the composite ID: "AuthorizationCode:{code}"
    fn make_id(code: &str) -> String {
        format!("{}:{}", PAYLOAD_TYPE, code)
    }

    /// Build JSONB payload from domain entity
    fn to_payload(code: &AuthorizationCode) -> serde_json::Value {
        json!({
            "accountId": code.principal_id,
            "clientId": code.client_id,
            "redirectUri": code.redirect_uri,
            "scope": code.scope,
            "codeChallenge": code.code_challenge,
            "codeChallengeMethod": code.code_challenge_method,
            "nonce": code.nonce,
            "state": code.state,
            "contextClientId": code.context_client_id,
            "kind": PAYLOAD_TYPE,
            "iat": code.created_at.timestamp(),
            "exp": code.expires_at.timestamp(),
        })
    }

    /// Convert from SeaORM model back to domain entity
    fn from_model(m: oauth_oidc_payloads::Model) -> AuthorizationCode {
        let p = &m.payload;
        let code = m.id.strip_prefix("AuthorizationCode:").unwrap_or(&m.id).to_string();
        let created_at = m.created_at.with_timezone(&Utc);
        let expires_at = m.expires_at
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|| created_at + chrono::Duration::minutes(10));

        // consumed_at being set means the code was used
        let used = m.consumed_at.is_some();

        AuthorizationCode {
            code,
            client_id: p.get("clientId").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            principal_id: p.get("accountId").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            redirect_uri: p.get("redirectUri").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            scope: p.get("scope").and_then(|v| v.as_str()).map(String::from),
            code_challenge: p.get("codeChallenge").and_then(|v| v.as_str()).map(String::from),
            code_challenge_method: p.get("codeChallengeMethod").and_then(|v| v.as_str()).map(String::from),
            nonce: p.get("nonce").and_then(|v| v.as_str()).map(String::from),
            state: p.get("state").and_then(|v| v.as_str()).map(String::from),
            context_client_id: p.get("contextClientId").and_then(|v| v.as_str()).map(String::from),
            created_at,
            expires_at,
            used,
        }
    }

    /// Insert a new authorization code.
    pub async fn insert(&self, code: &AuthorizationCode) -> Result<()> {
        let model = oauth_oidc_payloads::ActiveModel {
            id: Set(Self::make_id(&code.code)),
            r#type: Set(PAYLOAD_TYPE.to_string()),
            payload: Set(Self::to_payload(code)),
            grant_id: Set(None),
            user_code: Set(None),
            uid: Set(None),
            expires_at: Set(Some(code.expires_at.into())),
            consumed_at: Set(None),
            created_at: Set(code.created_at.into()),
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

    /// Find an authorization code by its code value.
    pub async fn find_by_code(&self, code: &str) -> Result<Option<AuthorizationCode>> {
        let composite_id = Self::make_id(code);
        let result = oauth_oidc_payloads::Entity::find_by_id(composite_id)
            .one(&self.db)
            .await?;
        Ok(result.map(Self::from_model))
    }

    /// Find a valid (not used, not expired) authorization code.
    pub async fn find_valid_code(&self, code: &str) -> Result<Option<AuthorizationCode>> {
        let composite_id = Self::make_id(code);
        let now: chrono::DateTime<chrono::FixedOffset> = Utc::now().into();
        let result = oauth_oidc_payloads::Entity::find()
            .filter(oauth_oidc_payloads::Column::Id.eq(composite_id))
            .filter(oauth_oidc_payloads::Column::ConsumedAt.is_null())
            .filter(oauth_oidc_payloads::Column::ExpiresAt.gt(now))
            .one(&self.db)
            .await?;
        Ok(result.map(Self::from_model))
    }

    /// Mark an authorization code as used (consumed).
    pub async fn mark_as_used(&self, code: &str) -> Result<bool> {
        let composite_id = Self::make_id(code);
        let now: chrono::DateTime<chrono::FixedOffset> = Utc::now().into();
        let result = oauth_oidc_payloads::Entity::update_many()
            .col_expr(oauth_oidc_payloads::Column::ConsumedAt, Expr::value(Some(now)))
            .filter(oauth_oidc_payloads::Column::Id.eq(composite_id))
            .exec(&self.db)
            .await?;
        Ok(result.rows_affected > 0)
    }

    /// Delete an authorization code.
    pub async fn delete(&self, code: &str) -> Result<bool> {
        let composite_id = Self::make_id(code);
        let result = oauth_oidc_payloads::Entity::delete_by_id(composite_id)
            .exec(&self.db)
            .await?;
        Ok(result.rows_affected > 0)
    }

    /// Delete all expired authorization codes.
    pub async fn delete_expired(&self) -> Result<u64> {
        let now: chrono::DateTime<chrono::FixedOffset> = Utc::now().into();
        let result = oauth_oidc_payloads::Entity::delete_many()
            .filter(oauth_oidc_payloads::Column::Type.eq(PAYLOAD_TYPE))
            .filter(oauth_oidc_payloads::Column::ExpiresAt.lt(now))
            .exec(&self.db)
            .await?;
        Ok(result.rows_affected)
    }

    /// Delete all authorization codes for a principal.
    pub async fn delete_by_principal(&self, principal_id: &str) -> Result<u64> {
        let result = oauth_oidc_payloads::Entity::delete_many()
            .filter(oauth_oidc_payloads::Column::Type.eq(PAYLOAD_TYPE))
            .filter(Expr::cust_with_values(
                "payload->>'accountId' = $1",
                [principal_id.to_string()],
            ))
            .exec(&self.db)
            .await?;
        Ok(result.rows_affected)
    }

    /// Delete all authorization codes for a client.
    pub async fn delete_by_client(&self, client_id: &str) -> Result<u64> {
        let result = oauth_oidc_payloads::Entity::delete_many()
            .filter(oauth_oidc_payloads::Column::Type.eq(PAYLOAD_TYPE))
            .filter(Expr::cust_with_values(
                "payload->>'clientId' = $1",
                [client_id.to_string()],
            ))
            .exec(&self.db)
            .await?;
        Ok(result.rows_affected)
    }

    /// Count all authorization codes.
    pub async fn count(&self) -> Result<u64> {
        let count = oauth_oidc_payloads::Entity::find()
            .filter(oauth_oidc_payloads::Column::Type.eq(PAYLOAD_TYPE))
            .count(&self.db)
            .await?;
        Ok(count)
    }

    /// Count valid (not consumed, not expired) authorization codes.
    pub async fn count_valid(&self) -> Result<u64> {
        let now: chrono::DateTime<chrono::FixedOffset> = Utc::now().into();
        let count = oauth_oidc_payloads::Entity::find()
            .filter(oauth_oidc_payloads::Column::Type.eq(PAYLOAD_TYPE))
            .filter(oauth_oidc_payloads::Column::ConsumedAt.is_null())
            .filter(oauth_oidc_payloads::Column::ExpiresAt.gt(now))
            .count(&self.db)
            .await?;
        Ok(count)
    }
}
