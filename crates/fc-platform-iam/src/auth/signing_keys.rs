//! JWT signing-key material, read the way Go's fc-server reads it.
//!
//! Production passes the RS256 private key inline through
//! `FLOWCATALYST_JWT_PRIVATE_KEY` (an SSM parameter injected by the ECS task
//! definition) and nothing else: Go derives the public key from it
//! (`authservice.publicPEMFromPrivatePEM`) and names the key by a hash of that
//! derived PEM (`generateKeyID`). Rust must land on the same `kid`, or JWKS
//! consumers holding Go's key set would look up a key id that no longer
//! exists after cutover.
//!
//! Values carried in environment variables get mangled on the way (literal
//! `\n`, surrounding quotes, whole-PEM base64); [`normalize_pem`] repairs them
//! exactly as Go's `server.NormalizePEM` does, and the key id of a supplied
//! PEM is computed over the normalised text, as Go computes it.

use base64::Engine;
use rsa::pkcs1::{DecodeRsaPrivateKey, DecodeRsaPublicKey};
use rsa::pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePublicKey, LineEnding};
use rsa::traits::PublicKeyParts;
use rsa::{RsaPrivateKey, RsaPublicKey};

/// Repair a PEM carried in an environment variable (Go `NormalizePEM`,
/// internal/server/signing_key.go): trim whitespace and surrounding double
/// quotes, turn literal `\r\n` / `\n` escapes into newlines, and decode a
/// whole-PEM base64 wrapping. A clean PEM passes through (trimmed).
pub fn normalize_pem(raw: &str) -> String {
    let mut s = raw.trim().trim_matches('"').to_string();
    if s.contains("\\n") {
        s = s.replace("\\r\\n", "\n").replace("\\n", "\n");
    }
    if !s.contains("-----BEGIN") {
        if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(s.trim()) {
            if let Ok(text) = String::from_utf8(decoded) {
                if text.contains("-----BEGIN") {
                    return text;
                }
            }
        }
    }
    s
}

/// The DER bytes of the first PEM block in `pem`, read as leniently as Go's
/// `pem.Decode`: any label, any line length, text around the block ignored.
fn pem_der(pem: &str) -> Option<Vec<u8>> {
    let start = pem.find("-----BEGIN")?;
    let after_begin = &pem[start..];
    let header_end = after_begin.find('\n')?;
    let body_and_rest = &after_begin[header_end + 1..];
    let end = body_and_rest.find("-----END")?;
    let body: String = body_and_rest[..end]
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    base64::engine::general_purpose::STANDARD.decode(body).ok()
}

/// Parse an RSA private key PEM, PKCS#1 or PKCS#8 (Go
/// `parseRSAPrivateKey`).
pub fn parse_private_key(pem: &str) -> Result<RsaPrivateKey, String> {
    let der = pem_der(pem).ok_or_else(|| "no PEM block found".to_string())?;
    RsaPrivateKey::from_pkcs1_der(&der)
        .or_else(|_| RsaPrivateKey::from_pkcs8_der(&der))
        .map_err(|e| format!("not an RSA private key: {e}"))
}

/// Parse an RSA public key PEM, SPKI (`PUBLIC KEY`) or PKCS#1
/// (`RSA PUBLIC KEY`) (Go `parseRSAPublicKey`).
pub fn parse_public_key(pem: &str) -> Result<RsaPublicKey, String> {
    let der = pem_der(pem).ok_or_else(|| "no PEM block found".to_string())?;
    RsaPublicKey::from_public_key_der(&der)
        .or_else(|_| RsaPublicKey::from_pkcs1_der(&der))
        .map_err(|e| format!("not an RSA public key: {e}"))
}

/// The public half of a private key PEM, as a PKIX `PUBLIC KEY` PEM with
/// 64-column lines and a trailing newline: byte-for-byte what Go's
/// `x509.MarshalPKIXPublicKey` + `pem.EncodeToMemory` produce, so the key id
/// hashed from it matches Go's.
pub fn public_pem_from_private_pem(private_pem: &str) -> Result<String, String> {
    let key = parse_private_key(private_pem)?;
    RsaPublicKey::from(&key)
        .to_public_key_pem(LineEnding::LF)
        .map_err(|e| format!("encode public key: {e}"))
}

/// Go `generateKeyID`: base64url (unpadded) of the first 16 bytes of the
/// SHA-256 of the public key PEM text.
pub fn key_id(public_key_pem: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(public_key_pem.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&hash[..16])
}

