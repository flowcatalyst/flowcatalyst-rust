//! Verifies a Sigstore bundle v0.3 against an artifact digest: a
//! line-for-line port of Java's JDK-only
//! `platform/function/artifact/SignatureVerifier.java` (`0118cdca`).
//!
//! It establishes **who signed these bytes and when**. Every bundle runs
//! every step, fail closed; what it deliberately does not check (no SCT,
//! Rekor v1 only, no online lookups) is Java's spec `function-artifacts.md`
//! §4, kept identical so both hosts accept and reject the same bundles.

use chrono::{DateTime, TimeZone, Utc};
use der::asn1::ObjectIdentifier;
use der::Decode;
use serde_json::Value;
use sha2::{Digest as _, Sha256, Sha384, Sha512};
use x509_cert::ext::pkix::name::GeneralName;
use x509_cert::ext::pkix::{BasicConstraints, ExtendedKeyUsage, KeyUsage, SubjectAltName};
use x509_cert::spki::SubjectPublicKeyInfoOwned;
use x509_cert::Certificate;

use super::trust_root::{CertificateAuthority, TrustRoot};
use super::{Reason, Verification};
use crate::digest::{Digest, SignerIdentity};
use crate::java;

const MEDIA_TYPE: &str = "application/vnd.dev.sigstore.bundle.v0.3+json";
const CODE_SIGNING_EKU: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.6.1.5.5.7.3.3");
const OID_EKU: ObjectIdentifier = ObjectIdentifier::new_unwrap("2.5.29.37");
const OID_ISSUER: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.6.1.4.1.57264.1.8");
const OID_ISSUER_DEPRECATED: ObjectIdentifier =
    ObjectIdentifier::new_unwrap("1.3.6.1.4.1.57264.1.1");

const EC_PUBLIC_KEY: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.2.1");
const SECP256R1: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.3.1.7");
const SECP384R1: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.132.0.34");
const ECDSA_SHA256: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.4.3.2");
const ECDSA_SHA384: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.4.3.3");
const ECDSA_SHA512: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.4.3.4");

/// Extensions the JDK's PKIX validator processes; any other extension
/// marked critical fails the path, as it does there.
const KNOWN_CRITICAL: [ObjectIdentifier; 8] = [
    ObjectIdentifier::new_unwrap("2.5.29.15"), // key usage
    ObjectIdentifier::new_unwrap("2.5.29.19"), // basic constraints
    ObjectIdentifier::new_unwrap("2.5.29.17"), // subject alternative name
    ObjectIdentifier::new_unwrap("2.5.29.37"), // extended key usage
    ObjectIdentifier::new_unwrap("2.5.29.30"), // name constraints
    ObjectIdentifier::new_unwrap("2.5.29.32"), // certificate policies
    ObjectIdentifier::new_unwrap("2.5.29.33"), // policy mappings
    ObjectIdentifier::new_unwrap("2.5.29.36"), // policy constraints
];

#[derive(Debug, Clone)]
pub struct SignatureVerifier {
    trust_root: TrustRoot,
}

/// Where Java's code would have thrown a `RuntimeException` that only
/// `verify`'s outer catch handles.
struct Unexpected(String);

fn rejected(reason: Reason, detail: impl Into<String>) -> Verification {
    Verification::Rejected {
        reason,
        detail: detail.into(),
    }
}

impl SignatureVerifier {
    pub fn new(trust_root: TrustRoot) -> Self {
        Self { trust_root }
    }

    pub fn trust_root(&self) -> &TrustRoot {
        &self.trust_root
    }

    /// Never panics or errors for any input: every malformation is an
    /// explicit rejection, and a shape Java would only catch in its outer
    /// net is `MALFORMED_BUNDLE` here too.
    pub fn verify(&self, bundle_json: Option<&str>, digest: &Digest) -> Verification {
        match self.do_verify(bundle_json, digest) {
            Ok(v) => v,
            Err(Unexpected(e)) => {
                rejected(Reason::MalformedBundle, format!("unreadable bundle: {e}"))
            }
        }
    }

