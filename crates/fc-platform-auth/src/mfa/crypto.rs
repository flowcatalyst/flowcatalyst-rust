//! Second-factor primitives, as Go's `internal/platform/mfa/crypto.go`:
//! RFC 6238 TOTP (SHA-1, 6 digits, 30 s steps, one step of skew either
//! side), recovery codes, email PINs, trusted-device tokens, and the
//! enrolment QR code.

use base64::Engine;
use hmac::{Hmac, Mac};
use rand::Rng;
use sha1::Sha1;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Seconds per TOTP step.
pub const TOTP_PERIOD: i64 = 30;
/// Accepted steps either side of now (±30 s).
const TOTP_SKEW: i64 = 1;
const TOTP_DIGITS: u32 = 6;
/// Shared-secret bytes (160 bits).
const TOTP_SECRET_BYTES: usize = 20;
/// Side of the enrolment QR image, in pixels (Go `qrDataURI(key, 240)`).
const QR_SIZE: usize = 240;

/// Crockford base32 minus visually ambiguous characters, upper-case.
const RECOVERY_ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTVWXYZ23456789";

/// Lower-case hex SHA-256: the at-rest form of recovery codes, email PINs
/// and trusted-device tokens (none are passwords; all are high-entropy).
pub fn sha256_hex(s: &str) -> String {
    hex_lower(&Sha256::digest(s.as_bytes()))
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Constant-time string equality.
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && bool::from(a.as_bytes().ct_eq(b.as_bytes()))
}

/// A fresh TOTP shared secret, base32 without padding (what authenticator
/// apps take for manual entry).
pub fn new_totp_secret() -> String {
    let mut bytes = [0u8; TOTP_SECRET_BYTES];
    rand::rng().fill(&mut bytes[..]);
    data_encoding::BASE32_NOPAD.encode(&bytes)
}

/// The `otpauth://` provisioning URI for `secret`, in pquerna/otp's shape:
/// `otpauth://totp/{issuer}:{account}?algorithm=SHA1&digits=6&issuer=…&period=30&secret=…`.
pub fn totp_uri(issuer: &str, account: &str, secret: &str) -> String {
    format!(
        "otpauth://totp/{}:{}?algorithm=SHA1&digits=6&issuer={}&period={}&secret={}",
        path_escape(issuer),
        path_escape(account),
        query_escape(issuer),
        TOTP_PERIOD,
        query_escape(secret),
    )
}

fn path_escape(s: &str) -> String {
    escape(s, |c| {
        c.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:@".contains(&c)
    })
}

fn query_escape(s: &str) -> String {
    escape(s, |c| c.is_ascii_alphanumeric() || b"-._~".contains(&c))
}

fn escape(s: &str, keep: impl Fn(u8) -> bool) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if keep(b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The base32 secret's bytes, read as pquerna/otp reads it: trimmed,
/// upper-cased, padding optional.
fn decode_secret(secret: &str) -> Option<Vec<u8>> {
    let cleaned = secret.trim().to_uppercase();
    let unpadded = cleaned.trim_end_matches('=');
    data_encoding::BASE32_NOPAD.decode(unpadded.as_bytes()).ok()
}

/// RFC 4226 HOTP for `counter`, zero-padded to six digits.
fn hotp(key: &[u8], counter: u64) -> String {
    let mut mac = Hmac::<Sha1>::new_from_slice(key).expect("HMAC takes any key length");
    mac.update(&counter.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = (digest[digest.len() - 1] & 0x0f) as usize;
    let binary = ((u32::from(digest[offset]) & 0x7f) << 24)
        | (u32::from(digest[offset + 1]) << 16)
        | (u32::from(digest[offset + 2]) << 8)
        | u32::from(digest[offset + 3]);
    format!(
        "{:0width$}",
        binary % 10u32.pow(TOTP_DIGITS),
        width = TOTP_DIGITS as usize
    )
}

/// The TOTP code for `secret` at the step containing `unix_secs`.
pub fn totp_code(secret: &str, unix_secs: i64) -> Option<String> {
    let key = decode_secret(secret)?;
    Some(hotp(&key, (unix_secs / TOTP_PERIOD) as u64))
}

/// Check `code` against `secret` at `unix_secs`, allowing one step either
/// side. On a match, the matched step, so the caller can refuse a replay
/// of it (Go `validateTOTP`).
pub fn validate_totp(secret: &str, code: &str, unix_secs: i64) -> Option<i64> {
    let key = decode_secret(secret)?;
    let base = unix_secs / TOTP_PERIOD;
    (-TOTP_SKEW..=TOTP_SKEW)
        .map(|i| base + i)
        .find(|&step| step >= 0 && constant_time_eq(&hotp(&key, step as u64), code))
}

/// The timestamp stored as `last_used_at` for `step`: its first second, so
/// the next check derives exactly the same step (Go `timeForStep`).
pub fn time_for_step(step: i64) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp(step * TOTP_PERIOD, 0).unwrap_or_default()
}

/// An `n`-digit numeric PIN, zero-padded, uniform.
pub fn random_digits(n: u32) -> String {
    let value = rand::rng().random_range(0..10u64.pow(n));
    format!("{value:0width$}", width = n as usize)
}

/// One recovery code: two groups of five from the unambiguous alphabet
/// (e.g. `A7K2M-9PQRT`).
pub fn random_recovery_code() -> String {
    let mut rng = rand::rng();
    let mut out = String::with_capacity(11);
    for group in 0..2 {
        if group > 0 {
            out.push('-');
        }
        for _ in 0..5 {
            let i = rng.random_range(0..RECOVERY_ALPHABET.len());
            out.push(RECOVERY_ALPHABET[i] as char);
        }
    }
    out
}

/// A user-entered recovery code in canonical form: upper-case, no spaces or
/// dashes, so `a7k2m 9pqrt` matches `A7K2M-9PQRT`.
pub fn normalize_recovery_code(s: &str) -> String {
    s.trim().to_uppercase().replace(['-', ' '], "")
}

/// 32 random bytes as URL-safe base64 without padding: the raw
/// trusted-device cookie value (only its hash is stored).
pub fn random_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes[..]);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// `uri` as a QR code in a 240×240 grayscale PNG data URI, for an `<img
/// src>` (Go renders the same with pquerna/otp at medium error correction).
/// `None` when the URI doesn't fit a QR code; the caller then omits it.
pub fn qr_data_uri(uri: &str) -> Option<String> {
    use qrcodegen::{QrCode, QrCodeEcc};
    let qr = QrCode::encode_text(uri, QrCodeEcc::Medium).ok()?;
    let modules = qr.size() as usize;
    let scale = QR_SIZE / modules;
    if scale == 0 {
        return None;
    }
    // Centred, the remainder as a white margin (boombuler/barcode `Scale`).
    let offset = (QR_SIZE - modules * scale) / 2;
    let mut pixels = vec![0xffu8; QR_SIZE * QR_SIZE];
    for y in 0..modules * scale {
        for x in 0..modules * scale {
            if qr.get_module((x / scale) as i32, (y / scale) as i32) {
                pixels[(y + offset) * QR_SIZE + x + offset] = 0;
            }
        }
    }
    let png = grayscale_png(QR_SIZE, QR_SIZE, &pixels)?;
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    ))
}

