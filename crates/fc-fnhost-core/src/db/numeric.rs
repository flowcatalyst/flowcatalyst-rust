//! PostgreSQL `numeric` in its binary wire form (`numeric_send` /
//! `numeric_recv`): `ndigits`, `weight`, `sign`, `dscale` (each 16 bits)
//! then `ndigits` base-10000 digits. Decoded to exactly the text PostgreSQL
//! prints (Java's row mapping sends `numeric` as that string), and encoded
//! from a decimal's text (a guest's `decimal` parameter).

const POSITIVE: u16 = 0x0000;
const NEGATIVE: u16 = 0x4000;
const NAN: u16 = 0xC000;
const PLUS_INFINITY: u16 = 0xD000;
const MINUS_INFINITY: u16 = 0xF000;

/// The largest `dscale` PostgreSQL accepts (`NUMERIC_MAX_DISPLAY_SCALE`).
const MAX_DSCALE: i64 = 1000;

/// The text PostgreSQL's `numeric_out` prints for a binary value; `None`
/// when the bytes are not a numeric.
pub fn decode(bytes: &[u8]) -> Option<String> {
    let word = |i: usize| -> Option<u16> {
        bytes
            .get(i..i + 2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]))
    };
    let ndigits = word(0)? as usize;
    let weight = word(2)? as i16 as i32;
    let sign = word(4)?;
    let dscale = word(6)? as usize;
    match sign {
        NAN => return Some("NaN".into()),
        PLUS_INFINITY => return Some("Infinity".into()),
        MINUS_INFINITY => return Some("-Infinity".into()),
        POSITIVE | NEGATIVE => {}
        _ => return None,
    }
    let mut digits = Vec::with_capacity(ndigits);
    for k in 0..ndigits {
        let d = word(8 + 2 * k)?;
        if d >= 10_000 {
            return None;
        }
        digits.push(d);
    }
    let digit = |k: i32| -> u16 {
        if k >= 0 && (k as usize) < digits.len() {
            digits[k as usize]
        } else {
            0
        }
    };
    let mut out = String::new();
    if sign == NEGATIVE {
        out.push('-');
    }
    if weight < 0 {
        out.push('0');
    } else {
        for k in 0..=weight {
            let d = digit(k);
            if k == 0 {
                out.push_str(&d.to_string());
            } else {
                out.push_str(&format!("{d:04}"));
            }
        }
    }
    if dscale > 0 {
        out.push('.');
        let mut written = 0;
        let mut k = weight + 1;
        while written < dscale {
            let group = format!("{:04}", digit(k));
            for c in group.chars() {
                if written == dscale {
                    break;
                }
                out.push(c);
                written += 1;
            }
            k += 1;
        }
    }
    Some(out)
}