    fn do_verify(
        &self,
        bundle_json: Option<&str>,
        digest: &Digest,
    ) -> Result<Verification, Unexpected> {
        let Some(bundle_json) = bundle_json else {
            return Ok(rejected(Reason::MalformedBundle, "bundle is null"));
        };
        let root: Value = match serde_json::from_str(bundle_json) {
            Ok(v) => v,
            Err(e) => {
                return Ok(rejected(
                    Reason::MalformedBundle,
                    format!("bundle is not valid JSON: {e}"),
                ))
            }
        };
        if !root.is_object() {
            return Ok(rejected(
                Reason::MalformedBundle,
                "bundle is not a JSON object",
            ));
        }
        if root.get("dsseEnvelope").is_some() {
            return Ok(rejected(
                Reason::UnsupportedBundle,
                "DSSE envelope bundles are not supported",
            ));
        }

        let Some(Value::String(media_type)) = root.get("mediaType") else {
            return Ok(rejected(Reason::MalformedBundle, "missing mediaType"));
        };
        if media_type != MEDIA_TYPE {
            return Ok(rejected(
                Reason::UnsupportedBundle,
                format!("unsupported mediaType: {media_type}"),
            ));
        }

        let Some(vm @ Value::Object(_)) = root.get("verificationMaterial") else {
            return Ok(rejected(
                Reason::MalformedBundle,
                "missing verificationMaterial",
            ));
        };
        if vm.get("x509CertificateChain").is_some() || vm.get("publicKey").is_some() {
            return Ok(rejected(
                Reason::UnsupportedBundle,
                "only a single leaf certificate is supported, not a chain or a public-key hint",
            ));
        }
        let Some(cert_node @ Value::Object(_)) = vm.get("certificate") else {
            return Ok(rejected(
                Reason::MalformedBundle,
                "missing verificationMaterial.certificate",
            ));
        };
        let Some(Value::String(raw_bytes)) = cert_node.get("rawBytes") else {
            return Ok(rejected(
                Reason::MalformedBundle,
                "missing certificate.rawBytes",
            ));
        };
        let Ok(leaf_der) = java::b64_decode(raw_bytes) else {
            return Ok(rejected(
                Reason::MalformedBundle,
                "certificate.rawBytes is not valid base64",
            ));
        };
        let Ok(leaf) = Certificate::from_der(&leaf_der) else {
            return Ok(rejected(
                Reason::MalformedBundle,
                "certificate.rawBytes is not a valid X.509 certificate",
            ));
        };

        let entries = match vm.get("tlogEntries") {
            Some(Value::Array(items)) => items.as_slice(),
            _ => &[],
        };
        if entries.len() != 1 {
            return Ok(rejected(
                Reason::UnsupportedBundle,
                format!(
                    "expected exactly one tlogEntries entry, found {}",
                    entries.len()
                ),
            ));
        }
        let entry = &entries[0];
        let kind_version = entry.get("kindVersion");
        let kind = java::as_string(kind_version.and_then(|k| k.get("kind")), None);
        let version = java::as_string(kind_version.and_then(|k| k.get("version")), None);
        if kind.as_deref() != Some("hashedrekord") || version.as_deref() != Some("0.0.1") {
            return Ok(rejected(
                Reason::UnsupportedBundle,
                "unsupported tlog entry kind/version",
            ));
        }

        let promise = entry.get("inclusionPromise").filter(|v| v.is_object());
        let proof = entry.get("inclusionProof").filter(|v| v.is_object());
        let (Some(promise), Some(proof)) = (promise, proof) else {
            return Ok(rejected(
                Reason::TlogMissing,
                "tlog entry is missing inclusionPromise or inclusionProof",
            ));
        };
        let checkpoint = proof.get("checkpoint").filter(|v| v.is_object());
        let Some(checkpoint) =
            checkpoint.filter(|c| matches!(c.get("envelope"), Some(Value::String(_))))
        else {
            return Ok(rejected(
                Reason::TlogMissing,
                "inclusionProof is missing its checkpoint",
            ));
        };

        let Some(message_signature @ Value::Object(_)) = root.get("messageSignature") else {
            return Ok(rejected(
                Reason::MalformedBundle,
                "missing messageSignature",
            ));
        };
        let message_digest = message_signature.get("messageDigest");
        if java::as_string(message_digest.and_then(|m| m.get("algorithm")), None).as_deref()
            != Some("SHA2_256")
        {
            return Ok(rejected(
                Reason::MalformedBundle,
                "messageDigest.algorithm must be SHA2_256",
            ));
        }
        let Some(Value::String(digest_value)) = message_digest.and_then(|m| m.get("digest")) else {
            return Ok(rejected(
                Reason::MalformedBundle,
                "missing messageDigest.digest",
            ));
        };
        let Ok(bundle_digest) = java::b64_decode(digest_value) else {
            return Ok(rejected(
                Reason::MalformedBundle,
                "messageDigest.digest is not valid base64",
            ));
        };
        if bundle_digest.len() != 32 {
            return Ok(rejected(
                Reason::MalformedBundle,
                "messageDigest.digest must be 32 bytes",
            ));
        }
        let Some(Value::String(signature_b64)) = message_signature.get("signature") else {
            return Ok(rejected(
                Reason::MalformedBundle,
                "missing messageSignature.signature",
            ));
        };
        let Ok(signature) = java::b64_decode(signature_b64) else {
            return Ok(rejected(
                Reason::MalformedBundle,
                "messageSignature.signature is not valid base64",
            ));
        };

        // ---- step 1: digest ----
        let expected_digest = digest.bytes();
        if !constant_time_eq(&expected_digest, &bundle_digest) {
            return Ok(rejected(
                Reason::DigestMismatch,
                "bundle messageDigest does not match the expected artifact digest",
            ));
        }

        let integrated_secs =
            java::as_string(entry.get("integratedTime"), Some("")).unwrap_or_default();
        let log_index = java::as_string(entry.get("logIndex"), Some("")).unwrap_or_default();
        let (Ok(integrated_secs), Ok(entry_log_index)) =
            (integrated_secs.parse::<i64>(), log_index.parse::<i64>())
        else {
            return Ok(rejected(
                Reason::MalformedBundle,
                "integratedTime/logIndex are not numeric",
            ));
        };
        let integrated_time = Utc
            .timestamp_opt(integrated_secs, 0)
            .single()
            .ok_or_else(|| Unexpected(format!("integratedTime out of range: {integrated_secs}")))?;
        let key_id = java::as_string(entry.get("logId").and_then(|l| l.get("keyId")), Some(""))
            .unwrap_or_default();
        let Ok(log_id) = java::b64_decode(&key_id) else {
            return Ok(rejected(
                Reason::MalformedBundle,
                "logId.keyId is not valid base64",
            ));
        };

        // ---- step 2: the entry belongs to a pinned log ----
        let Some(tlog) = self
            .trust_root
            .tlogs
            .iter()
            .find(|t| t.key_id == log_id && t.contains_at(integrated_time))
        else {
            return Ok(rejected(
                Reason::TlogUnknownLog,
                "no pinned transparency-log key covers this entry's logId/integratedTime",
            ));
        };
        let Some(tlog_key) = EcKey::from_spki_der(&tlog.public_key_der) else {
            return Ok(rejected(
                Reason::TlogUnknownLog,
                "pinned transparency-log key is not a usable EC key",
            ));
        };

        let Some(body_b64) = java::as_string(entry.get("canonicalizedBody"), None) else {
            return Ok(rejected(
                Reason::MalformedBundle,
                "missing canonicalizedBody",
            ));
        };
        let Ok(canonicalized_body) = java::b64_decode(&body_b64) else {
            return Ok(rejected(
                Reason::MalformedBundle,
                "canonicalizedBody is not valid base64",
            ));
        };

        // ---- step 3: the entry is about this signature ----
        let body_text = java::utf8_lossy(&canonicalized_body);
        let body: Value = if body_text.trim().is_empty() {
            Value::Null // Jackson reads "" as a missing node; every field below is then absent
        } else {
            match serde_json::from_str(&body_text) {
                Ok(v) => v,
                Err(_) => {
                    return Ok(rejected(
                        Reason::TlogEntryMismatch,
                        "canonicalizedBody is not valid JSON",
                    ))
                }
            }
        };
        let spec = body.get("spec");
        let hash = spec.and_then(|s| s.get("data")).and_then(|d| d.get("hash"));
        let body_signature = spec.and_then(|s| s.get("signature"));
        let body_hash_value = java::as_string(hash.and_then(|h| h.get("value")), None);
        let body_signature_b64 =
            java::as_string(body_signature.and_then(|s| s.get("content")), None);
        let body_public_key_b64 = java::as_string(
            body_signature
                .and_then(|s| s.get("publicKey"))
                .and_then(|p| p.get("content")),
            None,
        );
        let hash_algorithm = java::as_string(hash.and_then(|h| h.get("algorithm")), None);
        let (Some(body_hash_value), Some(body_signature_b64), Some(body_public_key_b64)) =
            (body_hash_value, body_signature_b64, body_public_key_b64)
        else {
            return Ok(rejected(
                Reason::TlogEntryMismatch,
                "canonicalizedBody is missing the expected hashedrekord fields",
            ));
        };
        if hash_algorithm.as_deref() != Some("sha256") {
            return Ok(rejected(
                Reason::TlogEntryMismatch,
                "canonicalizedBody is missing the expected hashedrekord fields",
            ));
        }
        if digest.hex() != body_hash_value {
            return Ok(rejected(
                Reason::TlogEntryMismatch,
                "canonicalizedBody hash.value does not match the artifact digest",
            ));
        }
        let Ok(body_signature) = java::b64_decode(&body_signature_b64) else {
            return Ok(rejected(
                Reason::TlogEntryMismatch,
                "canonicalizedBody signature.content is not valid base64",
            ));
        };
        if body_signature != signature {
            return Ok(rejected(
                Reason::TlogEntryMismatch,
                "canonicalizedBody signature does not match the bundle's signature",
            ));
        }
        let body_leaf_der = java::b64_decode(&body_public_key_b64)
            .ok()
            .and_then(|pem| pem_to_der(&java::utf8_lossy(&pem)));
        let Some(body_leaf_der) = body_leaf_der else {
            return Ok(rejected(
                Reason::TlogEntryMismatch,
                "canonicalizedBody publicKey.content is not a PEM certificate",
            ));
        };
        if body_leaf_der != leaf_der {
            return Ok(rejected(
                Reason::TlogEntryMismatch,
                "canonicalizedBody publicKey does not match the bundle's certificate",
            ));
        }

        // ---- step 4: the signed entry timestamp authenticates integratedTime ----
        let Some(set_b64) = java::as_string(promise.get("signedEntryTimestamp"), None) else {
            return Ok(rejected(
                Reason::TlogMissing,
                "inclusionPromise is missing signedEntryTimestamp",
            ));
        };
        let canonical_set = format!(
            "{{\"body\":\"{body_b64}\",\"integratedTime\":{integrated_secs},\"logID\":\"{}\",\"logIndex\":{entry_log_index}}}",
            hex::encode(&log_id)
        );
        let Ok(set) = java::b64_decode(&set_b64) else {
            return Ok(rejected(
                Reason::MalformedBundle,
                "signedEntryTimestamp is not valid base64",
            ));
        };
        if !tlog_key.verify_prehash(&Sha256::digest(canonical_set.as_bytes()), &set) {
            return Ok(rejected(
                Reason::TlogPromiseInvalid,
                "signed entry timestamp does not verify under the pinned log key",
            ));
        }

        // ---- step 5: inclusion (audit path + checkpoint) ----
        if let Some(failure) = check_inclusion(proof, checkpoint, &canonicalized_body, &tlog_key) {
            return Ok(failure);
        }

        // ---- step 6: certificate chain, validity window, code-signing EKU ----
        if let Some(failure) = self.check_certificate(&leaf, &leaf_der, integrated_time) {
            return Ok(failure);
        }

        // ---- step 7: signature over the digest ----
        let spki = &leaf.tbs_certificate.subject_public_key_info;
        let Some(leaf_key @ EcKey::P256(_)) = EcKey::from_spki(spki) else {
            return Ok(rejected(
                Reason::UnsupportedBundle,
                "leaf certificate key is not P-256 EC",
            ));
        };
        if !leaf_key.verify_prehash(&expected_digest, &signature) {
            return Ok(rejected(
                Reason::BadSignature,
                "signature does not verify under the leaf certificate's key",
            ));
        }

        // ---- step 8: identity ----
        let Some(identity) = identity_of(&leaf)? else {
            return Ok(rejected(
                Reason::IdentityMissing,
                "issuer extension or a single URI/rfc822 SAN is missing",
            ));
        };
        Ok(Verification::Verified {
            signer: identity,
            signed_at: integrated_time,
        })
    }

