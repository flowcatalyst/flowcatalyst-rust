//! The Sigstore certificate authorities and transparency-log keys the
//! verifier trusts, each with a validity window (Java
//! `platform/function/artifact/TrustRoot.java`). Parsed from a Sigstore
//! `trusted_root.json`; `ctlogs` and `timestampAuthorities` are read past:
//! the verifier does no SCT check and needs no timestamp authority.

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::java;

/// The public-good `trusted_root.json`, byte-identical to Java's
/// `server/src/main/resources/function/sigstore-trusted-root.json`
/// (`sigstore/root-signing` at `c9bda74a`). Refreshing it is a manual,
/// reviewed change: there is no TUF client.
const PUBLIC_GOOD: &str = include_str!("../../resources/sigstore-trusted-root.json");

#[derive(Debug, Clone)]
pub struct TrustRoot {
    pub cas: Vec<CertificateAuthority>,
    pub tlogs: Vec<TransparencyLog>,
}

/// One Fulcio generation: the chain from (excluding) the leaf up to and
/// including the self-signed root, in DER, and its active window.
#[derive(Debug, Clone)]
pub struct CertificateAuthority {
    pub cert_chain_der: Vec<Vec<u8>>,
    pub valid_from: DateTime<Utc>,
    pub valid_until: Option<DateTime<Utc>>,
}

/// One Rekor log key: its DER SubjectPublicKeyInfo, the `logId.keyId`
/// Sigstore computed for it, and its active window.
#[derive(Debug, Clone)]
pub struct TransparencyLog {
    pub key_id: Vec<u8>,
    pub public_key_der: Vec<u8>,
    pub valid_from: DateTime<Utc>,
    pub valid_until: Option<DateTime<Utc>>,
}

fn contains(from: DateTime<Utc>, until: Option<DateTime<Utc>>, time: DateTime<Utc>) -> bool {
    time >= from && until.is_none_or(|u| time <= u)
}

impl CertificateAuthority {
    pub fn contains_at(&self, time: DateTime<Utc>) -> bool {
        contains(self.valid_from, self.valid_until, time)
    }
}

impl TransparencyLog {
    pub fn contains_at(&self, time: DateTime<Utc>) -> bool {
        contains(self.valid_from, self.valid_until, time)
    }
}

impl TrustRoot {
    /// The committed Sigstore public-good root.
    pub fn sigstore_public_good() -> Self {
        Self::parse(PUBLIC_GOOD).expect("the committed public-good trust root parses")
    }

    /// An operator-supplied `trusted_root.json` (`FC_FN_TRUST_ROOT`).
    pub fn from_file(path: &std::path::Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read trust root file {}: {e}", path.display()))?;
        Self::parse(&text)
    }

    /// Reads a Sigstore `trusted_root.json`. Fails fast on a malformed
    /// document: this is the operator's own configuration, not attacker input.
    pub fn parse(json: &str) -> Result<Self, String> {
        let root: Value = serde_json::from_str(json)
            .map_err(|e| format!("trusted_root.json is not valid JSON: {e}"))?;
        let mut cas = Vec::new();
        for ca in java::elements(root.get("certificateAuthorities")) {
            let mut chain = Vec::new();
            for cert in java::elements(ca.get("certChain").and_then(|c| c.get("certificates"))) {
                chain.push(decode(cert.get("rawBytes"))?);
            }
            if chain.is_empty() {
                return Err("certChainDer must not be empty".into());
            }
            let valid_for = ca.get("validFor");
            cas.push(CertificateAuthority {
                cert_chain_der: chain,
                valid_from: instant(valid_for.and_then(|v| v.get("start")))?,
                valid_until: optional_instant(valid_for.and_then(|v| v.get("end")))?,
            });
        }
        let mut tlogs = Vec::new();
        for tlog in java::elements(root.get("tlogs")) {
            let key = tlog.get("publicKey");
            let valid_for = key.and_then(|k| k.get("validFor"));
            tlogs.push(TransparencyLog {
                key_id: decode(tlog.get("logId").and_then(|l| l.get("keyId")))?,
                public_key_der: decode(key.and_then(|k| k.get("rawBytes")))?,
                valid_from: instant(valid_for.and_then(|v| v.get("start")))?,
                valid_until: optional_instant(valid_for.and_then(|v| v.get("end")))?,
            });
        }
        Ok(Self { cas, tlogs })
    }
}

fn decode(node: Option<&Value>) -> Result<Vec<u8>, String> {
    let text = java::as_string(node, None).ok_or("trusted_root.json: missing base64 field")?;
    java::b64_decode(&text).map_err(|e| format!("trusted_root.json: bad base64: {e}"))
}

fn instant(node: Option<&Value>) -> Result<DateTime<Utc>, String> {
    let text = java::as_string(node, None).ok_or("trusted_root.json: missing validFor.start")?;
    DateTime::parse_from_rfc3339(&text)
        .map(|t| t.with_timezone(&Utc))
        .map_err(|e| format!("trusted_root.json: bad instant {text:?}: {e}"))
}

fn optional_instant(node: Option<&Value>) -> Result<Option<DateTime<Utc>>, String> {
    match node {
        Some(Value::String(_)) => instant(node).map(Some),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_good_root_parses() {
        let root = TrustRoot::sigstore_public_good();
        assert!(!root.cas.is_empty());
        assert!(!root.tlogs.is_empty());
    }
}
