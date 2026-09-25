//! Query-string decoding, exactly as the host's own (`fc-fnhost-core`
//! `listener/pipeline.rs::collect_query`, which is
//! `url::form_urlencoded::parse`): `&`-separated, empty pairs skipped, a key
//! without `=` has the value `""`, `+` is a space, `%XX` is decoded (a
//! malformed escape is kept as written), the bytes are read as UTF-8 with
//! U+FFFD for anything invalid, and repeated keys keep every value in order.

use fc_function_abi::MultiMap;

pub(crate) fn parse(query: &str) -> MultiMap {
    let mut out = MultiMap::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        out.entry(decode(key)).or_default().push(decode(value));
    }
    out
}

fn decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' => match escaped(bytes.get(i + 1..i + 3)) {
                Some(byte) => {
                    out.push(byte);
                    i += 2;
                }
                None => out.push(b'%'),
            },
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The byte a `%XX` escape's two digits stand for.
fn escaped(digits: Option<&[u8]>) -> Option<u8> {
    match digits? {
        [hi, lo] => Some(hex(*hi)? << 4 | hex(*lo)?),
        _ => None,
    }
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The host's own case (pipeline.rs
    // query_decoding_keeps_repeats_and_reads_plus_as_space).
    #[test]
    fn repeats_are_kept_and_plus_is_a_space() {
        let query = parse("y=hello+world&y=again&z=%41&flag&&");
        assert_eq!(query["y"], ["hello world", "again"]);
        assert_eq!(query["z"], ["A"]);
        assert_eq!(query["flag"], [""]);
        assert_eq!(query.keys().collect::<Vec<_>>(), ["y", "z", "flag"]);
    }

    #[test]
    fn escapes_decode_as_utf8_and_malformed_ones_stay_as_written() {
        let query = parse("a=%C3%A9t%C3%A9&b=100%&c=%zz&d=%2&e=%ff&k%3Dx=v%3D1");
        assert_eq!(query["a"], ["été"]);
        assert_eq!(query["b"], ["100%"]);
        assert_eq!(query["c"], ["%zz"]);
        assert_eq!(query["d"], ["%2"]);
        assert_eq!(query["e"], ["\u{fffd}"]);
        assert_eq!(query["k=x"], ["v=1"]);
    }

    #[test]
    fn only_the_first_equals_splits() {
        assert_eq!(parse("a=b=c")["a"], ["b=c"]);
        assert_eq!(parse("=v")[""], ["v"]);
        assert!(parse("").is_empty());
    }
}
