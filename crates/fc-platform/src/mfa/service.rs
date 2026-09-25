//! The second-factor business layer (Go `internal/platform/mfa/service.go`):
//! TOTP and email-PIN enrolment and verification, recovery codes and
//! trusted devices. Decoupled from the principal aggregate: callers pass the
//! user's id and, for email, the address.

use std::sync::Arc;

use chrono::{Duration, Utc};

use super::crypto;
use super::entity::{EmailPinPurpose, Method, MethodType, TrustedDevice};
use super::notify::PlatformName;
use super::repository::MfaRepository;
use crate::shared::email_service::{EmailMessage, EmailService};
use crate::shared::encryption_service::EncryptionService;
use crate::shared::error::{PlatformError, Result};

/// Digits in an email PIN.
const EMAIL_PIN_LENGTH: u32 = 6;
/// An email PIN's lifetime (10 minutes).
const EMAIL_PIN_TTL_MINUTES: i64 = 10;
/// Wrong guesses before a PIN is burned.
const EMAIL_PIN_MAX_ATTEMPTS: i32 = 5;
/// Codes in a recovery set.
const RECOVERY_CODE_COUNT: usize = 10;

/// Why an enrolment step can't proceed (Go's sentinel errors).
#[derive(Debug)]
pub enum EnrollError {
    /// No encryption key (`FLOWCATALYST_APP_KEY`): TOTP secrets can't be kept.
    EncryptionUnavailable,
    /// The factor is already confirmed.
    AlreadyEnrolled,
    /// Confirming a factor that was never begun.
    NoPendingEnrollment,
    Other(PlatformError),
}

impl From<PlatformError> for EnrollError {
    fn from(e: PlatformError) -> Self {
        EnrollError::Other(e)
    }
}

/// What the SPA needs to show a TOTP set-up screen.
pub struct TotpEnrollment {
    /// Base32 shared secret, for manual entry.
    pub secret: String,
    /// The `otpauth://` URI.
    pub uri: String,
    /// The URI as a PNG data URI; empty if it couldn't be rendered.
    pub qr: String,
}

pub struct MfaService {
    pub repo: Arc<MfaRepository>,
    /// None: TOTP is unavailable (email PINs still work).
    pub encryption: Option<Arc<EncryptionService>>,
    pub email: Arc<dyn EmailService>,
    /// The authenticator-app issuer (the platform name).
    pub issuer: PlatformName,
}

impl MfaService {
    // ── status ──────────────────────────────────────────────────────────

    /// The user's confirmed factor types, oldest first.
    pub async fn confirmed_methods(&self, principal_id: &str) -> Result<Vec<MethodType>> {
        Ok(self
            .repo
            .find_methods(principal_id)
            .await?
            .into_iter()
            .filter(Method::is_confirmed)
            .map(|m| m.method)
            .collect())
    }

    pub async fn has_confirmed_method(&self, principal_id: &str) -> Result<bool> {
        Ok(!self.confirmed_methods(principal_id).await?.is_empty())
    }

    // ── TOTP enrolment ─────────────────────────────────────────────────

    /// Generate a secret, store it encrypted as an unconfirmed factor
    /// (replacing a previous unconfirmed attempt) and return the set-up
    /// data.
    pub async fn begin_totp_enrollment(
        &self,
        principal_id: &str,
        account: &str,
    ) -> std::result::Result<TotpEnrollment, EnrollError> {
        let enc = self
            .encryption
            .as_ref()
            .ok_or(EnrollError::EncryptionUnavailable)?;
        if self
            .repo
            .find_method(principal_id, MethodType::Totp)
            .await?
            .is_some_and(|m| m.is_confirmed())
        {
            return Err(EnrollError::AlreadyEnrolled);
        }
        let secret = crypto::new_totp_secret();
        let mut method = Method::new(principal_id, MethodType::Totp);
        method.secret_encrypted = Some(
            enc.encrypt(&secret)
                .map_err(|e| PlatformError::internal(format!("mfa: encrypt totp secret: {e}")))?,
        );
        self.repo.replace_pending_method(&method).await?;
        let issuer = self.issuer.resolve().await;
        let uri = crypto::totp_uri(&issuer, account, &secret);
        let qr = crypto::qr_data_uri(&uri).unwrap_or_default();
        Ok(TotpEnrollment { secret, uri, qr })
    }

