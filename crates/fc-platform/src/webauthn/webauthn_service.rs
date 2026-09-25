//! WebAuthn ceremony service — wraps `webauthn-rs` with FlowCatalyst config.
//!
//! Reads three env vars:
//! - `FC_WEBAUTHN_RP_ID`     (default: `auth.flowcatalyst.io`)
//! - `FC_WEBAUTHN_RP_NAME`   (default: `FlowCatalyst`)
//! - `FC_WEBAUTHN_ORIGINS`   (default: `https://{RP_ID}`; comma-separated allow-list)

use base64::Engine;
use std::env;
use uuid::Uuid;
use webauthn_rs::fake::{FakePasskeyDistribution, WebauthnFakeCredentialGenerator};
use webauthn_rs::prelude::{
    AuthenticationResult, CreationChallengeResponse, CredentialID, Passkey, PasskeyAuthentication,
    PasskeyRegistration, PublicKeyCredential, RegisterPublicKeyCredential,
    RequestChallengeResponse, Url, Webauthn, WebauthnBuilder,
};

use crate::shared::error::{PlatformError, Result};

/// Stable namespace UUID used to derive a webauthn `user_unique_id` from a
/// principal's TSID. Never change this — it's bound into every passkey's
/// userHandle and rotation would orphan all credentials.
const PRINCIPAL_UUID_NAMESPACE: Uuid = Uuid::from_u128(0x6f656263_6661_7470_6173_736b65797576);

pub struct WebauthnService {
    inner: Webauthn,
    rp_id: String,
    fake_generator: WebauthnFakeCredentialGenerator<FakePasskeyDistribution>,
}

