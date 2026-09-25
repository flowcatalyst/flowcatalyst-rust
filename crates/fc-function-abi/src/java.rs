//! `String.isBlank`, the one piece of JDK behaviour the ABI keeps: whether a
//! reason or a dedup id is blank is decided the same way on both hosts.

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

#[cfg(test)]
mod tests {
    use super::*;

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
