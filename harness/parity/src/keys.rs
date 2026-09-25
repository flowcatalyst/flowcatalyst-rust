//! Per-run secrets handed to both sides (spec §2): one RSA-2048 key (PKCS#8
//! private PEM, plus its SPKI public PEM, because Rust's `fc-server` reads
//! `FC_JWT_PRIVATE_KEY_PATH` + `FC_JWT_PUBLIC_KEY_PATH` where Go reads
//! `FC_JWT_SIGNING_KEY_PATH`), and one 32-byte `FLOWCATALYST_APP_KEY`, so both
//! sides mint tokens under the same key and read the same encrypted columns.

use anyhow::{Context, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use rand::RngCore;
use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding};
use rsa::{RsaPrivateKey, RsaPublicKey};
use std::path::Path;

pub fn generate_rsa_pems(private_path: &Path, public_path: &Path) -> Result<()> {
    let mut rng = rsa::rand_core::OsRng;
    let private = RsaPrivateKey::new(&mut rng, 2048).context("generate RSA key")?;
    let public = RsaPublicKey::from(&private);
    let private_pem = private
        .to_pkcs8_pem(LineEnding::LF)
        .context("encode private key")?;
    let public_pem = public
        .to_public_key_pem(LineEnding::LF)
        .context("encode public key")?;
    std::fs::write(private_path, private_pem.as_bytes())
        .with_context(|| format!("write {}", private_path.display()))?;
    std::fs::write(public_path, public_pem)
        .with_context(|| format!("write {}", public_path.display()))?;
    Ok(())
}

/// 32 random bytes, padded standard base64.
pub fn generate_app_key() -> String {
    let mut key = [0u8; 32];
    rand::rng().fill_bytes(&mut key);
    STANDARD.encode(key)
}

/// 12 hex characters: `${run}`, the same on both sides.
pub fn random_token() -> String {
    let mut bytes = [0u8; 6];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}