    /// Check the first code against the pending secret and confirm the
    /// factor, spending that code's step so it can't be replayed at login.
    /// `Ok(false)` on a wrong code (the factor stays pending).
    pub async fn confirm_totp_enrollment(
        &self,
        principal_id: &str,
        code: &str,
    ) -> std::result::Result<bool, EnrollError> {
        let enc = self
            .encryption
            .as_ref()
            .ok_or(EnrollError::EncryptionUnavailable)?;
        let method = self
            .repo
            .find_method(principal_id, MethodType::Totp)
            .await?
            .filter(|m| m.secret_encrypted.is_some())
            .ok_or(EnrollError::NoPendingEnrollment)?;
        if method.is_confirmed() {
            return Err(EnrollError::AlreadyEnrolled);
        }
        let secret = decrypt(enc, &method)?;
        let Some(step) = crypto::validate_totp(&secret, code, Utc::now().timestamp()) else {
            return Ok(false);
        };
        if !self.repo.confirm_method(&method.id, Utc::now()).await? {
            // A concurrent confirm won.
            return Err(EnrollError::AlreadyEnrolled);
        }
        self.repo
            .claim_totp_step(&method.id, crypto::time_for_step(step))
            .await?;
        Ok(true)
    }

    // ── email-PIN enrolment ────────────────────────────────────────────

    /// Record an unconfirmed email factor and email a PIN proving inbox
    /// control.
    pub async fn begin_email_enrollment(
        &self,
        principal_id: &str,
        email: &str,
    ) -> std::result::Result<(), EnrollError> {
        match self
            .repo
            .find_method(principal_id, MethodType::EmailPin)
            .await?
        {
            Some(m) if m.is_confirmed() => return Err(EnrollError::AlreadyEnrolled),
            Some(_) => {}
            None => {
                self.repo
                    .insert_method_if_absent(&Method::new(principal_id, MethodType::EmailPin))
                    .await?
            }
        }
        self.issue_email_pin(principal_id, email, EmailPinPurpose::Enroll)
            .await?;
        Ok(())
    }

    /// Check the enrolment PIN and confirm the email factor.
    pub async fn confirm_email_enrollment(
        &self,
        principal_id: &str,
        code: &str,
    ) -> std::result::Result<bool, EnrollError> {
        if !self
            .verify_email_pin(principal_id, code, EmailPinPurpose::Enroll)
            .await?
        {
            return Ok(false);
        }
        let method = self
            .repo
            .find_method(principal_id, MethodType::EmailPin)
            .await?
            .ok_or(EnrollError::NoPendingEnrollment)?;
        if !method.is_confirmed() {
            self.repo.confirm_method(&method.id, Utc::now()).await?;
        }
        Ok(true)
    }

    // ── login challenge ────────────────────────────────────────────────

    pub async fn send_login_email_pin(&self, principal_id: &str, email: &str) -> Result<()> {
        self.issue_email_pin(principal_id, email, EmailPinPurpose::Login)
            .await
    }

    pub async fn verify_login_email_pin(&self, principal_id: &str, code: &str) -> Result<bool> {
        self.verify_email_pin(principal_id, code, EmailPinPurpose::Login)
            .await
    }

    /// Check a TOTP code for a confirmed factor. A time-step already spent
    /// (by this or a concurrent request) is refused: the step is claimed in
    /// one guarded statement, so each code signs in at most once.
    pub async fn verify_totp(&self, principal_id: &str, code: &str) -> Result<bool> {
        let Some(enc) = self.encryption.as_ref() else {
            return Err(PlatformError::internal(
                "mfa: encryption not configured (set FLOWCATALYST_APP_KEY)",
            ));
        };
        let Some(method) = self
            .repo
            .find_method(principal_id, MethodType::Totp)
            .await?
            .filter(|m| m.is_confirmed() && m.secret_encrypted.is_some())
        else {
            return Ok(false);
        };
        let secret = decrypt(enc, &method).map_err(|e| match e {
            EnrollError::Other(e) => e,
            _ => PlatformError::internal("mfa: decrypt totp secret"),
        })?;
        let Some(step) = crypto::validate_totp(&secret, code, Utc::now().timestamp()) else {
            return Ok(false);
        };
        self.repo
            .claim_totp_step(&method.id, crypto::time_for_step(step))
            .await
    }

    /// Spend a recovery code: `true` once, then the code is burned.
    pub async fn verify_recovery_code(&self, principal_id: &str, code: &str) -> Result<bool> {
        let hash = crypto::sha256_hex(&crypto::normalize_recovery_code(code));
        self.repo.consume_recovery_code(principal_id, &hash).await
    }

    // ── recovery codes ─────────────────────────────────────────────────

    /// Replace the user's recovery codes with a fresh set; the plaintext
    /// codes, shown once.
    pub async fn generate_recovery_codes(&self, principal_id: &str) -> Result<Vec<String>> {
        let codes: Vec<String> = (0..RECOVERY_CODE_COUNT)
            .map(|_| crypto::random_recovery_code())
            .collect();
        let hashes: Vec<String> = codes
            .iter()
            .map(|c| crypto::sha256_hex(&crypto::normalize_recovery_code(c)))
            .collect();
        self.repo
            .replace_recovery_codes(principal_id, &hashes)
            .await?;
        Ok(codes)
    }

    pub async fn remaining_recovery_codes(&self, principal_id: &str) -> Result<i64> {
        self.repo.count_unused_recovery_codes(principal_id).await
    }

