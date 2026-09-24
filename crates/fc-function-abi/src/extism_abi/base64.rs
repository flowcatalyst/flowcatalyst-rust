//! `java.util.Base64.getEncoder()` / `getDecoder()` (the basic, RFC 4648
//! alphabet), as Java's host uses them for `bodyBase64`.
//!
//! Java's decoder is not the strict canonical one most Rust crates default
//! to: padding is optional, but when present it must complete the final unit
//! (`YQ==` and `YQ` are accepted, `YQ=` is not); unused bits in the final unit
//! are ignored (`YR==` decodes like `YQ==`); nothing may follow the padding;
//! whitespace and the URL-safe alphabet are refused.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `Base64.getEncoder().encodeToString(bytes)`: padded.
pub(crate) fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 0x3F) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn value(b: u8) -> Option<u32> {
    ALPHABET.iter().position(|&a| a == b).map(|p| p as u32)
}

/// `Base64.getDecoder().decode(String)` (`Base64.Decoder.decode0`), `None`
/// wherever Java throws `IllegalArgumentException`.
pub(crate) fn decode(text: &str) -> Option<Vec<u8>> {
    // decode(String) goes through ISO-8859-1; anything outside ASCII is an
    // illegal character either way.
    let src = text.as_bytes();
    if src.len() == 1 {
        return None; // "should at least have 2 bytes for base64 bytes"
    }
    let mut out = Vec::with_capacity(src.len() / 4 * 3 + 2);
    let mut bits: u32 = 0;
    let mut shift: i32 = 18;
    let mut sp = 0;
    while sp < src.len() {
        let b = src[sp];
        sp += 1;
        if b == b'=' {
            // "=" at a unit start, "x=" (dangling), or "xx=" without its
            // second "=" are refused; "xx==" and "xxx=" end the input.
            if (shift == 6 && (sp == src.len() || src[sp] != b'=')) || shift == 18 {
                return None;
            }
            if shift == 6 {
                sp += 1;
            }
            break;
        }
        bits |= value(b)? << shift;
        shift -= 6;
        if shift < 0 {
            out.extend_from_slice(&[(bits >> 16) as u8, (bits >> 8) as u8, bits as u8]);
            shift = 18;
            bits = 0;
        }
    }
    match shift {
        6 => out.push((bits >> 16) as u8),
        0 => out.extend_from_slice(&[(bits >> 16) as u8, (bits >> 8) as u8]),
        12 => return None, // "Last unit does not have enough valid bits"
        _ => {}
    }
    // Anything left after the padding is invalid.
    (sp == src.len()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_pads() {
        assert_eq!(encode(b""), "");
        assert_eq!(encode(b"A"), "QQ==");
        assert_eq!(encode(b"hi"), "aGk=");
        assert_eq!(encode(&[0x00, 0xFF, 0x10, b'h', b'i']), "AP8QaGk=");
    }

    #[test]
    fn decode_follows_java() {
        assert_eq!(decode("aGk=").as_deref(), Some(&b"hi"[..]));
        assert_eq!(decode("aGk").as_deref(), Some(&b"hi"[..]));
        assert_eq!(decode("YR==").as_deref(), Some(&b"a"[..]));
        assert_eq!(decode("").as_deref(), Some(&b""[..]));
        for bad in [
            "YQ=", "aGk==", "aG=k", "a", "=", "==", "aGk=\n", "-_8=", "Y", "aGk=aGk=",
        ] {
            assert_eq!(decode(bad), None, "{bad:?}");
        }
    }
}
