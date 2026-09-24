//! Raw 13-character Crockford Base32 TSIDs (Java `sdk/tsid/Tsid.java`
//! `generate()`), the invocation ids the listener hands every call.
//!
//! Bit layout, as Java and Go: 42 bits of milliseconds since the Unix epoch,
//! then a 12-bit sequence, then 10 random bits. The sequence sits above the
//! random bits so two ids minted in one millisecond sort in minting order. A
//! fresh millisecond restarts the sequence at a random offset; running out of
//! sequence borrows the next millisecond; the state only moves forward, so a
//! wall-clock step backwards never reuses an id.

use std::sync::atomic::{AtomicU64, Ordering};

use rand::Rng;

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const LENGTH: usize = 13;
const MS_MASK: u64 = 0x3FF_FFFF_FFFF; // 42 bits
const SEQ_BITS: u32 = 12;
const SEQ_MASK: u64 = 0xFFF;
const RANDOM_BITS: u32 = 10;
const RANDOM_MASK: u64 = 0x3FF;

/// Bits 63..12 = the last issued millisecond, bits 11..0 = its sequence.
static STATE: AtomicU64 = AtomicU64::new(0);

/// A new raw TSID (no prefix).
pub fn generate() -> String {
    let (ms, seq) = next_ms_seq();
    let random = rand::rng().random_range(0..=RANDOM_MASK);
    encode(((ms & MS_MASK) << 22) | (seq << RANDOM_BITS) | random)
}

fn next_ms_seq() -> (u64, u64) {
    loop {
        let now = chrono::Utc::now().timestamp_millis().max(0) as u64;
        let old = STATE.load(Ordering::Acquire);
        let last_ms = old >> SEQ_BITS;
        let last_seq = old & SEQ_MASK;
        let (ms, seq) = if now > last_ms {
            (now, random_seq())
        } else if last_seq < SEQ_MASK {
            (last_ms, last_seq + 1)
        } else {
            (last_ms + 1, random_seq())
        };
        if STATE
            .compare_exchange(
                old,
                (ms << SEQ_BITS) | seq,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            return (ms, seq);
        }
    }
}

fn random_seq() -> u64 {
    rand::rng().random_range(0..=SEQ_MASK)
}

fn encode(value: u64) -> String {
    let mut out = [0u8; LENGTH];
    let mut v = value;
    for slot in out.iter_mut().rev() {
        *slot = ALPHABET[(v & 0x1F) as usize];
        v >>= 5;
    }
    out.iter().map(|&b| b as char).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thirteen_crockford_characters() {
        let id = generate();
        assert_eq!(id.len(), 13);
        assert!(id.bytes().all(|b| ALPHABET.contains(&b)), "{id}");
    }

    #[test]
    fn ids_minted_in_sequence_sort_in_sequence() {
        let ids: Vec<String> = (0..2000).map(|_| generate()).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted);
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len());
    }

    #[test]
    fn encode_matches_javas_alphabet_and_width() {
        assert_eq!(encode(0), "0000000000000");
        assert_eq!(encode(31), "000000000000Z");
        assert_eq!(encode(u64::MAX), "FZZZZZZZZZZZZ");
    }
}