/// The JWKS `n` / `e` members (base64url, unpadded, big-endian).
pub fn jwk_components(key: &RsaPublicKey) -> (String, String) {
    let enc = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    (
        enc.encode(key.n().to_bytes_be()),
        enc.encode(key.e().to_bytes_be()),
    )
}

/// The first non-empty value among `names`, as Go's `envFirst` reads it.
fn env_first(names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|n| std::env::var(n).ok().filter(|v| !v.trim().is_empty()))
}

/// The inline signing key from the environment, normalised: Go reads
/// `FLOWCATALYST_JWT_PRIVATE_KEY` (the deployed name) then
/// `FC_JWT_SIGNING_KEY_PEM`.
pub fn private_key_from_env() -> Option<String> {
    env_first(&["FLOWCATALYST_JWT_PRIVATE_KEY", "FC_JWT_SIGNING_KEY_PEM"])
        .map(|v| normalize_pem(&v))
}

/// The validation-only previous public key from the environment (Go
/// `normalizedPreviousPublicKey`): `FLOWCATALYST_JWT_PREVIOUS_PUBLIC_KEY`,
/// then Rust's earlier `FLOWCATALYST_JWT_PUBLIC_KEY_PREVIOUS`, normalised.
/// A value that is not a PEM at all (an SSM placeholder — SSM cannot hold an
/// empty string) is dropped, as Go drops it; a PEM that fails to parse is
/// left for the caller to refuse.
pub fn previous_public_key_from_env() -> Option<String> {
    let raw = env_first(&[
        "FLOWCATALYST_JWT_PREVIOUS_PUBLIC_KEY",
        "FLOWCATALYST_JWT_PUBLIC_KEY_PREVIOUS",
    ])?;
    let pem = normalize_pem(&raw);
    pem.contains("-----BEGIN").then_some(pem)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::pkcs1::EncodeRsaPrivateKey;
    use rsa::pkcs8::EncodePrivateKey;

    fn key() -> RsaPrivateKey {
        RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 2048).unwrap()
    }

    /// Go's `pem.EncodeToMemory` layout, built by hand.
    fn go_pem(label: &str, der: &[u8]) -> String {
        let b64 = base64::engine::general_purpose::STANDARD.encode(der);
        let mut out = format!("-----BEGIN {label}-----\n");
        for chunk in b64.as_bytes().chunks(64) {
            out.push_str(std::str::from_utf8(chunk).unwrap());
            out.push('\n');
        }
        out.push_str(&format!("-----END {label}-----\n"));
        out
    }

    #[test]
    fn derived_public_pem_is_byte_identical_to_gos() {
        let k = key();
        let spki = RsaPublicKey::from(&k).to_public_key_der().unwrap();
        let expected = go_pem("PUBLIC KEY", spki.as_bytes());
        let pkcs1 = k.to_pkcs1_pem(LineEnding::LF).unwrap();
        let pkcs8 = k.to_pkcs8_pem(LineEnding::LF).unwrap();
        assert_eq!(public_pem_from_private_pem(&pkcs1).unwrap(), expected);
        assert_eq!(public_pem_from_private_pem(&pkcs8).unwrap(), expected);
        assert_eq!(key_id(&expected).len(), 22);
    }

    #[test]
    fn normalize_repairs_escaped_quoted_and_base64_pems() {
        let pem = key().to_pkcs1_pem(LineEnding::LF).unwrap().to_string();
        let escaped = format!("\"{}\"", pem.trim().replace('\n', "\\n"));
        assert_eq!(normalize_pem(&escaped), pem.trim());
        let crlf = pem.trim().replace('\n', "\\r\\n");
        assert_eq!(normalize_pem(&crlf), pem.trim());
        let b64 = base64::engine::general_purpose::STANDARD.encode(pem.as_bytes());
        assert_eq!(normalize_pem(&b64), pem);
        assert_eq!(normalize_pem(&format!("  {pem}  ")), pem.trim());
        assert!(parse_private_key(&normalize_pem(&escaped)).is_ok());
    }

    #[test]
    fn public_keys_parse_as_spki_or_pkcs1() {
        use rsa::pkcs1::EncodeRsaPublicKey;
        let public = RsaPublicKey::from(&key());
        let spki = public.to_public_key_pem(LineEnding::LF).unwrap();
        let pkcs1 = public.to_pkcs1_pem(LineEnding::CRLF).unwrap();
        assert_eq!(parse_public_key(&spki).unwrap(), public);
        assert_eq!(parse_public_key(&pkcs1).unwrap(), public);
        assert!(parse_public_key("none").is_err());
    }
}
