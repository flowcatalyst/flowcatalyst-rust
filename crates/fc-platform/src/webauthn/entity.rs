//! WebAuthn Credential — pure data, no persistence.
//!
//! Wraps a webauthn-rs `Passkey` plus FlowCatalyst metadata (id, principal,
//! name, timestamps). The Passkey holds counter / backup-state / public key
//! and is updated in-place by `webauthn-rs` after successful authentications.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use webauthn_rs::prelude::{AuthenticationResult, Passkey};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebauthnCredential {
    pub id: String,
    pub principal_id: String,
    pub passkey: Passkey,
    pub name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

impl WebauthnCredential {
    pub fn new(principal_id: impl Into<String>, passkey: Passkey, name: Option<String>) -> Self {
        Self {
            id: crate::shared::tsid::generate(crate::EntityType::WebauthnCredential),
            principal_id: principal_id.into(),
            passkey,
            name,
            created_at: Utc::now(),
            last_used_at: None,
        }
    }

    /// The opaque authenticator-issued identifier — the BYTEA we index on.
    pub fn credential_id_bytes(&self) -> Vec<u8> {
        self.passkey.cred_id().as_ref().to_vec()
    }

    /// Apply a successful authentication result to the underlying Passkey.
    /// `webauthn-rs` enforces counter monotonicity and updates backup-state.
    /// Returns true when something actually changed.
    pub fn record_authentication(&mut self, result: &AuthenticationResult) -> bool {
        let changed = self.passkey.update_credential(result).unwrap_or(false);
        self.last_used_at = Some(Utc::now());
        changed
    }
}
