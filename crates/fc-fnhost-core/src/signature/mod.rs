//! Artifact signature policy and Sigstore verification (Java
//! `platform/function/artifact/{Signatures,SignaturesMode,Verification,
//! SignatureVerifier,TrustRoot}.java`).
//!
//! **Why a port, not `sigstore-rs`.** The host must accept and reject exactly
//! the bundles the Java host does, with the same reason codes
//! (`SIGNATURE:<reason>`). `sigstore-rs` verifies embedded SCTs against CT
//! logs and speaks its own error vocabulary; Java deliberately skips SCTs
//! (spec `function-artifacts.md` §4), so Java's own fixture bundles (which
//! carry no SCT) would be rejected, and no reason could be mapped back to
//! Java's. This module is therefore a direct port on RustCrypto (`p256`,
//! `p384`, `x509-cert`), tested against Java's golden fixture and every
//! broken variant `SignatureVerifierTest` builds.

mod trust_root;
mod verifier;

use std::fmt;
use std::sync::Arc;

use chrono::{DateTime, Utc};

pub use trust_root::{CertificateAuthority, TransparencyLog, TrustRoot};
pub use verifier::SignatureVerifier;

use crate::digest::SignerIdentity;

/// The outcome of [`SignatureVerifier::verify`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verification {
    /// `signed_at` is the tlog entry's `integratedTime`.
    Verified {
        signer: SignerIdentity,
        signed_at: DateTime<Utc>,
    },
    /// `detail` is diagnostic text for logs, never used for control flow.
    Rejected { reason: Reason, detail: String },
}

/// Every way verification fails; the names are Java's `Verification.Reason`
/// constants, which the heartbeat carries as `SIGNATURE:<NAME>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reason {
    MalformedBundle,
    UnsupportedBundle,
    DigestMismatch,
    BadSignature,
    UntrustedCertificate,
    CertificateNotValidAtSigning,
    NotACodeSigningCertificate,
    IdentityMissing,
    TlogMissing,
    TlogUnknownLog,
    TlogEntryMismatch,
    TlogPromiseInvalid,
    TlogInclusionInvalid,
    TlogCheckpointInvalid,
}

impl Reason {
    pub fn name(self) -> &'static str {
        match self {
            Reason::MalformedBundle => "MALFORMED_BUNDLE",
            Reason::UnsupportedBundle => "UNSUPPORTED_BUNDLE",
            Reason::DigestMismatch => "DIGEST_MISMATCH",
            Reason::BadSignature => "BAD_SIGNATURE",
            Reason::UntrustedCertificate => "UNTRUSTED_CERTIFICATE",
            Reason::CertificateNotValidAtSigning => "CERTIFICATE_NOT_VALID_AT_SIGNING",
            Reason::NotACodeSigningCertificate => "NOT_A_CODE_SIGNING_CERTIFICATE",
            Reason::IdentityMissing => "IDENTITY_MISSING",
            Reason::TlogMissing => "TLOG_MISSING",
            Reason::TlogUnknownLog => "TLOG_UNKNOWN_LOG",
            Reason::TlogEntryMismatch => "TLOG_ENTRY_MISMATCH",
            Reason::TlogPromiseInvalid => "TLOG_PROMISE_INVALID",
            Reason::TlogInclusionInvalid => "TLOG_INCLUSION_INVALID",
            Reason::TlogCheckpointInvalid => "TLOG_CHECKPOINT_INVALID",
        }
    }
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The raw `FC_FN_SIGNATURES` setting. Anything but `off` (trimmed,
/// case-insensitive), including a typo, is `Required`: a typo can never turn
/// signatures off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignaturesMode {
    Required,
    Off,
}

impl SignaturesMode {
    pub fn parse(raw: &str) -> Self {
        if raw.trim().eq_ignore_ascii_case("off") {
            SignaturesMode::Off
        } else {
            SignaturesMode::Required
        }
    }
}

/// The signature policy, chosen once at start-up.
#[derive(Clone)]
pub enum Signatures {
    /// Every artifact must carry a bundle that verifies and whose signer
    /// equals the one the platform recorded at publish.
    Required(Arc<SignatureVerifier>),
    /// Fetch and digest only. Reachable only in dev mode.
    Off,
}

impl fmt::Debug for Signatures {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Signatures::Required(_) => f.write_str("Required"),
            Signatures::Off => f.write_str("Off"),
        }
    }
}

impl Signatures {
    /// The one resolution rule (Java `Signatures.resolve(mode, devMode,
    /// trustRootPath)`): `Off` is refused outside dev mode, and a blank
    /// `trust_root_path` means the committed public-good root. The trust
    /// root is only read for `Required`.
    pub fn resolve(
        mode: SignaturesMode,
        dev_mode: bool,
        trust_root_path: &str,
    ) -> Result<Self, String> {
        match mode {
            SignaturesMode::Off if dev_mode => Ok(Signatures::Off),
            SignaturesMode::Off => Err(
                "FC_FN_SIGNATURES=off requires FLOWCATALYST_DEV_MODE=true; refusing to start \
                 with function-publish signature verification disabled outside dev mode"
                    .to_owned(),
            ),
            SignaturesMode::Required => {
                let root = if crate::java::is_blank(trust_root_path) {
                    TrustRoot::sigstore_public_good()
                } else {
                    TrustRoot::from_file(std::path::Path::new(trust_root_path)).map_err(|_| {
                        format!("FC_FN_TRUST_ROOT points at a file that could not be read: '{trust_root_path}'")
                    })?
                };
                Ok(Signatures::Required(Arc::new(SignatureVerifier::new(root))))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_defaults_to_required() {
        assert_eq!(SignaturesMode::parse(""), SignaturesMode::Required);
        assert_eq!(SignaturesMode::parse("of"), SignaturesMode::Required);
        assert_eq!(SignaturesMode::parse(" OFF "), SignaturesMode::Off);
    }

    #[test]
    fn off_needs_dev_mode() {
        let err = Signatures::resolve(SignaturesMode::Off, false, "").unwrap_err();
        assert!(err.contains("FC_FN_SIGNATURES") && err.contains("FLOWCATALYST_DEV_MODE"));
        assert!(matches!(
            Signatures::resolve(SignaturesMode::Off, true, ""),
            Ok(Signatures::Off)
        ));
        assert!(matches!(
            Signatures::resolve(SignaturesMode::Required, false, ""),
            Ok(Signatures::Required(_))
        ));
    }

    #[test]
    fn unreadable_trust_root_names_the_variable() {
        let err = Signatures::resolve(SignaturesMode::Required, false, "/nonexistent/root.json")
            .unwrap_err();
        assert!(err.contains("FC_FN_TRUST_ROOT"));
    }
}
