//! A minimal software authenticator for the `"authenticator": "register" |
//! "assert"` steps (spec §3): one ES256 key, a random credential id, a
//! signature counter, "none" attestation. Answers `navigator.credentials
//! .create()` / `.get()` the way a platform authenticator would, so the
//! server's ceremonies run for real. A port of Java's `SoftAuthenticator`;
//! one instance per scenario per side.

use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{DerSignature, SigningKey};
use rand::RngCore;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub struct SoftAuthenticator {
    key: SigningKey,
    credential_id: [u8; 32],
    counter: u32,
}

impl Default for SoftAuthenticator {
    fn default() -> Self {
        Self::new()
    }
}

impl SoftAuthenticator {
    pub fn new() -> Self {
        let key = SigningKey::random(&mut rsa::rand_core::OsRng);
        let mut credential_id = [0u8; 32];
        rand::rng().fill_bytes(&mut credential_id);
        Self {
            key,
            credential_id,
            counter: 0,
        }
    }

    /// The browser's `PublicKeyCredential` JSON for a `create()` over `options`.
    pub fn register(&mut self, options: &Value, origin: &str) -> Result<Value> {
        let pk = options
            .get("publicKey")
            .context("options has no publicKey")?;
        let challenge = str_at(pk, "/challenge")?;
        let rp_id = str_at(pk, "/rp/id")?;
        let client_data = client_data("webauthn.create", &challenge, origin);
        let mut auth_data = Vec::new();
        auth_data.extend_from_slice(&Sha256::digest(rp_id.as_bytes()));
        auth_data.push(0x45); // UP | UV | AT
        auth_data.extend_from_slice(&self.counter.to_be_bytes());
        auth_data.extend_from_slice(&[0u8; 16]); // AAGUID
        auth_data.extend_from_slice(&(self.credential_id.len() as u16).to_be_bytes());
        auth_data.extend_from_slice(&self.credential_id);
        auth_data.extend_from_slice(&self.cose_key());

        let mut att = Vec::new();
        cbor::map_header(&mut att, 3);
        cbor::text(&mut att, "fmt");
        cbor::text(&mut att, "none");
        cbor::text(&mut att, "attStmt");
        cbor::map_header(&mut att, 0);
        cbor::text(&mut att, "authData");
        cbor::bytes(&mut att, &auth_data);

        let id = URL_SAFE_NO_PAD.encode(self.credential_id);
        Ok(json!({
            "id": id,
            "rawId": id,
            "type": "public-key",
            "response": {
                "clientDataJSON": URL_SAFE_NO_PAD.encode(&client_data),
                "attestationObject": URL_SAFE_NO_PAD.encode(&att),
                "transports": ["internal"],
            },
            "clientExtensionResults": {},
        }))
    }

    /// The browser's `PublicKeyCredential` JSON for a `get()` over `options`;
    /// the counter advances by one each time.
    pub fn assertion(
        &mut self,
        options: &Value,
        origin: &str,
        principal_id: &str,
    ) -> Result<Value> {
        let pk = options
            .get("publicKey")
            .context("options has no publicKey")?;
        let challenge = str_at(pk, "/challenge")?;
        let rp_id = str_at(pk, "/rpId")?;
        self.counter += 1;
        let client_data = client_data("webauthn.get", &challenge, origin);
        let mut auth_data = Vec::new();
        auth_data.extend_from_slice(&Sha256::digest(rp_id.as_bytes()));
        auth_data.push(0x05); // UP | UV
        auth_data.extend_from_slice(&self.counter.to_be_bytes());
        let mut to_sign = auth_data.clone();
        to_sign.extend_from_slice(&Sha256::digest(&client_data));
        let signature: DerSignature = self.key.sign(&to_sign);

        let id = URL_SAFE_NO_PAD.encode(self.credential_id);
        Ok(json!({
            "id": id,
            "rawId": id,
            "type": "public-key",
            "response": {
                "clientDataJSON": URL_SAFE_NO_PAD.encode(&client_data),
                "authenticatorData": URL_SAFE_NO_PAD.encode(&auth_data),
                "signature": URL_SAFE_NO_PAD.encode(signature.as_bytes()),
                "userHandle": URL_SAFE_NO_PAD.encode(principal_id.as_bytes()),
            },
            "clientExtensionResults": {},
        }))
    }

