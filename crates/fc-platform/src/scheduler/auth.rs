//! Dispatch Auth Service
//!
//! Signs dispatch-job ids so the router's callback to `/api/dispatch/process`
//! (and `/api/dispatch/settled`) can prove it carries a job this platform
//! queued. A port of Go's `DispatchAuthService`
//! (`internal/platform/scheduler/auth.go`) and of its key derivation
//! (`dispatchAuthSecret`, `internal/server/subsystems.go`), byte for byte, so a
//! token signed by either platform verifies on the other:
//!
//! - secret = hex(HKDF-SHA256(ikm = FLOWCATALYST_APP_KEY trimmed, salt = none,
//!   info = "fc-dispatch-auth", 32 bytes)), a 64-character string;
//! - token  = hex(HMAC-SHA256(key = the secret's 64 ASCII bytes, message = job id)).
//!
//! The scheduler stamps the token into the queue message's `authToken`; both
//! routers forward it as `Authorization: Bearer <token>`.

use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

/// The HKDF `info` Go binds the derived key to.
const DISPATCH_AUTH_INFO: &[u8] = b"fc-dispatch-auth";

/// Signs and verifies dispatch-job tokens.
#[derive(Clone)]
pub struct DispatchAuthService {
    /// The derived secret, as Go passes it to `NewDispatchAuthService`: the
    /// hex string itself is the HMAC key.
    secret: String,
}

impl std::fmt::Debug for DispatchAuthService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DispatchAuthService")
            .finish_non_exhaustive()
    }
}

impl DispatchAuthService {
    /// A service over an already-derived secret.
    pub fn with_secret(secret: impl Into<String>) -> Self {
        Self {
            secret: secret.into(),
        }
    }

    /// Derive the secret from the application key, as Go does. `None` when
    /// the key is blank: the caller must fail closed.
    pub fn from_app_key(app_key: &str) -> Option<Self> {
        let key = app_key.trim();
        if key.is_empty() {
            return None;
        }
        Some(Self::with_secret(derive_dispatch_secret(key)))
    }

    /// Derive from `FLOWCATALYST_APP_KEY`. `None` when it is unset or blank.
    pub fn from_env() -> Option<Self> {
        std::env::var("FLOWCATALYST_APP_KEY")
            .ok()
            .and_then(|k| Self::from_app_key(&k))
    }

    /// The lower-case hex HMAC-SHA256 of `job_id`.
    pub fn sign(&self, job_id: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(self.secret.as_bytes())
            .expect("HMAC takes a key of any length");
        mac.update(job_id.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    /// Whether `token` is `job_id`'s token, compared in constant time.
    pub fn verify(&self, job_id: &str, token: &str) -> bool {
        if token.is_empty() {
            return false;
        }
        self.sign(job_id).as_bytes().ct_eq(token.as_bytes()).into()
    }
}

/// HKDF-SHA256 (RFC 5869) with no salt, `info = "fc-dispatch-auth"`, 32
/// bytes, hex-encoded. One expand block suffices for 32 bytes.
pub fn derive_dispatch_secret(app_key: &str) -> String {
    hex::encode(hkdf_sha256_32(app_key.as_bytes(), DISPATCH_AUTH_INFO))
}

fn hkdf_sha256_32(ikm: &[u8], info: &[u8]) -> [u8; 32] {
    // Extract: an absent salt is a hash-length run of zeros.
    let mut extract = HmacSha256::new_from_slice(&[0u8; 32]).expect("any key length");
    extract.update(ikm);
    let prk = extract.finalize().into_bytes();
    // Expand: T(1) = HMAC(PRK, info || 0x01).
    let mut expand = HmacSha256::new_from_slice(&prk).expect("any key length");
    expand.update(info);
    expand.update(&[0x01]);
    let okm = expand.finalize().into_bytes();
    let mut out = [0u8; 32];
    out.copy_from_slice(&okm[..32]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 5869 test case 3 (no salt, no info), first 32 bytes.
    #[test]
    fn hkdf_matches_rfc_5869_case_3() {
        assert_eq!(
            hex::encode(hkdf_sha256_32(&[0x0b; 22], b"")),
            "8da4e775a563c18f715f802a063c5a31b8a11f5c5ee1879ec3454e5f3c738d2d"
        );
    }

    /// Golden values computed independently (Python `hmac`/`hashlib`) from
    /// Go's recipe: a token the Go scheduler signs for this key verifies here.
    #[test]
    fn derivation_and_token_match_go() {
        assert_eq!(
            derive_dispatch_secret("test-app-key"),
            "3390cc7007f0d27b6ce93adaf9a76941ab89d8b9e7acc28c7c12a25e35579f65"
        );
        let svc = DispatchAuthService::from_app_key("  test-app-key\n").unwrap();
        assert_eq!(
            svc.sign("0HZXEQ5Y8JY5Z"),
            "97b8ea522f6146ed58b5764dfcc8a579ab2c2348b518668bc4d8607d9e9db847"
        );
    }

    #[test]
    fn verify_accepts_only_the_jobs_own_token() {
        let svc = DispatchAuthService::from_app_key("k").unwrap();
        let token = svc.sign("job1");
        assert!(svc.verify("job1", &token));
        assert!(!svc.verify("job2", &token));
        assert!(!svc.verify("job1", ""));
        assert!(!svc.verify("job1", "nope"));
        let other = DispatchAuthService::from_app_key("other").unwrap();
        assert!(!other.verify("job1", &token));
    }

    #[test]
    fn a_blank_key_fails_closed() {
        assert!(DispatchAuthService::from_app_key("").is_none());
        assert!(DispatchAuthService::from_app_key("   ").is_none());
    }
}
