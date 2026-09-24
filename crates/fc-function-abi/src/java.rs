//! Small re-statements of JDK behaviour that is observable on the ABI's wire.
//! Each cites the JDK method it mirrors; the golden tables in
//! `tests/data/java-golden/` pin them against a real JVM.

/// `new String(bytes, StandardCharsets.UTF_8)`: the JDK's lossy UTF-8
/// decoder (`java.lang.String.decodeUTF8_UTF16`, JDK 25), as UTF-16 code
/// units. Malformed input becomes U+FFFD with the JDK's own grouping, which
/// differs from Rust's `from_utf8_lossy` (for example an encoded surrogate
/// `ED A0 80` is one replacement in Java, three in Rust).
pub(crate) fn decode_utf8_lossy(src: &[u8]) -> Vec<u16> {
    const REPL: u16 = 0xFFFD;
    let not_cont = |b: u8| b & 0xC0 != 0x80;
    let mut dst = Vec::with_capacity(src.len());
    let sl = src.len();
    let mut sp = 0;
    while sp < sl {
        let b1 = src[sp];
        sp += 1;
        if b1 < 0x80 {
            dst.push(u16::from(b1));
        } else if b1 >> 5 == 0b110 && b1 & 0x1E != 0 {
            // two bytes: [C2..DF] [80..BF]
            if sp < sl {
                let b2 = src[sp];
                sp += 1;
                if not_cont(b2) {
                    dst.push(REPL);
                    sp -= 1;
                } else {
                    dst.push((u16::from(b1 & 0x1F) << 6) | u16::from(b2 & 0x3F));
                }
                continue;
            }
            dst.push(REPL);
            break;
        } else if b1 >> 4 == 0b1110 {
            if sp + 1 < sl {
                let (b2, b3) = (src[sp], src[sp + 1]);
                sp += 2;
                if is_malformed3(b1, b2, b3) {
                    dst.push(REPL);
                    sp -= 3;
                    sp += malformed3(src, sp);
                } else {
                    let c = (u16::from(b1 & 0x0F) << 12)
                        | (u16::from(b2 & 0x3F) << 6)
                        | u16::from(b3 & 0x3F);
                    dst.push(if (0xD800..=0xDFFF).contains(&c) {
                        REPL
                    } else {
                        c
                    });
                }
                continue;
            }
            if sp < sl && is_malformed3_2(b1, src[sp]) {
                dst.push(REPL);
                continue;
            }
            dst.push(REPL);
            break;
        } else if b1 >> 3 == 0b11110 {
            if sp + 2 < sl {
                let (b2, b3, b4) = (src[sp], src[sp + 1], src[sp + 2]);
                sp += 3;
                let uc = (u32::from(b1 & 0x07) << 18)
                    | (u32::from(b2 & 0x3F) << 12)
                    | (u32::from(b3 & 0x3F) << 6)
                    | u32::from(b4 & 0x3F);
                if not_cont(b2)
                    || not_cont(b3)
                    || not_cont(b4)
                    || !(0x10000..=0x10FFFF).contains(&uc)
                {
                    dst.push(REPL);
                    sp -= 4;
                    sp += malformed4(src, sp);
                } else {
                    let v = uc - 0x10000;
                    dst.push(0xD800 | (v >> 10) as u16);
                    dst.push(0xDC00 | (v & 0x3FF) as u16);
                }
                continue;
            }
            if b1 > 0xF4 || (sp < sl && is_malformed4_2(b1, src[sp])) {
                dst.push(REPL);
                continue;
            }
            sp += 1;
            dst.push(REPL);
            if sp < sl && not_cont(src[sp]) {
                continue;
            }
            break;
        } else {
            dst.push(REPL);
        }
    }
    dst
}

fn is_malformed3(b1: u8, b2: u8, b3: u8) -> bool {
    (b1 == 0xE0 && b2 & 0xE0 == 0x80) || b2 & 0xC0 != 0x80 || b3 & 0xC0 != 0x80
}

fn is_malformed3_2(b1: u8, b2: u8) -> bool {
    (b1 == 0xE0 && b2 & 0xE0 == 0x80) || b2 & 0xC0 != 0x80
}

fn is_malformed4_2(b1: u8, b2: u8) -> bool {
    (b1 == 0xF0 && !(0x90..=0xBF).contains(&b2))
        || (b1 == 0xF4 && b2 & 0xF0 != 0x80)
        || b2 & 0xC0 != 0x80
}

