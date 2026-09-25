//! Java `function/RoutePattern.java`.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::fmt;

use indexmap::IndexMap;

use crate::ValidationError;

/// A path pattern segment.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

/// An HTTP route path pattern: literal segments, `{param}` segments and a
/// trailing `*`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RoutePattern {
    value: String,
    segments: Vec<Segment>,
}

/// The `{param}` values a match captured, in declaration order.
pub type PathParams = IndexMap<String, String>;

/// One match: the pattern and the params it captured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteMatch<'a> {
    pub pattern: &'a RoutePattern,
    pub params: PathParams,
}

const MAX_LENGTH: usize = 1024;

impl RoutePattern {
    pub const INVALID_MESSAGE: &'static str =
        "route path must start with '/' and contain only literal, {param} or trailing '*' segments";

    /// `ROUTE_PATTERN_INVALID` for anything that is not a pattern.
    pub fn parse(raw: &str) -> Result<RoutePattern, ValidationError> {
        Self::try_parse(raw)
            .ok_or_else(|| ValidationError::new("ROUTE_PATTERN_INVALID", Self::INVALID_MESSAGE))
    }

    /// [`RoutePattern::parse`] without the error.
    pub fn try_parse(raw: &str) -> Option<RoutePattern> {
        if !raw.starts_with('/') || raw.len() > MAX_LENGTH {
            return None;
        }
        if raw == "/" {
            return Some(RoutePattern {
                value: raw.to_string(),
                segments: Vec::new(),
            });
        }
        let parts: Vec<&str> = raw[1..].split('/').collect();
        let mut segments = Vec::with_capacity(parts.len());
        let mut names = HashSet::new();
        for (i, part) in parts.iter().enumerate() {
            let segment = parse_segment(part, i == parts.len() - 1)?;
            if let Segment::Param(name) = &segment {
                if !names.insert(name.clone()) {
                    return None;
                }
            }
            segments.push(segment);
        }
        Some(RoutePattern {
            value: raw.to_string(),
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

    /// Whether every segment is a literal (a subscription's, schedule's or
    /// public route's path must be).
    pub fn is_literal(&self) -> bool {
        self.segments
            .iter()
            .all(|s| matches!(s, Segment::Literal(_)))
    }

    /// The captured params when `path` (raw, no query string) matches.
    ///
    /// Literals compare with the raw segment; a param takes one non-empty
    /// segment, percent-decoded as UTF-8 (`+` is not a space; a bad escape or
    /// bad UTF-8 is no match); `Rest` takes zero or more segments; without
    /// `Rest` the segment counts must be equal, so a trailing slash matches
    /// only `Rest`.
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
                Segment::Rest => unreachable!("Rest handled above"),
            }
            i += 1;
        }
        (i >= parts.len()).then_some(params)
    }

    /// True when the patterns have the same length and, position by
    /// position, are the same literal, both params (names aside) or both
    /// `Rest`: exactly the pairs the specificity order cannot separate.
    pub fn ambiguous_with(&self, other: &RoutePattern) -> bool {
        self.segments.len() == other.segments.len()
            && self
                .segments
                .iter()
                .zip(&other.segments)
                .all(|(a, b)| match (a, b) {
                    (Segment::Literal(x), Segment::Literal(y)) => x == y,
                    (Segment::Param(_), Segment::Param(_)) => true,
                    (Segment::Rest, Segment::Rest) => true,
                    _ => false,
                })
    }

    /// Sorts `patterns` most specific first and returns the first that
    /// matches `path`. The sort is stable, so of identical patterns the
    /// first given wins.
    pub fn first_match<'a>(
        patterns: impl IntoIterator<Item = &'a RoutePattern>,
        path: &str,
    ) -> Option<RouteMatch<'a>> {
        let mut sorted: Vec<&RoutePattern> = patterns.into_iter().collect();
        sorted.sort();
        sorted.into_iter().find_map(|pattern| {
            pattern
                .matches(path)
                .map(|params| RouteMatch { pattern, params })
        })
    }
}

/// Most specific first: segment by segment from the left, literal before
/// param before rest; a pattern that runs out first sorts first; a tie is
/// broken by the pattern text, so only identical patterns compare equal.
impl Ord for RoutePattern {
    fn cmp(&self, other: &Self) -> Ordering {
        for (a, b) in self.segments.iter().zip(&other.segments) {
            let by_rank = a.rank().cmp(&b.rank());
            if by_rank != Ordering::Equal {
                return by_rank;
            }
        }
        self.segments
            .len()
            .cmp(&other.segments.len())
            // Patterns are ASCII, so byte order is Java's String order.
            .then_with(|| self.value.cmp(&other.value))
    }
}

impl PartialOrd for RoutePattern {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for RoutePattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.value)
    }
}

