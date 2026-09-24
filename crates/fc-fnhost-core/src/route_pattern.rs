//! An endpoint's route path pattern (Java `platform/function/RoutePattern.java`,
//! spec `function-registry.md` §5.2-5.3): literal segments, `{param}`
//! segments and a trailing `*` (rest) segment.

use std::cmp::Ordering;

use indexmap::IndexMap;

use crate::java::utf16_len;

const MAX_LENGTH: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    /// Compared byte for byte, case-sensitively, with the raw (undecoded)
    /// request segment.
    Literal(String),
    /// Matches one non-empty segment; the captured value is percent-decoded.
    Param(String),
    /// The trailing `*`: zero or more remaining segments.
    Rest,
}

impl Segment {
    fn rank(&self) -> u8 {
        match self {
            Segment::Literal(_) => 0,
            Segment::Param(_) => 1,
            Segment::Rest => 2,
        }
    }
}

/// The captured `{param}` values, in declaration order.
pub type PathParams = IndexMap<String, String>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutePattern {
    value: String,
    segments: Vec<Segment>,
}

impl RoutePattern {
    /// Java `RoutePattern.parse`; `None` where Java throws
    /// `ROUTE_PATTERN_INVALID`.
    pub fn parse(raw: &str) -> Option<Self> {
        if !raw.starts_with('/') || utf16_len(raw) > MAX_LENGTH {
            return None;
        }
        if raw == "/" {
            return Some(Self {
                value: raw.to_owned(),
                segments: Vec::new(),
            });
        }
        let parts: Vec<&str> = raw[1..].split('/').collect();
        let mut segments = Vec::with_capacity(parts.len());
        let mut names = std::collections::HashSet::new();
        for (i, part) in parts.iter().enumerate() {
            let segment = parse_segment(part, i == parts.len() - 1)?;
            if let Segment::Param(name) = &segment {
                if !names.insert(name.clone()) {
                    return None;
                }
            }
            segments.push(segment);
        }
        Some(Self {
            value: raw.to_owned(),
            segments,
        })
    }

    /// The pattern exactly as parsed.
    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// The captured params when `path` (the raw path, no query) matches.
    /// A param's value is percent-decoded as UTF-8 (`+` stays `+`); a
    /// malformed escape or invalid UTF-8 is no match. Without `Rest` the
    /// segment counts must be equal, so a trailing slash (an empty last
    /// segment) matches nothing but `Rest`.
    pub fn matches(&self, path: &str) -> Option<PathParams> {
        if !path.starts_with('/') {
            return None;
        }
        let parts: Vec<&str> = if path == "/" {
            Vec::new()
        } else {
            path[1..].split('/').collect()
        };
        let mut params = PathParams::new();
        let mut i = 0;
        while i < self.segments.len() {
            let segment = &self.segments[i];
            if *segment == Segment::Rest {
                return Some(params);
            }
            let raw = parts.get(i)?;
            if raw.is_empty() {
                return None;
            }
            match segment {
                Segment::Literal(literal) => {
                    if literal != raw {
                        return None;
                    }
                }
                Segment::Param(name) => {
                    params.insert(name.clone(), percent_decode(raw)?);
                }
                Segment::Rest => unreachable!("handled above"),
            }
            i += 1;
        }
        if i < parts.len() {
            return None;
        }
        Some(params)
    }

    /// Sorts `patterns` most specific first and returns the first that
    /// matches `path`, with its index in `patterns` (Java `firstMatch`).
    pub fn first_match<'a>(
        patterns: impl IntoIterator<Item = &'a RoutePattern>,
        path: &str,
    ) -> Option<(usize, PathParams)> {
        let mut ordered: Vec<(usize, &RoutePattern)> = patterns.into_iter().enumerate().collect();
        ordered.sort_by(|a, b| a.1.cmp(b.1)); // stable, as Java's sorted stream
        ordered
            .into_iter()
            .find_map(|(index, pattern)| pattern.matches(path).map(|params| (index, params)))
    }
}

/// More specific first: segment by segment, `Literal` before `Param` before
/// `Rest`; a pattern that runs out first sorts first; a tie breaks by value.
impl Ord for RoutePattern {
    fn cmp(&self, other: &Self) -> Ordering {
        for (a, b) in self.segments.iter().zip(other.segments.iter()) {
            match a.rank().cmp(&b.rank()) {
                Ordering::Equal => {}
                unequal => return unequal,
            }
        }
        self.segments
            .len()
            .cmp(&other.segments.len())
            .then_with(|| self.value.cmp(&other.value))
    }
}