impl WebauthnService {
    pub fn from_env() -> Result<Self> {
        // Go's names and defaults (internal/server/wire_services.go,
        // envcfg.go `webauthnOrigins`): the RP id defaults to `localhost`;
        // origins come from `FC_WEBAUTHN_ORIGINS` (comma-separated, matched
        // verbatim), then the legacy singular `FC_WEBAUTHN_RP_ORIGIN`, then
        // `http://localhost:8080`. Blank values count as unset.
        let non_empty = |k: &str| env::var(k).ok().filter(|v| !v.trim().is_empty());
        let rp_id = non_empty("FC_WEBAUTHN_RP_ID").unwrap_or_else(|| "localhost".to_string());
        let rp_name =
            non_empty("FC_WEBAUTHN_RP_NAME").unwrap_or_else(|| "FlowCatalyst".to_string());
        let origins_raw = non_empty("FC_WEBAUTHN_ORIGINS")
            .or_else(|| non_empty("FC_WEBAUTHN_RP_ORIGIN"))
            .unwrap_or_else(|| "http://localhost:8080".to_string());
        let origins: Vec<Url> = origins_raw
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| {
                Url::parse(s).map_err(|e| {
                    PlatformError::internal(format!(
                        "invalid FC_WEBAUTHN_ORIGINS entry {:?}: {}",
                        s, e
                    ))
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let fake_hmac_key = match env::var("FC_WEBAUTHN_FAKE_HMAC_KEY") {
            Ok(b64) => base64::engine::general_purpose::STANDARD
                .decode(&b64)
                .map_err(|e| {
                    PlatformError::internal(format!(
                        "FC_WEBAUTHN_FAKE_HMAC_KEY must be base64: {}",
                        e
                    ))
                })?,
            Err(_) => {
                // Per-process random key — degrades enumeration defence to
                // "fake creds vary across restarts" but never leaks signal
                // within a single process. Set FC_WEBAUTHN_FAKE_HMAC_KEY in
                // prod for cross-pod stability.
                use rand::RngCore;
                let mut k = vec![0u8; 32];
                rand::rng().fill_bytes(&mut k);
                k
            }
        };

        Self::new(&rp_id, &rp_name, &origins, &fake_hmac_key)
    }

    pub fn new(rp_id: &str, rp_name: &str, origins: &[Url], fake_hmac_key: &[u8]) -> Result<Self> {
        let primary = origins.first().ok_or_else(|| {
            PlatformError::internal("FC_WEBAUTHN_ORIGINS must contain at least one origin")
        })?;
        let mut builder = WebauthnBuilder::new(rp_id, primary)
            .map_err(|e| PlatformError::internal(format!("WebauthnBuilder: {}", e)))?
            .rp_name(rp_name);
        for extra in origins.iter().skip(1) {
            builder = builder.append_allowed_origin(extra);
        }
        let inner = builder
            .build()
            .map_err(|e| PlatformError::internal(format!("Webauthn build: {}", e)))?;
        let fake_generator = WebauthnFakeCredentialGenerator::new(fake_hmac_key)
            .map_err(|e| PlatformError::internal(format!("fake generator: {}", e)))?;
        Ok(Self {
            inner,
            rp_id: rp_id.to_string(),
            fake_generator,
        })
    }

    pub fn rp_id(&self) -> &str {
        &self.rp_id
    }

    /// Build a realistic-looking authentication challenge for an unknown,
    /// federated, or no-credential user. Deterministic per email under the
    /// configured HMAC key — repeated calls return the same `allowCredentials`
    /// shape, so attackers can't distinguish "exists with no passkey" from
    /// "doesn't exist" by request fingerprinting.
    pub fn fake_authentication_challenge(&self, email: &str) -> Result<serde_json::Value> {
        use base64::Engine;
        use rand::RngCore;

        let creds = self
            .fake_generator
            .generate(email.as_bytes())
            .map_err(|e| PlatformError::internal(format!("fake generator: {}", e)))?;
        let allow_credentials: Vec<serde_json::Value> = creds
            .iter()
            .map(|cid| {
                let bytes: &[u8] = cid.as_ref();
                serde_json::json!({
                    "type": "public-key",
                    "id": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes),
                })
            })
            .collect();

        let mut challenge = vec![0u8; 32];
        rand::rng().fill_bytes(&mut challenge);
        Ok(serde_json::json!({
            "publicKey": {
                "challenge": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&challenge),
                "timeout": 60_000,
                "rpId": self.rp_id,
                "userVerification": "required",
                "allowCredentials": allow_credentials,
            }
        }))
    }

    /// Derive the stable webauthn user-handle from a principal TSID.
    pub fn user_handle(principal_id: &str) -> Uuid {
        Uuid::new_v5(&PRINCIPAL_UUID_NAMESPACE, principal_id.as_bytes())
    }

    pub fn start_registration(
        &self,
        principal_id: &str,
        user_name: &str,
        display_name: &str,
        already_registered: &[CredentialID],
    ) -> Result<(CreationChallengeResponse, PasskeyRegistration)> {
        let exclude = if already_registered.is_empty() {
            None
        } else {
            Some(already_registered.to_vec())
        };
        self.inner
            .start_passkey_registration(
                Self::user_handle(principal_id),
                user_name,
                display_name,
                exclude,
            )
            .map_err(|e| PlatformError::internal(format!("start_passkey_registration: {}", e)))
    }

    pub fn finish_registration(
        &self,
        response: &RegisterPublicKeyCredential,
        state: &PasskeyRegistration,
    ) -> Result<Passkey> {
        self.inner
            .finish_passkey_registration(response, state)
            .map_err(|e| PlatformError::Unauthorized {
                message: format!("passkey registration failed: {}", e),
            })
    }

    pub fn start_authentication(
        &self,
        passkeys: &[Passkey],
    ) -> Result<(RequestChallengeResponse, PasskeyAuthentication)> {
        self.inner
            .start_passkey_authentication(passkeys)
            .map_err(|e| PlatformError::internal(format!("start_passkey_authentication: {}", e)))
    }

    pub fn finish_authentication(
        &self,
        response: &PublicKeyCredential,
        state: &PasskeyAuthentication,
    ) -> Result<AuthenticationResult> {
        self.inner
            .finish_passkey_authentication(response, state)
            .map_err(|e| PlatformError::Unauthorized {
                message: format!("passkey authentication failed: {}", e),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(rp_id: &str, origin: &str) -> Result<WebauthnService> {
        let url = Url::parse(origin).expect("valid url");
        WebauthnService::new(rp_id, "Test", &[url], &[0u8; 32])
    }

    #[test]
    fn new_rejects_origin_not_under_rp_id() {
        // RP ID must be an effective domain of the origin.
        assert!(build("acme.com", "https://example.com").is_err());
    }

    #[test]
    fn new_accepts_origin_subdomain_of_rp_id() {
        assert!(build("acme.com", "https://auth.acme.com").is_ok());
    }

    #[test]
    fn user_handle_is_deterministic() {
        let a = WebauthnService::user_handle("prn_0HZXEQ5Y8JY5Z");
        let b = WebauthnService::user_handle("prn_0HZXEQ5Y8JY5Z");
        assert_eq!(a, b);
    }

    #[test]
    fn user_handle_varies_with_principal() {
        let a = WebauthnService::user_handle("prn_AAAAAAAAAAAAA");
        let b = WebauthnService::user_handle("prn_BBBBBBBBBBBBB");
        assert_ne!(a, b);
    }

    #[test]
    fn fake_challenge_is_deterministic_per_email() {
        let svc = build("auth.example.com", "https://auth.example.com").unwrap();
        let a = svc
            .fake_authentication_challenge("alice@unknown.com")
            .unwrap();
        let b = svc
            .fake_authentication_challenge("alice@unknown.com")
            .unwrap();
        // Challenge bytes are random; allowCredentials is HMAC-deterministic.
        let a_creds = a["publicKey"]["allowCredentials"].clone();
        let b_creds = b["publicKey"]["allowCredentials"].clone();
        assert_eq!(a_creds, b_creds);
    }

    #[test]
    fn fake_challenge_varies_with_email() {
        let svc = build("auth.example.com", "https://auth.example.com").unwrap();
        let a = svc
            .fake_authentication_challenge("alice@unknown.com")
            .unwrap();
        let b = svc
            .fake_authentication_challenge("bob@unknown.com")
            .unwrap();
        let a_creds = a["publicKey"]["allowCredentials"].clone();
        let b_creds = b["publicKey"]["allowCredentials"].clone();
        // With overwhelming probability these differ.
        assert_ne!(a_creds, b_creds);
    }

    #[test]
    fn fake_challenge_varies_with_key() {
        let svc_a = WebauthnService::new(
            "auth.example.com",
            "Test",
            &[Url::parse("https://auth.example.com").unwrap()],
            &[0u8; 32],
        )
        .unwrap();
        let svc_b = WebauthnService::new(
            "auth.example.com",
            "Test",
            &[Url::parse("https://auth.example.com").unwrap()],
            &[1u8; 32],
        )
        .unwrap();
        let a = svc_a
            .fake_authentication_challenge("alice@unknown.com")
            .unwrap();
        let b = svc_b
            .fake_authentication_challenge("alice@unknown.com")
            .unwrap();
        assert_ne!(
            a["publicKey"]["allowCredentials"],
            b["publicKey"]["allowCredentials"]
        );
    }
}
