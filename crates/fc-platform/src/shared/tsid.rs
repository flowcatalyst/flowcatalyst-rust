//! TSID Generator
//!
//! Generates Time-Sorted IDs as Crockford Base32 strings.
//! Matches Java's TsidGenerator for ID compatibility.

use std::sync::atomic::{AtomicU16, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Crockford Base32 alphabet (excludes I, L, O, U)
const ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

static COUNTER: AtomicU16 = AtomicU16::new(0);

/// TSID Generator for creating unique, time-sorted identifiers
pub struct TsidGenerator;

impl TsidGenerator {
    /// Generate a new TSID as a Crockford Base32 string
    /// Example output: "0HZXEQ5Y8JY5Z"
    ///
    /// TSID structure (64 bits):
    /// - 42 bits: timestamp (milliseconds since epoch, ~139 years)
    /// - 10 bits: random component
    /// - 12 bits: counter (4096 unique IDs per millisecond)
    pub fn generate() -> String {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis() as u64;

        // Get counter and increment atomically
        let counter = COUNTER.fetch_add(1, Ordering::SeqCst) as u64;

        // Random component (10 bits)
        let random: u64 = rand_u16() as u64 & 0x3FF;

        // Combine: timestamp (42 bits) | random (10 bits) | counter (12 bits)
        let tsid = ((now & 0x3FFFFFFFFFF) << 22) | (random << 12) | (counter & 0xFFF);

        encode_crockford(tsid)
    }

    /// Convert a TSID string to its numeric representation
    pub fn to_long(tsid_str: &str) -> Option<i64> {
        decode_crockford(tsid_str).map(|v| v as i64)
    }

    /// Convert a numeric TSID to its string representation
    pub fn from_long(value: i64) -> String {
        encode_crockford(value as u64)
    }
}

/// Encode a 64-bit value to Crockford Base32 (13 characters)
fn encode_crockford(mut value: u64) -> String {
    let mut result = [b'0'; 13];

    for i in (0..13).rev() {
        result[i] = ALPHABET[(value & 0x1F) as usize];
        value >>= 5;
    }

    String::from_utf8(result.to_vec()).unwrap()
}

/// Decode a Crockford Base32 string to 64-bit value
fn decode_crockford(s: &str) -> Option<u64> {
    if s.len() != 13 {
        return None;
    }

    let mut result: u64 = 0;
    for c in s.chars() {
        let c = c.to_ascii_uppercase();
        let val = match c {
            '0'..='9' => c as u64 - '0' as u64,
            'A'..='H' => c as u64 - 'A' as u64 + 10,
            'J'..='K' => c as u64 - 'J' as u64 + 18,
            'M'..='N' => c as u64 - 'M' as u64 + 20,
            'P'..='T' => c as u64 - 'P' as u64 + 22,
            'V'..='Z' => c as u64 - 'V' as u64 + 27,
            _ => return None,
        };
        result = (result << 5) | val;
    }

    Some(result)
}

/// Simple random u16 using system time and counter
fn rand_u16() -> u16 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    let counter = COUNTER.load(Ordering::Relaxed) as u64;
    ((now ^ (counter.wrapping_mul(0x5851F42D4C957F2D))) & 0xFFFF) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_tsid() {
        let id = TsidGenerator::generate();
        assert_eq!(id.len(), 13); // TSID is always 13 characters
        println!("Generated TSID: {}", id);
    }

    #[test]
    fn test_uniqueness() {
        let mut ids = std::collections::HashSet::new();
        for _ in 0..1000 {
            let id = TsidGenerator::generate();
            assert!(ids.insert(id), "Duplicate TSID generated");
        }
    }

    #[test]
    fn test_round_trip() {
        let id = TsidGenerator::generate();
        let num = TsidGenerator::to_long(&id).unwrap();
        let back = TsidGenerator::from_long(num);
        assert_eq!(id, back);
    }

    #[test]
    fn test_sortability() {
        let id1 = TsidGenerator::generate();
        std::thread::sleep(std::time::Duration::from_millis(1));
        let id2 = TsidGenerator::generate();
        assert!(id1 < id2, "TSIDs should be lexicographically sortable");
    }
}