/// A decimal's text (`12.50`, `-3`, `.5`, `1.5e10`, `NaN`, `Infinity`) as
/// the binary form; `None` when it is not one. The scale is the text's own
/// (`12.50` keeps two places), as PostgreSQL's `numeric_in`.
pub fn encode(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    let special = |sign: u16| {
        let mut out = Vec::with_capacity(8);
        for w in [0u16, 0, sign, 0] {
            out.extend_from_slice(&w.to_be_bytes());
        }
        Some(out)
    };
    match text.to_ascii_lowercase().as_str() {
        "nan" => return special(NAN),
        "infinity" | "+infinity" | "inf" | "+inf" => return special(PLUS_INFINITY),
        "-infinity" | "-inf" => return special(MINUS_INFINITY),
        _ => {}
    }
    let (negative, rest) = match text.as_bytes().first()? {
        b'-' => (true, &text[1..]),
        b'+' => (false, &text[1..]),
        _ => (false, text),
    };
    let (mantissa, exponent) = match rest.find(['e', 'E']) {
        Some(p) => (&rest[..p], rest[p + 1..].parse::<i64>().ok()?),
        None => (rest, 0),
    };
    let (int_part, frac_part) = match mantissa.split_once('.') {
        Some((i, f)) => (i, f),
        None => (mantissa, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    if !int_part
        .bytes()
        .chain(frac_part.bytes())
        .all(|b| b.is_ascii_digit())
    {
        return None;
    }
    // value = 0.<all digits> × 10^point, as decimal digits.
    let all: Vec<u8> = int_part
        .bytes()
        .chain(frac_part.bytes())
        .map(|b| b - b'0')
        .collect();
    let point = int_part.len() as i64 + exponent;
    let dscale = (frac_part.len() as i64 - exponent).clamp(0, MAX_DSCALE);
    if exponent.abs() > 100_000 {
        return None;
    }
    // Strip leading zeros (moving the point) and trailing zeros.
    let first = all.iter().position(|&d| d != 0);
    let Some(first) = first else {
        // Zero: no digits, weight 0, the text's scale.
        let mut out = Vec::with_capacity(8);
        for w in [0u16, 0, POSITIVE, dscale as u16] {
            out.extend_from_slice(&w.to_be_bytes());
        }
        return Some(out);
    };
    let last = all.iter().rposition(|&d| d != 0).unwrap_or(first);
    let significant = &all[first..=last];
    let point = point - first as i64;
    // Group into base-10000 digits aligned so that the decimal point falls
    // on a group boundary: the digit at decimal position p (p = point − 1
    // for the first) belongs to group floor(p / 4).
    let first_pos = point - 1; // power of ten of significant[0]
    let weight = first_pos.div_euclid(4);
    let lead_pad = 3 - first_pos.rem_euclid(4); // zeros before significant[0] in its group
    let mut padded = vec![0u8; lead_pad as usize];
    padded.extend_from_slice(significant);
    while !padded.len().is_multiple_of(4) {
        padded.push(0);
    }
    let groups: Vec<u16> = padded
        .chunks(4)
        .map(|c| c.iter().fold(0u16, |acc, &d| acc * 10 + d as u16))
        .collect();
    let mut groups = groups;
    while groups.last() == Some(&0) {
        groups.pop();
    }
    let weight = i16::try_from(weight).ok()?;
    let ndigits = u16::try_from(groups.len()).ok()?;
    let mut out = Vec::with_capacity(8 + 2 * groups.len());
    out.extend_from_slice(&ndigits.to_be_bytes());
    out.extend_from_slice(&weight.to_be_bytes());
    out.extend_from_slice(&(if negative { NEGATIVE } else { POSITIVE }).to_be_bytes());
    out.extend_from_slice(&(dscale as u16).to_be_bytes());
    for g in groups {
        out.extend_from_slice(&g.to_be_bytes());
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wire(ndigits: u16, weight: i16, sign: u16, dscale: u16, digits: &[u16]) -> Vec<u8> {
        let mut out = Vec::new();
        for w in [ndigits, weight as u16, sign, dscale] {
            out.extend_from_slice(&w.to_be_bytes());
        }
        for d in digits {
            out.extend_from_slice(&d.to_be_bytes());
        }
        out
    }

    /// What PostgreSQL sends for each value (`numeric_send`), and prints.
    #[test]
    fn decodes_to_postgres_text() {
        for (bytes, text) in [
            (wire(0, 0, POSITIVE, 0, &[]), "0"),
            (wire(0, 0, POSITIVE, 2, &[]), "0.00"),
            (wire(1, 0, POSITIVE, 2, &[12]), "12.00"),
            (wire(2, 0, POSITIVE, 2, &[12, 5000]), "12.50"),
            (wire(2, 1, NEGATIVE, 0, &[1, 2345]), "-12345"),
            (wire(1, -1, POSITIVE, 3, &[500]), "0.050"),
            (wire(1, -2, POSITIVE, 6, &[12]), "0.000000"),
            (wire(1, -2, POSITIVE, 8, &[12]), "0.00000012"),
            (wire(3, 2, POSITIVE, 0, &[1, 0, 0]), "100000000"),
            (wire(1, 2, POSITIVE, 0, &[1]), "100000000"),
            (wire(0, 0, NAN, 0, &[]), "NaN"),
            (wire(0, 0, PLUS_INFINITY, 0, &[]), "Infinity"),
            (wire(0, 0, MINUS_INFINITY, 0, &[]), "-Infinity"),
        ] {
            assert_eq!(decode(&bytes).as_deref(), Some(text), "{text}");
        }
        assert_eq!(decode(&[0, 1]), None);
    }

    #[test]
    fn encodes_as_postgres_would_and_round_trips() {
        assert_eq!(encode("12.50"), Some(wire(2, 0, POSITIVE, 2, &[12, 5000])));
        assert_eq!(encode("-12345"), Some(wire(2, 1, NEGATIVE, 0, &[1, 2345])));
        assert_eq!(encode("0.050"), Some(wire(1, -1, POSITIVE, 3, &[500])));
        assert_eq!(encode("100000000"), Some(wire(1, 2, POSITIVE, 0, &[1])));
        assert_eq!(encode("0"), Some(wire(0, 0, POSITIVE, 0, &[])));
        for text in [
            "0",
            "0.00",
            "12.50",
            "-12345",
            "0.050",
            "0.00000012",
            "123456789.123456789",
            "-0.5",
            "9999",
            "10000",
            "1",
            "NaN",
            "Infinity",
            "-Infinity",
        ] {
            assert_eq!(
                decode(&encode(text).unwrap()).as_deref(),
                Some(text),
                "{text}"
            );
        }
        for (text, printed) in [
            ("1.5e10", "15000000000"),
            ("1.5E-3", "0.0015"),
            ("+7", "7"),
            (".5", "0.5"),
            ("5.", "5"),
        ] {
            assert_eq!(
                decode(&encode(text).unwrap()).as_deref(),
                Some(printed),
                "{text}"
            );
        }
        for bad in ["", "-", "1.2.3", "abc", "1e", "1e5x", "--1", "٣"] {
            assert_eq!(encode(bad), None, "{bad}");
        }
    }
}
