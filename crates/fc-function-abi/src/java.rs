//! `String.isBlank` (`Character.isWhitespace`): the one JDK rule shared by
//! the guest ABI, the function model, the platform, the host and the
//! signature verifier, so "blank" is decided the same way everywhere, and
//! the same way as on Java. Not guest API; `fc-function-model` re-exports it.

/// `Character.isWhitespace`, the rule `String.isBlank` uses: the ASCII
/// controls `\t \n \x0B \f \r \x1C-\x1F`, and every Unicode space, line or
/// paragraph separator except the no-break ones (U+00A0, U+2007, U+202F).
/// U+0085 is not whitespace to Java. Differs from `char::is_whitespace`.
pub fn is_java_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | '\u{1C}'..='\u{1F}' | ' ' | '\u{1680}'
            | '\u{2000}'..='\u{2006}' | '\u{2008}'..='\u{200A}' | '\u{2028}' | '\u{2029}'
            | '\u{205F}' | '\u{3000}'
    )
}

/// `String.isBlank`: empty, or only [`is_java_whitespace`] characters.
pub fn is_blank(s: &str) -> bool {
    s.chars().all(is_java_whitespace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_follows_character_is_whitespace() {
        assert!(is_blank(""));
        assert!(is_blank(" \t\n"));
        assert!(is_blank(" \t\u{0B}\u{1C}\u{2003}\u{3000}"));
        assert!(!is_blank("\u{00A0}"));
        assert!(!is_blank("\u{0085}"));
        assert!(!is_blank("\u{2007}"));
        assert!(!is_blank("\u{202F}"));
        assert!(!is_blank(" x "));
        assert!(!is_blank("x"));
    }

    /// The explicit table agrees with the definition (`char::is_whitespace`
    /// minus the no-break spaces and U+0085, plus `\x1C-\x1F`), over every
    /// `char`.
    #[test]
    fn whitespace_table_matches_the_unicode_definition() {
        let by_definition = |c: char| {
            matches!(c, '\u{1C}'..='\u{1F}')
                || (c.is_whitespace()
                    && !matches!(c, '\u{A0}' | '\u{2007}' | '\u{202F}' | '\u{85}'))
        };
        for c in (0..=0x10FFFF).filter_map(char::from_u32) {
            assert_eq!(is_java_whitespace(c), by_definition(c), "{:?}", c);
        }
    }
}