    // ── removal / reset ────────────────────────────────────────────────

    pub async fn remove_method(&self, principal_id: &str, method: MethodType) -> Result<u64> {
        self.repo.delete_method(principal_id, method).await
    }

    /// Clear every factor, code, PIN and remembered device (admin reset,
    /// lost device).
    pub async fn reset_all(&self, principal_id: &str) -> Result<()> {
        self.repo.reset_all(principal_id).await
    }

    // ── trusted devices ────────────────────────────────────────────────

    /// Mint a remember-device token, store its hash and return the raw
    /// token for the cookie.
    pub async fn issue_trusted_device(
        &self,
        principal_id: &str,
        label: Option<&str>,
        ttl: Duration,
    ) -> Result<String> {
        let raw = crypto::random_token();
        self.repo
            .insert_trusted_device(
                principal_id,
                &crypto::sha256_hex(&raw),
                label,
                Utc::now() + ttl,
            )
            .await?;
        Ok(raw)
    }

    /// Whether `raw` is an unexpired remembered device of the user
    /// (stamping its use).
    pub async fn verify_trusted_device(&self, principal_id: &str, raw: &str) -> Result<bool> {
        if raw.is_empty() {
            return Ok(false);
        }
        self.repo
            .use_trusted_device(principal_id, &crypto::sha256_hex(raw))
            .await
    }

    pub async fn list_trusted_devices(&self, principal_id: &str) -> Result<Vec<TrustedDevice>> {
        self.repo.list_trusted_devices(principal_id).await
    }

    pub async fn revoke_trusted_device(&self, principal_id: &str, id: &str) -> Result<u64> {
        self.repo.delete_trusted_device(principal_id, id).await
    }

    pub async fn revoke_all_trusted_devices(&self, principal_id: &str) -> Result<()> {
        self.repo.delete_trusted_devices(principal_id).await
    }

    // ── email PINs ─────────────────────────────────────────────────────

    /// Replace outstanding PINs of `purpose` with a fresh one and email it.
    /// A delivery failure is returned: the user needs the PIN.
    async fn issue_email_pin(
        &self,
        principal_id: &str,
        email: &str,
        purpose: EmailPinPurpose,
    ) -> Result<()> {
        let pin = crypto::random_digits(EMAIL_PIN_LENGTH);
        self.repo
            .replace_email_pin(
                principal_id,
                purpose,
                &crypto::sha256_hex(&pin),
                Utc::now() + Duration::minutes(EMAIL_PIN_TTL_MINUTES),
            )
            .await?;
        let message = EmailMessage {
            to: email.to_string(),
            subject: "Your verification code".to_string(),
            html_body: format!(
                "<p>Your verification code is:</p>\
                 <p style=\"font-size:24px;font-weight:bold;letter-spacing:3px\">{pin}</p>\
                 <p>This code expires in {EMAIL_PIN_TTL_MINUTES} minutes.</p>\
                 <p>If you did not try to sign in, you can ignore this email.</p>"
            ),
            text_body: None,
        };
        self.email
            .send(&message)
            .await
            .map_err(|e| PlatformError::internal(format!("mfa: send email pin: {e}")))
    }

    /// Check `code` against the latest PIN of `purpose`. A correct code
    /// burns the PIN (once, whoever races for it); a wrong one counts an
    /// attempt and burns the PIN at the ceiling. Expired, absent and
    /// exhausted all read as a wrong code.
    async fn verify_email_pin(
        &self,
        principal_id: &str,
        code: &str,
        purpose: EmailPinPurpose,
    ) -> Result<bool> {
        let Some(pin) = self
            .repo
            .find_latest_email_pin(principal_id, purpose)
            .await?
        else {
            return Ok(false);
        };
        if pin.is_expired() || pin.attempts >= EMAIL_PIN_MAX_ATTEMPTS {
            let _ = self.repo.delete_email_pin(&pin.id).await;
            return Ok(false);
        }
        if crypto::constant_time_eq(&pin.pin_hash, &crypto::sha256_hex(code.trim())) {
            return self.repo.delete_email_pin(&pin.id).await;
        }
        if let Some(n) = self.repo.increment_email_pin_attempts(&pin.id).await? {
            if n >= EMAIL_PIN_MAX_ATTEMPTS {
                let _ = self.repo.delete_email_pin(&pin.id).await;
            }
        }
        Ok(false)
    }
}

fn decrypt(enc: &EncryptionService, method: &Method) -> std::result::Result<String, EnrollError> {
    let stored = method
        .secret_encrypted
        .as_deref()
        .ok_or(EnrollError::NoPendingEnrollment)?;
    enc.decrypt(stored).map_err(|e| {
        EnrollError::Other(PlatformError::internal(format!(
            "mfa: decrypt totp secret: {e}"
        )))
    })
}