/// A minimal 8-bit grayscale PNG.
fn grayscale_png(width: usize, height: usize, pixels: &[u8]) -> Option<Vec<u8>> {
    use std::io::Write;
    let mut raw = Vec::with_capacity((width + 1) * height);
    for row in pixels.chunks(width) {
        raw.push(0); // filter: none
        raw.extend_from_slice(row);
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(&raw).ok()?;
    let idat = z.finish().ok()?;

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(width as u32).to_be_bytes());
    ihdr.extend_from_slice(&(height as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 0, 0, 0, 0]); // 8-bit, grayscale

    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    for (kind, data) in [
        (b"IHDR", &ihdr[..]),
        (b"IDAT", &idat[..]),
        (b"IEND", &[][..]),
    ] {
        png.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut crc = crc32fast::Hasher::new();
        crc.update(kind);
        crc.update(data);
        png.extend_from_slice(kind);
        png.extend_from_slice(data);
        png.extend_from_slice(&crc.finalize().to_be_bytes());
    }
    Some(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 6238 appendix B, SHA-1 (8 digits there; the last 6 here).
    #[test]
    fn totp_matches_the_rfc_vectors() {
        let secret = data_encoding::BASE32_NOPAD.encode(b"12345678901234567890");
        for (t, code) in [
            (59, "287082"),
            (1111111109, "081804"),
            (1234567890, "005924"),
            (2000000000, "279037"),
        ] {
            assert_eq!(totp_code(&secret, t).unwrap(), code, "t={t}");
        }
    }

    #[test]
    fn a_code_is_accepted_one_step_either_side_and_reports_its_step() {
        let secret = new_totp_secret();
        let now = 1_700_000_000;
        let step = now / TOTP_PERIOD;
        for (at, expect) in [(now - 30, step - 1), (now, step), (now + 30, step + 1)] {
            let code = totp_code(&secret, at).unwrap();
            assert_eq!(validate_totp(&secret, &code, now), Some(expect));
        }
        let far = totp_code(&secret, now + 90).unwrap();
        assert_eq!(validate_totp(&secret, &far, now), None);
        assert_eq!(validate_totp(&secret, "abc", now), None);
        assert_eq!(time_for_step(step).timestamp(), step * TOTP_PERIOD);
    }

    #[test]
    fn a_lower_case_padded_secret_reads_the_same() {
        let secret = new_totp_secret();
        let messy = format!(" {}== ", secret.to_lowercase());
        assert_eq!(totp_code(&messy, 1_000), totp_code(&secret, 1_000));
    }

    #[test]
    fn recovery_codes_normalise_for_comparison() {
        let code = random_recovery_code();
        assert_eq!(code.len(), 11);
        assert_eq!(&code[5..6], "-");
        let typed = format!(" {} ", code.to_lowercase().replace('-', " "));
        assert_eq!(normalize_recovery_code(&typed), code.replace('-', ""));
    }

    #[test]
    fn the_uri_and_qr_are_well_formed() {
        let uri = totp_uri("FlowCatalyst", "a b@acme.test", "JBSWY3DPEHPK3PXP");
        assert_eq!(
            uri,
            "otpauth://totp/FlowCatalyst:a%20b@acme.test?algorithm=SHA1&digits=6\
             &issuer=FlowCatalyst&period=30&secret=JBSWY3DPEHPK3PXP"
        );
        let qr = qr_data_uri(&uri).unwrap();
        let png = base64::engine::general_purpose::STANDARD
            .decode(qr.trim_start_matches("data:image/png;base64,"))
            .unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(random_digits(6).len(), 6);
        assert_eq!(sha256_hex("").len(), 64);
    }
}