fn malformed3(src: &[u8], sp: usize) -> usize {
    let (b1, b2) = (src[sp], src[sp + 1]);
    if (b1 == 0xE0 && b2 & 0xE0 == 0x80) || b2 & 0xC0 != 0x80 {
        1
    } else {
        2
    }
}

fn malformed4(src: &[u8], sp: usize) -> usize {
    let (b1, b2) = (src[sp], src[sp + 1]);
    if b1 > 0xF4
        || (b1 == 0xF0 && !(0x90..=0xBF).contains(&b2))
        || (b1 == 0xF4 && b2 & 0xF0 != 0x80)
        || b2 & 0xC0 != 0x80
    {
        1
    } else if src[sp + 2] & 0xC0 != 0x80 {
        2
    } else {
        3
    }
}

/// UTF-16 code units to a Rust string, each unpaired surrogate becoming `?`:
/// exactly the bytes Java's `String.getBytes(UTF_8)` produces for the same
/// string, which is how such a value reaches any UTF-8 wire from Java.
pub(crate) fn utf16_to_string(units: &[u16]) -> String {
    char::decode_utf16(units.iter().copied())
        .map(|r| r.unwrap_or('?'))
        .collect()
}

/// `Character.isWhitespace(int)`: Unicode space, line and paragraph
/// separators except the non-breaking ones (U+00A0, U+2007, U+202F), plus
/// U+0009-U+000D and U+001C-U+001F. Differs from `char::is_whitespace` (which
/// includes the non-breaking spaces and U+0085, and excludes U+001C-U+001F).
pub(crate) fn is_java_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{0009}'..='\u{000D}'
            | '\u{001C}'..='\u{001F}'
            | '\u{0020}'
            | '\u{1680}'
            | '\u{2000}'..='\u{2006}'
            | '\u{2008}'..='\u{200A}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{205F}'
            | '\u{3000}'
    )
}

/// `String.isBlank()`: empty, or every code point is [`is_java_whitespace`].
pub(crate) fn is_blank(s: &str) -> bool {
    s.chars().all(is_java_whitespace)
}

/// Java `function-api`'s `JsonEscape.escape`: `"` `\` and the short escapes,
/// every other C0 control and every non-ASCII UTF-16 code unit (0x7F
/// included) as a lower-case `\uXXXX`.
pub(crate) fn json_escape_ascii(raw: &str, out: &mut String) {
    for unit in raw.encode_utf16() {
        match unit {
            0x22 => out.push_str("\\\""),
            0x5C => out.push_str("\\\\"),
            0x08 => out.push_str("\\b"),
            0x0C => out.push_str("\\f"),
            0x0A => out.push_str("\\n"),
            0x0D => out.push_str("\\r"),
            0x09 => out.push_str("\\t"),
            0x20..=0x7E => out.push(char::from(unit as u8)),
            _ => out.push_str(&format!("\\u{unit:04x}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lossy(bytes: &[u8]) -> String {
        utf16_to_string(&decode_utf8_lossy(bytes))
    }

    #[test]
    fn utf8_valid_round_trips() {
        let s = "a\u{e9}\u{20ac}\u{1F600}";
        assert_eq!(lossy(s.as_bytes()), s);
    }

    #[test]
    fn utf8_malformed_groups_like_the_jdk() {
        // An encoded surrogate is one replacement in Java.
        assert_eq!(lossy(&[b'a', 0xED, 0xA0, 0x80, b'b']), "a\u{FFFD}b");
        // C0 is never a valid lead; each byte is its own replacement.
        assert_eq!(lossy(&[0xC0, 0xAF]), "\u{FFFD}\u{FFFD}");
        // A truncated 3-byte sequence before an ASCII byte is one replacement.
        assert_eq!(lossy(&[0xE2, 0x82, b'x']), "\u{FFFD}x");
    }

    #[test]
    fn unpaired_surrogates_become_question_marks() {
        assert_eq!(utf16_to_string(&[0x61, 0xD800, 0x62]), "a?b");
        assert_eq!(utf16_to_string(&[0xDE00, 0xD83D]), "??");
        assert_eq!(utf16_to_string(&[0xD83D, 0xDE00]), "\u{1F600}");
    }

    #[test]
    fn blank_follows_character_is_whitespace() {
        assert!(is_blank(""));
        assert!(is_blank(" \t\u{000B}\u{001C}\u{2003}\u{3000}"));
        assert!(!is_blank("\u{00A0}"));
        assert!(!is_blank("\u{0085}"));
        assert!(!is_blank("\u{2007}"));
        assert!(!is_blank(" x "));
    }
}
