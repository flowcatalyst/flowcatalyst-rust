//! RFC 6238 TOTP (SHA-1, 6 digits, 30-second steps) for `${totp:secretVar}`:
//! a code from a secret captured out of an enrolment response.

use anyhow::{anyhow, Result};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use std::time::{SystemTime, UNIX_EPOCH};

const PERIOD_SECONDS: i64 = 30;
const DIGITS: u32 = 6;

pub fn current_step() -> i64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    now.div_euclid(PERIOD_SECONDS)
}

/// The code for `step` under a base32 secret (padding and case tolerated).
pub fn code(base32_secret: &str, step: i64) -> Result<String> {
    let cleaned: String = base32_secret
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '=')
        .map(|c| c.to_ascii_uppercase())
        .collect();
    let key = data_encoding::BASE32_NOPAD
        .decode(cleaned.as_bytes())
        .map_err(|e| anyhow!("TOTP secret is not base32: {e}"))?;
    let mut mac = Hmac::<Sha1>::new_from_slice(&key).map_err(|e| anyhow!("{e}"))?;
    mac.update(&step.to_be_bytes());
    let hash = mac.finalize().into_bytes();
    let offset = (hash[hash.len() - 1] & 0x0f) as usize;
    let binary = ((u32::from(hash[offset]) & 0x7f) << 24)
        | (u32::from(hash[offset + 1]) << 16)
        | (u32::from(hash[offset + 2]) << 8)
        | u32::from(hash[offset + 3]);
    let otp = binary % 10u32.pow(DIGITS);
    Ok(format!("{otp:0width$}", width = DIGITS as usize))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 6238 appendix B, SHA-1, T = 59 s → step 1 → 94287082 (8 digits),
    /// whose last six are 287082.
    #[test]
    fn rfc6238_vector() {
        let secret = data_encoding::BASE32_NOPAD.encode(b"12345678901234567890");
        assert_eq!(code(&secret, 1).unwrap(), "287082");
    }
}
