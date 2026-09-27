//! JDBC-style `?` placeholders to PostgreSQL's `$n` (what pgjdbc does for
//! Java's `fc_db_*`): a `?` outside quotes and comments is the next
//! parameter; `??` is a literal `?` (for the jsonb operators `?`, `?|`,
//! `?&`). Skipped, as PostgreSQL's lexer reads them: `'…'` strings (`''`
//! escapes, and backslash escapes in `E'…'`), `"…"` identifiers, `--`
//! comments, nested `/* … */` comments and `$tag$ … $tag$` strings.

/// The rewritten SQL and how many parameters it takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewritten {
    pub sql: String,
    pub placeholders: usize,
}

pub fn rewrite(sql: &str) -> Rewritten {
    let bytes = sql.as_bytes();
    let mut out = String::with_capacity(sql.len() + 8);
    let mut n = 0;
    let mut i = 0;
    // Copies `sql[from..to]` verbatim.
    let copy = |out: &mut String, from: usize, to: usize| out.push_str(&sql[from..to]);
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'?' => {
                if bytes.get(i + 1) == Some(&b'?') {
                    out.push('?');
                    i += 2;
                } else {
                    n += 1;
                    out.push('$');
                    out.push_str(&n.to_string());
                    i += 1;
                }
            }
            b'\'' => {
                let escapes = i > 0
                    && matches!(bytes[i - 1], b'e' | b'E')
                    && (i < 2 || !is_ident_byte(bytes[i - 2]));
                let end = skip_quoted(bytes, i, b'\'', escapes);
                copy(&mut out, i, end);
                i = end;
            }
            b'"' => {
                let end = skip_quoted(bytes, i, b'"', false);
                copy(&mut out, i, end);
                i = end;
            }
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                let end = bytes[i..]
                    .iter()
                    .position(|&c| c == b'\n')
                    .map_or(bytes.len(), |p| i + p + 1);
                copy(&mut out, i, end);
                i = end;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let end = skip_block_comment(bytes, i);
                copy(&mut out, i, end);
                i = end;
            }
            b'$' if i == 0 || !is_ident_byte(bytes[i - 1]) => match dollar_tag(bytes, i) {
                Some(tag_end) => {
                    let tag = &bytes[i..tag_end];
                    let end = find(bytes, tag_end, tag).map_or(bytes.len(), |p| p + tag.len());
                    copy(&mut out, i, end);
                    i = end;
                }
                None => {
                    out.push('$');
                    i += 1;
                }
            },
            _ => {
                // Copy a whole UTF-8 sequence (never splits one: the bytes
                // matched above are all ASCII).
                let len = utf8_len(b);
                copy(&mut out, i, (i + len).min(bytes.len()));
                i += len;
            }
        }
    }
    Rewritten {
        sql: out,
        placeholders: n,
    }
}

fn utf8_len(first: u8) -> usize {
    match first {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

/// The index just past the closing quote (or the end of the text). A
/// doubled quote is an escaped one; with `backslash`, so is `\'`.
fn skip_quoted(bytes: &[u8], start: usize, quote: u8, backslash: bool) -> usize {
    let mut i = start + 1;
    while i < bytes.len() {
        let b = bytes[i];
        if backslash && b == b'\\' {
            i += 2;
            continue;
        }
        if b == quote {
            if bytes.get(i + 1) == Some(&quote) {
                i += 2;
                continue;
            }
            return i + 1;
        }
        i += 1;
    }
    bytes.len()
}

fn skip_block_comment(bytes: &[u8], start: usize) -> usize {
    let mut depth = 0;
    let mut i = start;
    while i + 1 < bytes.len() {
        if bytes[i] == b'/' && bytes[i + 1] == b'*' {
            depth += 1;
            i += 2;
        } else if bytes[i] == b'*' && bytes[i + 1] == b'/' {
            depth -= 1;
            i += 2;
            if depth == 0 {
                return i;
            }
        } else {
            i += 1;
        }
    }
    bytes.len()
}

/// `$tag$` starting at `start`: the index just past its closing `$`. A tag
/// is empty or an identifier that does not start with a digit (`$1` is a
/// positional parameter, not a quote).
fn dollar_tag(bytes: &[u8], start: usize) -> Option<usize> {
    let mut i = start + 1;
    if let Some(&first) = bytes.get(i) {
        if first.is_ascii_digit() {
            return None;
        }
    }
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'$' {
            return Some(i + 1);
        }
        if !is_ident_byte(b) {
            return None;
        }
        i += 1;
    }
    None
}

fn find(haystack: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| from + p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(sql: &str) -> (String, usize) {
        let out = rewrite(sql);
        (out.sql, out.placeholders)
    }

    #[test]
    fn placeholders_become_positional_parameters() {
        assert_eq!(
            r("SELECT * FROM t WHERE a = ? AND b IN (?, ?)"),
            ("SELECT * FROM t WHERE a = $1 AND b IN ($2, $3)".into(), 3)
        );
        assert_eq!(r("SELECT 1"), ("SELECT 1".into(), 0));
    }

    #[test]
    fn a_doubled_question_mark_is_a_literal_one() {
        assert_eq!(
            r("SELECT doc ?? 'k', doc ??| ? FROM t WHERE id = ?"),
            ("SELECT doc ? 'k', doc ?| $1 FROM t WHERE id = $2".into(), 2)
        );
    }

    #[test]
    fn quotes_comments_and_dollar_strings_are_left_alone() {
        for (sql, expected, n) in [
            ("SELECT '?', ?", "SELECT '?', $1", 1),
            ("SELECT 'it''s ?', ?", "SELECT 'it''s ?', $1", 1),
            (r"SELECT E'\' ?', ?", r"SELECT E'\' ?', $1", 1),
            (r"SELECT '\', ?", r"SELECT '\', $1", 1),
            (
                "SELECT \"a?b\" FROM t WHERE x = ?",
                "SELECT \"a?b\" FROM t WHERE x = $1",
                1,
            ),
            ("SELECT ? -- what?\n, ?", "SELECT $1 -- what?\n, $2", 2),
            (
                "SELECT /* ? /* ? */ ? */ ?",
                "SELECT /* ? /* ? */ ? */ $1",
                1,
            ),
            ("SELECT $$ ? $$, ?", "SELECT $$ ? $$, $1", 1),
            ("SELECT $x$ ? $y$ ? $x$, ?", "SELECT $x$ ? $y$ ? $x$, $1", 1),
            ("SELECT a$b, ?", "SELECT a$b, $1", 1),
            ("SELECT $1, ?", "SELECT $1, $1", 1),
            ("SELECT 'é?', ?", "SELECT 'é?', $1", 1),
            ("SELECT ?, 'unterminated ?", "SELECT $1, 'unterminated ?", 1),
        ] {
            assert_eq!(r(sql), (expected.to_string(), n), "{sql}");
        }
    }
}
