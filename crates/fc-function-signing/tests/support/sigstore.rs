//! A port of Java's `TestSigstore` (server test sources, `0118cdca`): a
//! self-contained miniature Sigstore ecosystem. A P-256 root CA and a leaf
//! it signs (built with `x509-cert` rather than `keytool`, with the same
//! extensions), a P-256 log key, and a bundle builder that produces a
//! correct v0.3 bundle over an RFC 6962 tree, with every broken variant
//! Java's `SignatureVerifierTest` needs as a deliberate divergence.

#![allow(dead_code)]

use std::str::FromStr;
use std::time::Duration;

use base64::Engine;
use chrono::{DateTime, Utc};
use der::asn1::{BitString, Ia5String, ObjectIdentifier, OctetString, UtcTime};
use der::{Decode, Encode};
use fc_function_signing::signature::{CertificateAuthority, TransparencyLog, TrustRoot};
use p256::ecdsa::signature::hazmat::PrehashSigner;
use p256::ecdsa::{Signature, SigningKey};
use p256::pkcs8::EncodePublicKey;
use rand_core::{OsRng, RngCore};
use serde_json::json;
use sha2::{Digest, Sha256, Sha384};
use x509_cert::certificate::{TbsCertificate, Version};
use x509_cert::ext::pkix::name::GeneralName;
use x509_cert::ext::pkix::{
    BasicConstraints, ExtendedKeyUsage, KeyUsage, KeyUsages, SubjectAltName,
};
use x509_cert::ext::Extension;
use x509_cert::name::Name;
use x509_cert::serial_number::SerialNumber;
use x509_cert::spki::{AlgorithmIdentifierOwned, SubjectPublicKeyInfoOwned};
use x509_cert::time::{Time, Validity};
use x509_cert::Certificate;

pub const ISSUER_OID: &str = "1.3.6.1.4.1.57264.1.8";
pub const ISSUER_OID_DEPRECATED: &str = "1.3.6.1.4.1.57264.1.1";
pub const LOG_ORIGIN: &str = "test.rekor.local - 1";

/// A fixed RSA-2048 SubjectPublicKeyInfo: the leaf key for the "non-EC key
/// is unsupported" case. Only its type matters; nothing is ever signed with it.
const RSA_SPKI_B64: &str =
    "MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAyEaBTQ+Qn0FzzwD1IfGrQUlsowrIQDEHkc+VOHO16IU4j0Vqz5hQuLycguzco2Ju+f9LF5DUIDz4EcB04VxiRghJIjsgB5GaxwV2x2sewGjGw9rgfd6YaVC+8+qSxLBH977jGoMfJzeBnd0sAJbAAI9ez8qQlMuJTWfl0e0IXaeO7Qr0pLbyUA6p00Yvp6CB80vd991UK14htaJzQx7zYsMw4wQvbP+DaHf61kVUcHMV4OI2hKSroilCDjYGs4x+bg7ZaF1oNbJ9T7qvxGCZ095p/mLaOENdluF0cKFXtkfVzK67wD30bqJhSP6OIdFvuGaYsxLKCKuwRaxxoV8B1QIDAQAB";

pub fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub fn random_bytes(n: usize) -> Vec<u8> {
    let mut out = vec![0u8; n];
    OsRng.fill_bytes(&mut out);
    out
}