fn parse_segment(part: &str, is_last: bool) -> Option<Segment> {
    if part == "*" {
        return is_last.then_some(Segment::Rest);
    }
    if part.len() >= 2 && part.starts_with('{') && part.ends_with('}') {
        let name = &part[1..part.len() - 1];
        let mut chars = name.chars();
        let valid = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
        return valid.then(|| Segment::Param(name.to_string()));
    }
    let literal = !part.is_empty()
        && part
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'~' | b'-'));
    literal.then(|| Segment::Literal(part.to_string()))
}

/// One path segment, percent-decoded as UTF-8; `None` on a malformed escape,
/// a raw non-ASCII character or invalid UTF-8. (Java's `Character.digit`
/// also takes non-ASCII digits in an escape; ASCII hex only here.)
fn percent_decode(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hi = (*bytes.get(i + 1)? as char).to_digit(16)?;
                let lo = (*bytes.get(i + 2)? as char).to_digit(16)?;
                out.push((hi * 16 + lo) as u8);
                i += 3;
            }
            b if b.is_ascii() => {
                out.push(b);
                i += 1;
            }
            _ => return None,
        }
    }
    String::from_utf8(out).ok()
}

/// Java `RoutePatternTest`.
#[cfg(test)]
mod tests {
    use super::*;

    fn p(raw: &str) -> RoutePattern {
        RoutePattern::parse(raw).unwrap()
    }

    fn lit(s: &str) -> Segment {
        Segment::Literal(s.into())
    }

    fn param(s: &str) -> Segment {
        Segment::Param(s.into())
    }

    fn assert_rejected(raw: &str) {
        let err = RoutePattern::parse(raw).unwrap_err();
        assert_eq!(err.code(), "ROUTE_PATTERN_INVALID", "{raw:?}");
        assert_eq!(err.message(), RoutePattern::INVALID_MESSAGE);
    }

    fn params(pairs: &[(&str, &str)]) -> IndexMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    // ── parse ───────────────────────────────────────────────────────────

    #[test]
    fn leading_slash() {
        for raw in ["/", "/invoices"] {
            assert_eq!(p(raw).value(), raw);
        }
        assert_rejected("invoices");
        assert_rejected("");
    }

    #[test]
    fn literal_segments() {
        assert_eq!(
            p("/v1/invoices.json").segments(),
            [lit("v1"), lit("invoices.json")]
        );
        for raw in ["/in voices", "/a%20b", "/a?b", "/a#b"] {
            assert_rejected(raw);
        }
    }

    #[test]
    fn param_segments() {
        assert_eq!(
            p("/invoices/{id}").segments(),
            [lit("invoices"), param("id")]
        );
        assert_eq!(
            p("/a/{x}/b/{y}").segments(),
            [lit("a"), param("x"), lit("b"), param("y")]
        );
        for raw in ["/{}", "/{1d}", "/a{id}", "/{id}x", "/{a}/{a}"] {
            assert_rejected(raw);
        }
    }

    #[test]
    fn rest_segments() {
        for raw in ["/files/*", "/*"] {
            assert_eq!(p(raw).segments().last(), Some(&Segment::Rest));
        }
        for raw in ["/*/a", "/a*", "/**"] {
            assert_rejected(raw);
        }
    }

    #[test]
    fn empty_segments_rejected() {
        for raw in ["//a", "/a//b", "/a/"] {
            assert_rejected(raw);
        }
    }

    #[test]
    fn length_limit_is_1024() {
        let ok = format!("/{}", "a".repeat(1023));
        assert_eq!(p(&ok).value(), ok);
        assert_rejected(&format!("/{}", "a".repeat(1024)));
    }

    // ── match ───────────────────────────────────────────────────────────

    #[test]
    fn literal_matches_byte_for_byte_case_sensitive() {
        let pattern = p("/Invoices");
        assert!(pattern.matches("/Invoices").is_some());
        assert!(pattern.matches("/invoices").is_none());
    }

    #[test]
    fn params_are_captured_and_decoded() {
        assert_eq!(
            p("/invoices/{id}").matches("/invoices/abc-123"),
            Some(params(&[("id", "abc-123")]))
        );
        let search = p("/search/{term}");
        assert_eq!(
            search.matches("/search/caf%C3%A9"),
            Some(params(&[("term", "café")]))
        );
        assert_eq!(
            search.matches("/search/a+b"),
            Some(params(&[("term", "a+b")]))
        );
        assert_eq!(search.matches("/search/a%2"), None);
        assert_eq!(search.matches("/search/a%zz"), None);
        assert_eq!(search.matches("/search/%FF"), None);
    }

    #[test]
    fn rest_matches_zero_or_more_segments() {
        let pattern = p("/files/*");
        for path in ["/files", "/files/", "/files/a/b"] {
            assert!(pattern.matches(path).is_some(), "{path}");
        }
    }

    #[test]
    fn without_rest_segment_counts_must_be_equal() {
        let pattern = p("/a");
        assert!(pattern.matches("/a").is_some());
        assert!(pattern.matches("/a/").is_none());
        assert!(pattern.matches("/a/b").is_none());
        assert!(pattern.matches("").is_none());
    }