    /// Step 6: every pinned CA whose window contains `integratedTime` is
    /// tried; `UNTRUSTED_CERTIFICATE` when none covers the time or none
    /// validates, `CERTIFICATE_NOT_VALID_AT_SIGNING` when a covering CA's
    /// chain is fine but the leaf's own window excludes the time.
    fn check_certificate(
        &self,
        leaf: &Certificate,
        leaf_der: &[u8],
        integrated_time: DateTime<Utc>,
    ) -> Option<Verification> {
        let candidates: Vec<&CertificateAuthority> = self
            .trust_root
            .cas
            .iter()
            .filter(|ca| ca.contains_at(integrated_time))
            .collect();
        if candidates.is_empty() {
            return Some(rejected(
                Reason::UntrustedCertificate,
                "no pinned certificate authority is active at integratedTime",
            ));
        }
        let mut trusted = false;
        let mut leaf_time_failure = false;
        for ca in candidates {
            match validate_chain(leaf, leaf_der, ca, integrated_time.timestamp()) {
                ChainOutcome::Trusted => {
                    trusted = true;
                    break;
                }
                ChainOutcome::LeafNotValidAtTime => leaf_time_failure = true,
                ChainOutcome::Untrusted => {}
            }
        }
        if !trusted {
            return Some(if leaf_time_failure {
                rejected(
                    Reason::CertificateNotValidAtSigning,
                    "leaf certificate's own validity window does not contain integratedTime",
                )
            } else {
                rejected(
                    Reason::UntrustedCertificate,
                    "leaf certificate does not chain to a pinned certificate authority",
                )
            });
        }
        let eku =
            extension(leaf, &OID_EKU).and_then(|(_, value)| ExtendedKeyUsage::from_der(value).ok());
        if !eku.is_some_and(|eku| eku.0.contains(&CODE_SIGNING_EKU)) {
            return Some(rejected(
                Reason::NotACodeSigningCertificate,
                "leaf certificate's extended key usage does not include code signing",
            ));
        }
        None
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

// ---- step 5: RFC 6962 audit path + signed-note checkpoint ----

/// `(logIndex, treeSize, rootHash, hashes)` of an inclusion proof.
type InclusionProof = (i64, i64, Vec<u8>, Vec<Vec<u8>>);

fn check_inclusion(
    proof: &Value,
    checkpoint: &Value,
    canonicalized_body: &[u8],
    tlog_key: &EcKey,
) -> Option<Verification> {
    let parsed = (|| -> Option<InclusionProof> {
        let log_index = java::as_string(proof.get("logIndex"), Some(""))
            .unwrap_or_default()
            .parse()
            .ok()?;
        let tree_size = java::as_string(proof.get("treeSize"), Some(""))
            .unwrap_or_default()
            .parse()
            .ok()?;
        let root_hash =
            java::b64_decode(&java::as_string(proof.get("rootHash"), Some("")).unwrap_or_default())
                .ok()?;
        let mut hashes = Vec::new();
        for hash in java::elements(proof.get("hashes")) {
            hashes.push(
                java::b64_decode(&java::as_string(Some(hash), Some("")).unwrap_or_default())
                    .ok()?,
            );
        }
        Some((log_index, tree_size, root_hash, hashes))
    })();
    let Some((log_index, tree_size, root_hash, hashes)) = parsed else {
        return Some(rejected(
            Reason::TlogInclusionInvalid,
            "inclusionProof fields are malformed",
        ));
    };

    let mut leaf_input = Vec::with_capacity(canonicalized_body.len() + 1);
    leaf_input.push(0x00);
    leaf_input.extend_from_slice(canonicalized_body);
    let leaf_hash = Sha256::digest(&leaf_input).to_vec();
    let Some(computed_root) = root_from_inclusion_proof(log_index, tree_size, &hashes, leaf_hash)
    else {
        return Some(rejected(
            Reason::TlogInclusionInvalid,
            "audit path is too short for the proof",
        ));
    };
    if computed_root != root_hash {
        return Some(rejected(
            Reason::TlogInclusionInvalid,
            "audit path does not produce the checkpoint's root hash",
        ));
    }

    let envelope = java::as_string(checkpoint.get("envelope"), Some("")).unwrap_or_default();
    let note = match parse_signed_note(&envelope) {
        Ok(note) => note,
        Err(e) => {
            return Some(rejected(
                Reason::TlogCheckpointInvalid,
                format!("checkpoint envelope is malformed: {e}"),
            ))
        }
    };
    if note.size != tree_size || note.root_hash != root_hash {
        return Some(rejected(
            Reason::TlogCheckpointInvalid,
            "checkpoint size/root do not match the inclusion proof",
        ));
    }
    let body_hash = Sha256::digest(note.body.as_bytes());
    if !note
        .signatures
        .iter()
        .any(|s| tlog_key.verify_prehash(&body_hash, s))
    {
        return Some(rejected(
            Reason::TlogCheckpointInvalid,
            "no checkpoint signature verifies under the pinned log key",
        ));
    }
    None
}

/// The RFC 6962 client-side audit-path algorithm, in Java's `long`
/// arithmetic. `None` where Java indexes past the end of the proof.
fn root_from_inclusion_proof(
    leaf_index: i64,
    tree_size: i64,
    proof: &[Vec<u8>],
    leaf_hash: Vec<u8>,
) -> Option<Vec<u8>> {
    let mut hash = leaf_hash;
    let mut node = leaf_index;
    let mut last_node = tree_size.wrapping_sub(1);
    let mut i = 0usize;
    while last_node > 0 {
        if node % 2 == 1 {
            hash = hash_children(proof.get(i)?, &hash);
            i += 1;
        } else if node < last_node {
            hash = hash_children(&hash, proof.get(i)?);
            i += 1;
        }
        node /= 2;
        last_node /= 2;
    }
    Some(hash)
}

fn hash_children(left: &[u8], right: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update([0x01]);
    hasher.update(left);
    hasher.update(right);
    hasher.finalize().to_vec()
}

struct SignedNote {
    body: String,
    size: i64,
    root_hash: Vec<u8>,
    signatures: Vec<Vec<u8>>,
}

/// Java's `String.split("\n")`: trailing empty strings dropped.
fn java_split_lines(s: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = s.split('\n').collect();
    while parts.len() > 1 && parts.last() == Some(&"") {
        parts.pop();
    }
    if parts.len() == 1 && parts[0].is_empty() && !s.is_empty() {
        parts.clear();
    }
    parts
}

fn parse_signed_note(envelope: &str) -> Result<SignedNote, String> {
    let blank = envelope
        .find("\n\n")
        .ok_or("no blank line separating the checkpoint body from its signatures")?;
    let body = &envelope[..blank + 1];
    let body_lines = java_split_lines(body);
    if body_lines.len() < 3 {
        return Err("checkpoint body has fewer than three lines".into());
    }
    let size = body_lines[1]
        .trim()
        .parse::<i64>()
        .map_err(|_| "checkpoint size/root line is malformed")?;
    let root_hash = java::b64_decode(body_lines[2].trim())
        .map_err(|_| "checkpoint size/root line is malformed")?;
    let mut signatures = Vec::new();
    for line in java_split_lines(&envelope[blank + 2..]) {
        if line.is_empty() {
            continue;
        }
        let rest = line
            .strip_prefix("\u{2014} ")
            .ok_or("malformed checkpoint signature line")?;
        let space = rest
            .find(' ')
            .ok_or("malformed checkpoint signature line")?;
        let raw = java::b64_decode(&rest[space + 1..]).map_err(|e| format!("bad base64: {e}"))?;
        if raw.len() <= 4 {
            return Err("checkpoint signature is too short for its 4-byte key hint".into());
        }
        signatures.push(raw[4..].to_vec());
    }
    if signatures.is_empty() {
        return Err("checkpoint has no signature lines".into());
    }
    Ok(SignedNote {
        body: body.to_owned(),
        size,
        root_hash,
        signatures,
    })
}

// ---- step 6: a PKIX subset, as the JDK's validator applies it here ----

#[derive(Debug, PartialEq, Eq)]
enum ChainOutcome {
    Trusted,
    LeafNotValidAtTime,
    Untrusted,
}

/// The JDK's `CertPathValidator("PKIX")` with revocation off, the date set
/// to `integratedTime` and the CA's last certificate as the trust anchor:
/// processed from the anchor down, each certificate is checked for CA
/// constraints (non-leaf), signature, validity at the date, name chaining
/// and unrecognised critical extensions, in that order. The anchor's own
/// validity is not checked, as in the JDK.
fn validate_chain(
    leaf: &Certificate,
    leaf_der: &[u8],
    ca: &CertificateAuthority,
    at: i64,
) -> ChainOutcome {
    let mut chain = Vec::new();
    for der in &ca.cert_chain_der {
        match Certificate::from_der(der) {
            Ok(cert) => chain.push((cert, der.as_slice())),
            Err(_) => return ChainOutcome::Untrusted,
        }
    }
    let (anchor, _) = chain.pop().expect("a CA chain is never empty");
    // path[0] is the leaf; the anchor signs the last element.
    let mut path: Vec<(&Certificate, &[u8])> = vec![(leaf, leaf_der)];
    path.extend(chain.iter().map(|(cert, der)| (cert, *der)));

    let mut issuer_key = &anchor.tbs_certificate.subject_public_key_info;
    let mut issuer_name = &anchor.tbs_certificate.subject;
    let mut max_path_length = path.len() as i64;
    for index in (0..path.len()).rev() {
        let (cert, cert_der) = path[index];
        let tbs = &cert.tbs_certificate;
        if index > 0 {
            // an intermediate: must be a CA, allowed to sign certificates, within pathLen
            let Some((_, bc)) = extension(cert, &ObjectIdentifier::new_unwrap("2.5.29.19")) else {
                return ChainOutcome::Untrusted;
            };
            let Ok(bc) = BasicConstraints::from_der(bc) else {
                return ChainOutcome::Untrusted;
            };
            if !bc.ca {
                return ChainOutcome::Untrusted;
            }
            if let Some((_, ku)) = extension(cert, &ObjectIdentifier::new_unwrap("2.5.29.15")) {
                match KeyUsage::from_der(ku) {
                    Ok(ku) if ku.key_cert_sign() => {}
                    _ => return ChainOutcome::Untrusted,
                }
            }
            let self_issued = tbs.issuer == tbs.subject;
            if !self_issued {
                if max_path_length <= 0 {
                    return ChainOutcome::Untrusted;
                }
                max_path_length -= 1;
            }
            if let Some(limit) = bc.path_len_constraint {
                max_path_length = max_path_length.min(i64::from(limit));
            }
        } else if max_path_length < 0 {
            return ChainOutcome::Untrusted;
        }
        if !verify_certificate_signature(cert, cert_der, issuer_key) {
            return ChainOutcome::Untrusted;
        }
        let not_before = tbs.validity.not_before.to_unix_duration().as_secs() as i64;
        let not_after = tbs.validity.not_after.to_unix_duration().as_secs() as i64;
        if at < not_before || at > not_after {
            return if index == 0 {
                ChainOutcome::LeafNotValidAtTime
            } else {
                ChainOutcome::Untrusted
            };
        }
        if &tbs.issuer != issuer_name {
            return ChainOutcome::Untrusted;
        }
        let unknown_critical = tbs
            .extensions
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .any(|e| e.critical && !KNOWN_CRITICAL.contains(&e.extn_id));
        if unknown_critical {
            return ChainOutcome::Untrusted;
        }
        issuer_key = &tbs.subject_public_key_info;
        issuer_name = &tbs.subject;
    }
    ChainOutcome::Trusted
}

fn verify_certificate_signature(
    cert: &Certificate,
    cert_der: &[u8],
    issuer: &SubjectPublicKeyInfoOwned,
) -> bool {
    let Some(key) = EcKey::from_spki(issuer) else {
        return false; // Sigstore CAs are ECDSA; anything else is untrusted here
    };
    let Some(tbs) = raw_tbs(cert_der) else {
        return false;
    };
    let Some(signature) = cert.signature.as_bytes() else {
        return false;
    };
    let oid = cert.signature_algorithm.oid;
    let prehash: Vec<u8> = if oid == ECDSA_SHA256 {
        Sha256::digest(tbs).to_vec()
    } else if oid == ECDSA_SHA384 {
        Sha384::digest(tbs).to_vec()
    } else if oid == ECDSA_SHA512 {
        Sha512::digest(tbs).to_vec()
    } else {
        return false;
    };
    key.verify_prehash(&prehash, signature)
}

/// The TBS bytes exactly as they appear in the certificate, so signature
/// checks never depend on a re-encoding.
fn raw_tbs(der: &[u8]) -> Option<&[u8]> {
    let outer = read_header(der, 0)?;
    let inner = read_header(der, outer.0)?;
    der.get(outer.0..outer.0 + inner.0 + inner.1)
}

/// `(header length, content length)` of the DER TLV at `offset`.
fn read_header(der: &[u8], offset: usize) -> Option<(usize, usize)> {
    let length_byte = *der.get(offset + 1)?;
    if length_byte & 0x80 == 0 {
        return Some((2, usize::from(length_byte)));
    }
    let count = usize::from(length_byte & 0x7F);
    let mut length = 0usize;
    for i in 0..count {
        length = (length << 8) | usize::from(*der.get(offset + 2 + i)?);
    }
    Some((2 + count, length))
}

pub(crate) enum EcKey {
    P256(p256::ecdsa::VerifyingKey),
    P384(p384::ecdsa::VerifyingKey),
}

impl EcKey {
    pub(crate) fn from_spki_der(der: &[u8]) -> Option<Self> {
        Self::from_spki(&SubjectPublicKeyInfoOwned::from_der(der).ok()?)
    }

    fn from_spki(spki: &SubjectPublicKeyInfoOwned) -> Option<Self> {
        if spki.algorithm.oid != EC_PUBLIC_KEY {
            return None;
        }
        let curve: ObjectIdentifier = spki.algorithm.parameters.as_ref()?.decode_as().ok()?;
        let point = spki.subject_public_key.as_bytes()?;
        if curve == SECP256R1 {
            p256::ecdsa::VerifyingKey::from_sec1_bytes(point)
                .ok()
                .map(EcKey::P256)
        } else if curve == SECP384R1 {
            p384::ecdsa::VerifyingKey::from_sec1_bytes(point)
                .ok()
                .map(EcKey::P384)
        } else {
            None
        }
    }

    /// ECDSA over an already-computed hash, DER signature, as the JDK's
    /// `SHA256withECDSA` (after hashing) and `NONEwithECDSA` verify.
    pub(crate) fn verify_prehash(&self, prehash: &[u8], signature_der: &[u8]) -> bool {
        use p256::ecdsa::signature::hazmat::PrehashVerifier;
        match self {
            EcKey::P256(key) => p256::ecdsa::Signature::from_der(signature_der)
                .is_ok_and(|sig| key.verify_prehash(prehash, &sig).is_ok()),
            EcKey::P384(key) => p384::ecdsa::Signature::from_der(signature_der)
                .is_ok_and(|sig| key.verify_prehash(prehash, &sig).is_ok()),
        }
    }
}

fn pem_to_der(pem: &str) -> Option<Vec<u8>> {
    let stripped: String = pem
        .replace("-----BEGIN CERTIFICATE-----", "")
        .replace("-----END CERTIFICATE-----", "")
        .chars()
        .filter(|c| !matches!(c, ' ' | '\t' | '\n' | '\u{000B}' | '\u{000C}' | '\r'))
        .collect();
    java::b64_decode(&stripped).ok()
}

/// The extension `oid`, as `(critical, extnValue content)`.
fn extension<'a>(cert: &'a Certificate, oid: &ObjectIdentifier) -> Option<(bool, &'a [u8])> {
    cert.tbs_certificate
        .extensions
        .as_deref()?
        .iter()
        .find(|e| &e.extn_id == oid)
        .map(|e| (e.critical, e.extn_value.as_bytes()))
}

// ---- step 8: identity ----

fn identity_of(leaf: &Certificate) -> Result<Option<SignerIdentity>, Unexpected> {
    let issuer = match extension(leaf, &OID_ISSUER) {
        Some((_, value)) => {
            let inner = read_tlv(value)?;
            if inner.0 == 0x0C {
                Some(java::utf8_lossy(&inner.1))
            } else {
                None
            }
        }
        None => None,
    };
    let issuer = match issuer {
        Some(issuer) => Some(issuer),
        None => extension(leaf, &OID_ISSUER_DEPRECATED).map(|(_, value)| java::utf8_lossy(value)),
    };
    let subject = single_uri_or_email_san(leaf);
    Ok(match (issuer, subject) {
        (Some(issuer), Some(subject)) => Some(SignerIdentity { issuer, subject }),
        _ => None,
    })
}

fn single_uri_or_email_san(leaf: &Certificate) -> Option<String> {
    let (_, value) = extension(leaf, &ObjectIdentifier::new_unwrap("2.5.29.17"))?;
    let san = SubjectAltName::from_der(value).ok()?;
    if san.0.len() != 1 {
        return None;
    }
    match &san.0[0] {
        GeneralName::UniformResourceIdentifier(uri) => Some(uri.to_string()),
        GeneralName::Rfc822Name(email) => Some(email.to_string()),
        _ => None,
    }
}

/// Java's minimal `readTlv`, including `Arrays.copyOfRange`'s zero padding
/// past the end; a header that runs off the input is where Java throws.
fn read_tlv(der: &[u8]) -> Result<(u8, Vec<u8>), Unexpected> {
    let truncated = || Unexpected("truncated DER in a certificate extension".into());
    let tag = *der.first().ok_or_else(truncated)?;
    let (header, length) = read_header(der, 0).ok_or_else(truncated)?;
    if header > der.len() {
        return Err(truncated());
    }
    let mut value = vec![0u8; length];
    let available = der.len().saturating_sub(header).min(length);
    value[..available].copy_from_slice(&der[header..header + available]);
    Ok((tag, value))
}