impl PartialOrd for RoutePattern {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn parse_segment(part: &str, is_last: bool) -> Option<Segment> {
    if part == "*" {
        return is_last.then_some(Segment::Rest);
    }
    if part.len() >= 2 && part.starts_with('{') && part.ends_with('}') {
        let name = &part[1..part.len() - 1];
        return is_param_name(name).then(|| Segment::Param(name.to_owned()));
    }
    is_literal(part).then(|| Segment::Literal(part.to_owned()))
}

/// `^[A-Za-z0-9._~-]+$`
fn is_literal(part: &str) -> bool {
    !part.is_empty()
        && part
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'~' | b'-'))
}

/// `^[A-Za-z][A-Za-z0-9_]*$`
fn is_param_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b) if b.is_ascii_alphabetic())
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// One path segment, UTF-8, strict: `None` on a malformed escape, a raw
/// non-ASCII character, or an invalid UTF-8 sequence.
fn percent_decode(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                if i + 2 >= bytes.len() {
                    return None;
                }
                let hi = (bytes[i + 1] as char).to_digit(16)?;
                let lo = (bytes[i + 2] as char).to_digit(16)?;
                out.push(((hi << 4) | lo) as u8);
                i += 3;
            }
            b if b <= 0x7F => {
                out.push(b);
                i += 1;
            }
            _ => return None,
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(raw: &str) -> RoutePattern {
        RoutePattern::parse(raw).unwrap_or_else(|| panic!("{raw} should parse"))
    }

    #[test]
    fn parse_rules() {
        for ok in ["/", "/a", "/a/{id}", "/a/*", "/{x}/{y}/*", "/a.b_c~d-e"] {
            assert!(RoutePattern::parse(ok).is_some(), "{ok}");
        }
        for bad in [
            "", "a", "/a/*/b", "/{1x}", "/{x}/{x}", "/a b", "/a//b", "/{}", "/%20",
        ] {
            assert!(RoutePattern::parse(bad).is_none(), "{bad}");
        }
        assert!(RoutePattern::parse(&format!("/{}", "a".repeat(1024))).is_none());
    }

    #[test]
    fn matching_rules() {
        assert_eq!(
            p("/echo/{id}").matches("/echo/42").unwrap()["id"],
            "42".to_owned()
        );
        assert_eq!(
            p("/echo/{id}").matches("/echo/a%20b").unwrap()["id"],
            "a b".to_owned()
        );
        assert_eq!(
            p("/echo/{id}").matches("/echo/a+b").unwrap()["id"],
            "a+b".to_owned()
        );
        assert!(p("/echo/{id}").matches("/echo/%zz").is_none());
        assert!(p("/echo/{id}").matches("/echo/%C3").is_none());
        assert!(p("/echo/{id}").matches("/echo/").is_none());
        assert!(p("/echo/{id}").matches("/echo/1/2").is_none());
        assert!(p("/echo").matches("/echo/").is_none());
        assert!(p("/echo/*").matches("/echo").is_some());
        assert!(p("/echo/*").matches("/echo/").is_some());
        assert!(p("/echo/*").matches("/echo/a/b").is_some());
        assert!(p("/").matches("/").is_some());
        assert!(p("/").matches("/x").is_none());
        assert!(p("/*").matches("/").is_some());
        assert!(p("/Echo").matches("/echo").is_none());
        assert!(p("/x").matches("").is_none());
    }

    #[test]
    fn more_specific_first() {
        let patterns = [p("/*"), p("/a/{id}"), p("/a/b"), p("/a/*")];
        assert_eq!(RoutePattern::first_match(&patterns, "/a/b").unwrap().0, 2);
        assert_eq!(RoutePattern::first_match(&patterns, "/a/c").unwrap().0, 1);
        assert_eq!(RoutePattern::first_match(&patterns, "/a/c/d").unwrap().0, 3);
        assert_eq!(RoutePattern::first_match(&patterns, "/z").unwrap().0, 0);
        assert!(RoutePattern::first_match(&[p("/a")], "/b").is_none());
    }
}