pub fn fresh_ec_key() -> SigningKey {
    SigningKey::random(&mut OsRng)
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LeafKey {
    Ec,
    Rsa,
}

/// What to bake into the leaf; `None` omits that extension.
#[derive(Clone)]
pub struct LeafSpec {
    pub not_before: DateTime<Utc>,
    pub not_after: DateTime<Utc>,
    pub code_signing_eku: bool,
    /// `("uri" | "email", value)` entries; empty omits the extension.
    pub sans: Vec<(&'static str, String)>,
    pub issuer: Option<(&'static str, String)>,
    pub key: LeafKey,
    /// The issuer extension's content, verbatim (not even a TLV).
    pub raw_issuer_extension: Option<Vec<u8>>,
}

impl LeafSpec {
    pub fn valid(not_before: DateTime<Utc>, not_after: DateTime<Utc>) -> Self {
        Self {
            not_before,
            not_after,
            code_signing_eku: true,
            sans: vec![("uri", "https://example.test/workflow.yml".into())],
            issuer: Some((ISSUER_OID, "https://example.test/issuer".into())),
            key: LeafKey::Ec,
            raw_issuer_extension: None,
        }
    }
    pub fn without_eku(mut self) -> Self {
        self.code_signing_eku = false;
        self
    }
    pub fn with_sans(mut self, sans: Vec<(&'static str, String)>) -> Self {
        self.sans = sans;
        self
    }
    pub fn with_san(self, kind: &'static str, value: &str) -> Self {
        self.with_sans(vec![(kind, value.to_owned())])
    }
    pub fn without_san(self) -> Self {
        self.with_sans(Vec::new())
    }
    pub fn with_issuer(mut self, oid: &'static str, value: &str) -> Self {
        self.issuer = Some((oid, value.to_owned()));
        self
    }
    pub fn without_issuer(mut self) -> Self {
        self.issuer = None;
        self
    }
    pub fn with_rsa_key(mut self) -> Self {
        self.key = LeafKey::Rsa;
        self
    }
    pub fn with_raw_issuer_extension(mut self, content: Vec<u8>) -> Self {
        self.raw_issuer_extension = Some(content);
        self
    }
}

pub struct Ecosystem {
    pub root_der: Vec<u8>,
    pub leaf_der: Vec<u8>,
    /// `None` for an RSA leaf.
    pub leaf_key: Option<SigningKey>,
    pub log_key: SigningKey,
}

impl Ecosystem {
    pub fn trust_root_for(
        &self,
        ca_from: DateTime<Utc>,
        ca_until: Option<DateTime<Utc>>,
        tlog_from: DateTime<Utc>,
        tlog_until: Option<DateTime<Utc>>,
    ) -> TrustRoot {
        let log_spki = spki_der(&self.log_key);
        TrustRoot {
            cas: vec![CertificateAuthority {
                cert_chain_der: vec![self.root_der.clone()],
                valid_from: ca_from,
                valid_until: ca_until,
            }],
            tlogs: vec![TransparencyLog {
                key_id: sha256(&log_spki).to_vec(),
                public_key_der: log_spki,
                valid_from: tlog_from,
                valid_until: tlog_until,
            }],
        }
    }
}

pub fn spki_der(key: &SigningKey) -> Vec<u8> {
    key.verifying_key()
        .to_public_key_der()
        .unwrap()
        .as_bytes()
        .to_vec()
}

fn time(t: DateTime<Utc>) -> Time {
    Time::UtcTime(UtcTime::from_unix_duration(Duration::from_secs(t.timestamp() as u64)).unwrap())
}

fn ext(oid: &str, critical: bool, content: Vec<u8>) -> Extension {
    Extension {
        extn_id: ObjectIdentifier::new_unwrap(oid),
        critical,
        extn_value: OctetString::new(content).unwrap(),
    }
}

/// Signs `tbs` with `issuer_key` under ecdsa-with-SHA384, as keytool's
/// `-sigalg SHA384withECDSA` does.
fn sign_certificate(tbs: TbsCertificate, issuer_key: &SigningKey) -> Vec<u8> {
    let tbs_der = tbs.to_der().unwrap();
    let signature: Signature = issuer_key.sign_prehash(&Sha384::digest(&tbs_der)).unwrap();
    let cert = Certificate {
        tbs_certificate: tbs,
        signature_algorithm: AlgorithmIdentifierOwned {
            oid: ObjectIdentifier::new_unwrap("1.2.840.10045.4.3.3"),
            parameters: None,
        },
        signature: BitString::from_bytes(signature.to_der().as_bytes()).unwrap(),
    };
    cert.to_der().unwrap()
}

fn tbs(
    serial: u8,
    issuer: &str,
    subject: &str,
    not_before: DateTime<Utc>,
    not_after: DateTime<Utc>,
    spki: Vec<u8>,
    extensions: Vec<Extension>,
) -> TbsCertificate {
    TbsCertificate {
        version: Version::V3,
        serial_number: SerialNumber::new(&[serial, 0x42]).unwrap(),
        signature: AlgorithmIdentifierOwned {
            oid: ObjectIdentifier::new_unwrap("1.2.840.10045.4.3.3"),
            parameters: None,
        },
        issuer: Name::from_str(issuer).unwrap(),
        validity: Validity {
            not_before: time(not_before),
            not_after: time(not_after),
        },
        subject: Name::from_str(subject).unwrap(),
        subject_public_key_info: SubjectPublicKeyInfoOwned::from_der(&spki).unwrap(),
        issuer_unique_id: None,
        subject_unique_id: None,
        extensions: Some(extensions),
    }
}

/// Builds a root CA and a leaf it signs, with the extensions `spec` describes.
pub fn build(spec: &LeafSpec) -> Ecosystem {
    let root_key = fresh_ec_key();
    let now = Utc::now();
    let root = tbs(
        1,
        "CN=test-root",
        "CN=test-root",
        now,
        now + chrono::Duration::days(3650),
        spki_der(&root_key),
        vec![ext(
            "2.5.29.19",
            true,
            BasicConstraints {
                ca: true,
                path_len_constraint: None,
            }
            .to_der()
            .unwrap(),
        )],
    );
    let root_der = sign_certificate(root, &root_key);

    let (leaf_key, leaf_spki) = match spec.key {
        LeafKey::Ec => {
            let key = fresh_ec_key();
            let spki = spki_der(&key);
            (Some(key), spki)
        }
        LeafKey::Rsa => (
            None,
            base64::engine::general_purpose::STANDARD
                .decode(RSA_SPKI_B64)
                .unwrap(),
        ),
    };
    let mut extensions = Vec::new();
    if spec.code_signing_eku {
        let eku = ExtendedKeyUsage(vec![ObjectIdentifier::new_unwrap("1.3.6.1.5.5.7.3.3")]);
        extensions.push(ext("2.5.29.37", false, eku.to_der().unwrap()));
    }
    extensions.push(ext(
        "2.5.29.15",
        true,
        KeyUsage(KeyUsages::DigitalSignature.into())
            .to_der()
            .unwrap(),
    ));
    if !spec.sans.is_empty() {
        let names = spec
            .sans
            .iter()
            .map(|(kind, value)| match *kind {
                "uri" => GeneralName::UniformResourceIdentifier(Ia5String::new(value).unwrap()),
                "email" => GeneralName::Rfc822Name(Ia5String::new(value).unwrap()),
                other => panic!("unsupported SAN type {other}"),
            })
            .collect();
        extensions.push(ext(
            "2.5.29.17",
            false,
            SubjectAltName(names).to_der().unwrap(),
        ));
    }
    let issuer_oid = spec
        .issuer
        .as_ref()
        .map(|(oid, _)| *oid)
        .unwrap_or(ISSUER_OID);
    if let Some(raw) = &spec.raw_issuer_extension {
        extensions.push(ext(issuer_oid, false, raw.clone()));
    } else if let Some((oid, value)) = &spec.issuer {
        let content = if *oid == ISSUER_OID {
            let mut tlv = vec![0x0C, value.len() as u8];
            tlv.extend_from_slice(value.as_bytes());
            tlv
        } else {
            value.as_bytes().to_vec()
        };
        extensions.push(ext(oid, false, content));
    }
    let leaf = tbs(
        2,
        "CN=test-root",
        "CN=test-leaf",
        spec.not_before,
        spec.not_after,
        leaf_spki,
        extensions,
    );
    let leaf_der = sign_certificate(leaf, &root_key);
    Ecosystem {
        root_der,
        leaf_der,
        leaf_key,
        log_key: fresh_ec_key(),
    }
}

/// Every knob `buildBundle` has; `Default` is a wholly valid one-leaf bundle.
#[derive(Default)]
pub struct BundleOptions {
    /// What the leaf key actually signs (default: the declared digest).
    pub sign_over_digest: Option<[u8; 32]>,
    /// The tlog entry's `spec.data.hash.value` (default: the declared digest's hex).
    pub rekor_hash_value_hex: Option<String>,
    pub entry_signature_override: Option<Vec<u8>>,
    pub entry_public_key_cert_der_override: Option<Vec<u8>>,
    pub tree_size: Option<usize>,
    pub leaf_index: Option<usize>,
    pub checkpoint_signing_key: Option<SigningKey>,
    pub checkpoint_root_override: Option<Vec<u8>>,
    pub checkpoint_size_override: Option<i64>,
}

pub fn valid_bundle_json(
    eco: &Ecosystem,
    digest: [u8; 32],
    integrated_time: DateTime<Utc>,
    log_index: i64,
) -> String {
    bundle_json(
        eco,
        digest,
        integrated_time,
        log_index,
        BundleOptions::default(),
    )
}

pub fn bundle_json(
    eco: &Ecosystem,
    declared_digest: [u8; 32],
    integrated_time: DateTime<Utc>,
    entry_log_index: i64,
    options: BundleOptions,
) -> String {
    let sign_over = options.sign_over_digest.unwrap_or(declared_digest);
    let signature = match &eco.leaf_key {
        Some(key) => {
            let sig: Signature = key.sign_prehash(&sign_over).unwrap();
            sig.to_der().as_bytes().to_vec()
        }
        None => random_bytes(256),
    };
    let rekor_hash = options
        .rekor_hash_value_hex
        .unwrap_or_else(|| hex::encode(declared_digest));
    let entry_signature = options
        .entry_signature_override
        .unwrap_or_else(|| signature.clone());
    let entry_cert = options
        .entry_public_key_cert_der_override
        .unwrap_or_else(|| eco.leaf_der.clone());
    let body = json!({
        "apiVersion": "0.0.1",
        "kind": "hashedrekord",
        "spec": {
            "data": {"hash": {"algorithm": "sha256", "value": rekor_hash}},
            "signature": {"content": b64(&entry_signature), "publicKey": {"content": b64(pem(&entry_cert).as_bytes())}}
        }
    });
    let canonicalized_body = serde_json::to_vec(&body).unwrap();
    let body_b64 = b64(&canonicalized_body);

    let log_spki = spki_der(&eco.log_key);
    let log_id = sha256(&log_spki);
    let mut leaf_input = vec![0u8];
    leaf_input.extend_from_slice(&canonicalized_body);
    let real_leaf_hash = sha256(&leaf_input).to_vec();

    let tree_size = options.tree_size.unwrap_or(1);
    let leaf_index = options.leaf_index.unwrap_or(0);
    let leaves: Vec<Vec<u8>> = (0..tree_size)
        .map(|i| {
            if i == leaf_index {
                real_leaf_hash.clone()
            } else {
                random_bytes(32)
            }
        })
        .collect();
    let root_hash = merkle_root(&leaves, 0, tree_size);
    let proof_hashes = if tree_size == 1 {
        Vec::new()
    } else {
        audit_path(&leaves, leaf_index, 0, tree_size)
    };

    let checkpoint_key = options
        .checkpoint_signing_key
        .as_ref()
        .unwrap_or(&eco.log_key);
    let checkpoint_root = options
        .checkpoint_root_override
        .unwrap_or_else(|| root_hash.clone());
    let checkpoint_size = options.checkpoint_size_override.unwrap_or(tree_size as i64);
    let checkpoint_body = format!(
        "{LOG_ORIGIN}\n{checkpoint_size}\n{}\n",
        b64(&checkpoint_root)
    );
    let checkpoint_sig = sha256_with_ecdsa(checkpoint_key, checkpoint_body.as_bytes());
    let mut sig_line = vec![0u8; 4];
    sig_line.extend_from_slice(&checkpoint_sig);
    let envelope = format!(
        "{checkpoint_body}\n\u{2014} test.rekor.local {}\n",
        b64(&sig_line)
    );

    let canonical_set = format!(
        "{{\"body\":\"{body_b64}\",\"integratedTime\":{},\"logID\":\"{}\",\"logIndex\":{entry_log_index}}}",
        integrated_time.timestamp(),
        hex::encode(log_id)
    );
    let set = sha256_with_ecdsa(&eco.log_key, canonical_set.as_bytes());

    let bundle = json!({
        "mediaType": "application/vnd.dev.sigstore.bundle.v0.3+json",
        "verificationMaterial": {
            "certificate": {"rawBytes": b64(&eco.leaf_der)},
            "tlogEntries": [{
                "logIndex": entry_log_index.to_string(),
                "logId": {"keyId": b64(&log_id)},
                "kindVersion": {"kind": "hashedrekord", "version": "0.0.1"},
                "integratedTime": integrated_time.timestamp().to_string(),
                "inclusionPromise": {"signedEntryTimestamp": b64(&set)},
                "inclusionProof": {
                    "logIndex": leaf_index.to_string(),
                    "rootHash": b64(&root_hash),
                    "treeSize": tree_size.to_string(),
                    "hashes": proof_hashes.iter().map(|h| b64(h)).collect::<Vec<_>>(),
                    "checkpoint": {"envelope": envelope}
                },
                "canonicalizedBody": body_b64
            }]
        },
        "messageSignature": {
            "messageDigest": {"algorithm": "SHA2_256", "digest": b64(&declared_digest)},
            "signature": b64(&signature)
        }
    });
    serde_json::to_string(&bundle).unwrap()
}

fn sha256_with_ecdsa(key: &SigningKey, message: &[u8]) -> Vec<u8> {
    let sig: Signature = key.sign_prehash(&sha256(message)).unwrap();
    sig.to_der().as_bytes().to_vec()
}

pub fn pem(der: &[u8]) -> String {
    let encoded = b64(der);
    let lines: Vec<&str> = encoded
        .as_bytes()
        .chunks(64)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect();
    format!(
        "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n",
        lines.join("\n")
    )
}

fn highest_one_bit(x: usize) -> usize {
    if x == 0 {
        0
    } else {
        1 << (usize::BITS - 1 - x.leading_zeros())
    }
}

fn hash_children(left: &[u8], right: &[u8]) -> Vec<u8> {
    let mut input = vec![0x01];
    input.extend_from_slice(left);
    input.extend_from_slice(right);
    sha256(&input).to_vec()
}

fn merkle_root(leaves: &[Vec<u8>], lo: usize, hi: usize) -> Vec<u8> {
    if hi - lo == 1 {
        return leaves[lo].clone();
    }
    let k = highest_one_bit(hi - lo - 1);
    hash_children(
        &merkle_root(leaves, lo, lo + k),
        &merkle_root(leaves, lo + k, hi),
    )
}

fn audit_path(leaves: &[Vec<u8>], m: usize, lo: usize, hi: usize) -> Vec<Vec<u8>> {
    if hi - lo == 1 {
        return Vec::new();
    }
    let k = highest_one_bit(hi - lo - 1);
    let mut path;
    if m - lo < k {
        path = audit_path(leaves, m, lo, lo + k);
        path.push(merkle_root(leaves, lo + k, hi));
    } else {
        path = audit_path(leaves, m, lo + k, hi);
        path.push(merkle_root(leaves, lo, lo + k));
    }
    path
}