    /// COSE_Key `{1: 2 (EC2), 3: -7 (ES256), -1: 1 (P-256), -2: x, -3: y}`.
    fn cose_key(&self) -> Vec<u8> {
        let point = self.key.verifying_key().to_encoded_point(false);
        let x = point.x().expect("uncompressed point has x");
        let y = point.y().expect("uncompressed point has y");
        let mut out = Vec::new();
        cbor::map_header(&mut out, 5);
        cbor::int(&mut out, 1);
        cbor::int(&mut out, 2);
        cbor::int(&mut out, 3);
        cbor::int(&mut out, -7);
        cbor::int(&mut out, -1);
        cbor::int(&mut out, 1);
        cbor::int(&mut out, -2);
        cbor::bytes(&mut out, x);
        cbor::int(&mut out, -3);
        cbor::bytes(&mut out, y);
        out
    }
}

fn str_at(node: &Value, pointer: &str) -> Result<String> {
    node.pointer(pointer)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow!("options have no string at {pointer}"))
}

fn client_data(kind: &str, challenge: &str, origin: &str) -> Vec<u8> {
    json!({"type": kind, "challenge": challenge, "origin": origin, "crossOrigin": false})
        .to_string()
        .into_bytes()
}

/// Just enough deterministic CBOR (RFC 8949) for an attestation object and a COSE key.
mod cbor {
    fn head(out: &mut Vec<u8>, major: u8, n: u64) {
        let m = major << 5;
        match n {
            0..=23 => out.push(m | n as u8),
            24..=0xff => out.extend_from_slice(&[m | 24, n as u8]),
            0x100..=0xffff => {
                out.push(m | 25);
                out.extend_from_slice(&(n as u16).to_be_bytes());
            }
            0x1_0000..=0xffff_ffff => {
                out.push(m | 26);
                out.extend_from_slice(&(n as u32).to_be_bytes());
            }
            _ => {
                out.push(m | 27);
                out.extend_from_slice(&n.to_be_bytes());
            }
        }
    }
    pub fn int(out: &mut Vec<u8>, v: i64) {
        if v >= 0 {
            head(out, 0, v as u64);
        } else {
            head(out, 1, (-1 - v) as u64);
        }
    }
    pub fn bytes(out: &mut Vec<u8>, b: &[u8]) {
        head(out, 2, b.len() as u64);
        out.extend_from_slice(b);
    }
    pub fn text(out: &mut Vec<u8>, s: &str) {
        head(out, 3, s.len() as u64);
        out.extend_from_slice(s.as_bytes());
    }
    pub fn map_header(out: &mut Vec<u8>, entries: u64) {
        head(out, 5, entries);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::signature::Verifier;
    use p256::ecdsa::VerifyingKey;

    #[test]
    fn an_assertion_verifies_under_the_registered_key() {
        let mut a = SoftAuthenticator::new();
        let reg = a
            .register(
                &json!({"publicKey": {"challenge": "abc", "rp": {"id": "localhost"}}}),
                "http://localhost:1",
            )
            .unwrap();
        assert_eq!(reg["type"], "public-key");
        let asr = a
            .assertion(
                &json!({"publicKey": {"challenge": "def", "rpId": "localhost"}}),
                "http://localhost:1",
                "prn_1",
            )
            .unwrap();
        let auth_data = URL_SAFE_NO_PAD
            .decode(asr["response"]["authenticatorData"].as_str().unwrap())
            .unwrap();
        let cd = URL_SAFE_NO_PAD
            .decode(asr["response"]["clientDataJSON"].as_str().unwrap())
            .unwrap();
        let sig = URL_SAFE_NO_PAD
            .decode(asr["response"]["signature"].as_str().unwrap())
            .unwrap();
        let mut signed = auth_data.clone();
        signed.extend_from_slice(&Sha256::digest(&cd));
        let vk = VerifyingKey::from(&a.key);
        let sig = p256::ecdsa::DerSignature::try_from(sig.as_slice()).unwrap();
        vk.verify(&signed, &sig).unwrap();
        assert_eq!(u32::from_be_bytes(auth_data[33..37].try_into().unwrap()), 1);
    }

    #[test]
    fn cbor_ints_and_heads() {
        let mut out = Vec::new();
        cbor::int(&mut out, -7);
        cbor::int(&mut out, 24);
        cbor::int(&mut out, 500);
        assert_eq!(out, vec![0x26, 0x18, 24, 0x19, 0x01, 0xf4]);
    }
}
