//! AES-256-GCM session cookie encryption.
//!
//! Envelope format (base64url): `v1.<iv>.<ciphertext+tag>`. Multi-key for
//! rotation: the first key is used to encrypt, any of them can decrypt.
//!
//! Mirrors `src/fastify/crypto.ts` in the TypeScript SDK.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;

use crate::auth::AuthError;
use base64::engine::general_purpose;

const IV_LEN: usize = 12;
const KEY_LEN: usize = 32;

/// Encrypts/decrypts cookie payloads with key rotation.
#[derive(Clone)]
pub struct SessionCrypto {
    ciphers: Vec<Aes256Gcm>,
}

impl SessionCrypto {
    /// Build from one or more 32-byte secrets. Accepts base64url, base64,
    /// or hex strings. The first key encrypts; any key can decrypt.
    pub fn new<I, S>(secrets: I) -> Result<Self, AuthError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut ciphers = Vec::new();
        for s in secrets {
            let bytes = decode_secret(s.as_ref())?;
            let key = Key::<Aes256Gcm>::from_slice(&bytes);
            ciphers.push(Aes256Gcm::new(key));
        }
        if ciphers.is_empty() {
            return Err(AuthError::Config(
                "session crypto requires at least one secret".into(),
            ));
        }
        Ok(Self { ciphers })
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> Result<String, AuthError> {
        let cipher = self.ciphers.first().expect("at least one cipher");
        let mut iv_bytes = [0u8; IV_LEN];
        rand::rng().fill_bytes(&mut iv_bytes);
        let nonce = Nonce::from_slice(&iv_bytes);
        let ct = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| AuthError::Crypto(format!("encrypt failed: {e}")))?;
        Ok(format!(
            "v1.{}.{}",
            URL_SAFE_NO_PAD.encode(iv_bytes),
            URL_SAFE_NO_PAD.encode(ct)
        ))
    }

    pub fn decrypt(&self, envelope: &str) -> Option<Vec<u8>> {
        let mut parts = envelope.splitn(3, '.');
        let v = parts.next()?;
        if v != "v1" {
            return None;
        }
        let iv = URL_SAFE_NO_PAD.decode(parts.next()?).ok()?;
        let ct = URL_SAFE_NO_PAD.decode(parts.next()?).ok()?;
        if iv.len() != IV_LEN {
            return None;
        }
        let nonce = Nonce::from_slice(&iv);
        for cipher in &self.ciphers {
            if let Ok(pt) = cipher.decrypt(nonce, ct.as_slice()) {
                return Some(pt);
            }
        }
        None
    }
}

fn decode_secret(s: &str) -> Result<[u8; KEY_LEN], AuthError> {
    let candidates: Vec<Vec<u8>> = [
        URL_SAFE_NO_PAD.decode(s).ok(),
        general_purpose::STANDARD.decode(s).ok(),
        hex_decode(s).ok(),
    ]
    .into_iter()
    .flatten()
    .collect();
    for c in candidates {
        if c.len() == KEY_LEN {
            let mut arr = [0u8; KEY_LEN];
            arr.copy_from_slice(&c);
            return Ok(arr);
        }
    }
    Err(AuthError::Config(format!(
        "session secret must decode to {KEY_LEN} bytes (base64url, base64, or hex)"
    )))
}

fn hex_decode(s: &str) -> Result<Vec<u8>, ()> {
    if s.len() % 2 != 0 {
        return Err(());
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for chunk in s.as_bytes().chunks(2) {
        let hi = hex_val(chunk[0])?;
        let lo = hex_val(chunk[1])?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn hex_val(c: u8) -> Result<u8, ()> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(()),
    }
}

/// Generate a fresh 32-byte secret, base64url-encoded.
pub fn generate_session_secret() -> String {
    let mut buf = [0u8; KEY_LEN];
    rand::rng().fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let c = SessionCrypto::new([generate_session_secret()]).unwrap();
        let env = c.encrypt(b"hello").unwrap();
        assert_eq!(c.decrypt(&env).as_deref(), Some(&b"hello"[..]));
    }

    #[test]
    fn rotation_old_keys_still_decrypt() {
        let k1 = generate_session_secret();
        let k2 = generate_session_secret();
        let old = SessionCrypto::new([&k1]).unwrap();
        let env = old.encrypt(b"legacy").unwrap();
        let rotated = SessionCrypto::new([&k2, &k1]).unwrap();
        assert_eq!(rotated.decrypt(&env).as_deref(), Some(&b"legacy"[..]));
        // New encrypt uses k2; k1 alone can't read it.
        let fresh = rotated.encrypt(b"new").unwrap();
        assert!(SessionCrypto::new([&k1]).unwrap().decrypt(&fresh).is_none());
    }

    #[test]
    fn tampered_returns_none() {
        let c = SessionCrypto::new([generate_session_secret()]).unwrap();
        let env = c.encrypt(b"x").unwrap();
        let mut bytes = env.as_bytes().to_vec();
        // Flip last char.
        let n = bytes.len() - 1;
        bytes[n] ^= 0x01;
        let s = String::from_utf8(bytes).unwrap();
        assert!(c.decrypt(&s).is_none());
    }

    #[test]
    fn wrong_key_returns_none() {
        let a = SessionCrypto::new([generate_session_secret()]).unwrap();
        let b = SessionCrypto::new([generate_session_secret()]).unwrap();
        let env = a.encrypt(b"x").unwrap();
        assert!(b.decrypt(&env).is_none());
    }

    #[test]
    fn short_key_rejected() {
        assert!(SessionCrypto::new(["too-short"]).is_err());
    }
}