    #[test]
    fn literals_are_compared_raw_never_decoded() {
        let pattern = p("/a-b");
        assert!(pattern.matches("/a%2Db").is_none());
        assert!(pattern.matches("/a-b").is_some());
    }

    // ── precedence and ambiguity ────────────────────────────────────────

    fn winner<'a>(a: &'a RoutePattern, b: &'a RoutePattern, path: &str) -> RoutePattern {
        RoutePattern::first_match(&[a.clone(), b.clone()], path)
            .unwrap()
            .pattern
            .clone()
    }

    #[test]
    fn precedence() {
        let (a, b) = (p("/a/b"), p("/a/{x}"));
        assert_eq!(winner(&a, &b, "/a/b"), a);
        let (a, b) = (p("/a/{x}"), p("/a/*"));
        assert_eq!(winner(&a, &b, "/a/b"), a);
        let (a, b) = (p("/a/b/{y}"), p("/a/{x}/c"));
        assert_eq!(winner(&a, &b, "/a/b/c"), a);
        let (a, b) = (p("/files"), p("/files/*"));
        assert_eq!(winner(&a, &b, "/files"), a);
    }

    #[test]
    fn ambiguity() {
        assert!(p("/a/{x}").ambiguous_with(&p("/a/{y}")));
        assert!(p("/a/{y}").ambiguous_with(&p("/a/{x}")));
        assert!(p("/a/*").ambiguous_with(&p("/a/*")));
        assert_eq!(p("/a/*").cmp(&p("/a/*")), Ordering::Equal);
        assert_ne!(p("/a/{x}").cmp(&p("/a/{y}")), Ordering::Equal);
        assert!(!p("/a/{x}").ambiguous_with(&p("/a/b")));
        assert!(!p("/a/{x}").ambiguous_with(&p("/a/{x}/b")));
        assert!(!p("/a/b").ambiguous_with(&p("/a/c")));
    }

    #[test]
    fn first_match_sorts_by_specificity_and_returns_params() {
        let (literal, param_p, rest) = (p("/a/b"), p("/a/{x}"), p("/a/*"));
        let patterns = [rest, param_p.clone(), literal.clone()];
        let on_literal = RoutePattern::first_match(&patterns, "/a/b").unwrap();
        assert_eq!(on_literal.pattern, &literal);
        assert!(on_literal.params.is_empty());
        let on_param = RoutePattern::first_match(&patterns, "/a/c").unwrap();
        assert_eq!(on_param.pattern, &param_p);
        assert_eq!(on_param.params, params(&[("x", "c")]));
        assert!(RoutePattern::first_match(&[p("/a/b")], "/x/y").is_none());
    }

    // ── the function host's cases (formerly fc-fnhost-core's copy) ─────

    #[test]
    fn host_parse_rules() {
        for ok in ["/", "/a", "/a/{id}", "/a/*", "/{x}/{y}/*", "/a.b_c~d-e"] {
            assert!(RoutePattern::try_parse(ok).is_some(), "{ok}");
        }
        for bad in [
            "", "a", "/a/*/b", "/{1x}", "/{x}/{x}", "/a b", "/a//b", "/{}", "/%20",
        ] {
            assert!(RoutePattern::try_parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn host_matching_rules() {
        assert_eq!(p("/echo/{id}").matches("/echo/42").unwrap()["id"], "42");
        assert_eq!(p("/echo/{id}").matches("/echo/a%20b").unwrap()["id"], "a b");
        assert!(p("/echo/{id}").matches("/echo/%C3").is_none());
        assert!(p("/echo/{id}").matches("/echo/").is_none());
        assert!(p("/echo/{id}").matches("/echo/1/2").is_none());
        assert!(p("/").matches("/").is_some());
        assert!(p("/").matches("/x").is_none());
        assert!(p("/*").matches("/").is_some());
    }

    #[test]
    fn first_match_is_the_most_specific_and_the_first_of_identical_patterns() {
        let patterns = [p("/*"), p("/a/{id}"), p("/a/b"), p("/a/*")];
        let winner = |path: &str| RoutePattern::first_match(&patterns, path).map(|m| m.pattern);
        assert_eq!(winner("/a/b"), Some(&patterns[2]));
        assert_eq!(winner("/a/c"), Some(&patterns[1]));
        assert_eq!(winner("/a/c/d"), Some(&patterns[3]));
        assert_eq!(winner("/z"), Some(&patterns[0]));
        // Identical patterns compare equal; the stable sort keeps the first.
        let twins = [p("/x"), p("/x")];
        let m = RoutePattern::first_match(&twins, "/x").unwrap();
        assert!(std::ptr::eq(m.pattern, &twins[0]));
        // Any iterator of patterns, e.g. an endpoint list's paths.
        let owned = [p("/a/{id}"), p("/a/b")];
        let m = RoutePattern::first_match(owned.iter(), "/a/b").unwrap();
        assert!(std::ptr::eq(m.pattern, &owned[1]));
    }
}
