//! Java's host reads and writes the Extism envelope with Jackson 3.1
//! (`platform/shared/json/Json.MAPPER`), so this module restates the parts
//! of Jackson's behaviour that the envelope can observe:
//!
//! - [`read_tree`]: `ObjectMapper.readTree(byte[])` with Jackson's default
//!   `StreamReadConstraints` (nesting 500, number length 1000 digits, property
//!   name 50,000 UTF-8 bytes, string 100,000,000 chars), its UTF-8 decoding
//!   (overlong 2-byte sequences accepted, encoded surrogates refused), a
//!   leading BOM skipped, duplicate keys resolved "last value wins, first
//!   position kept", trailing tokens refused, and no content at all read as a
//!   missing root rather than an error. Input in UTF-16/32 (which Jackson
//!   would auto-detect) is not supported: it is refused as malformed.
//! - [`write_node`]: `writeValueAsBytes(JsonNode)`: floating-point numbers as
//!   Java's `Double.toString`, infinities as the strings `"Infinity"` /
//!   `"-Infinity"`, lone surrogates as `\uXXXX`.
//! - [`JacksonFormatter`]: a `serde_json` formatter whose string escaping is
//!   Jackson's (`\u001F`, upper-case hex; `/` and non-ASCII written raw).
//!
//! `tests/data/java-golden/{decode,emit}.tsv` pin all of this against Java.

use std::io;

use indexmap::IndexMap;

const MAX_DEPTH: usize = 500;
const MAX_NUM_LEN: usize = 1000;
const MAX_NAME_LEN: usize = 50_000;
const MAX_STRING_LEN: usize = 100_000_000;

/// A parsed JSON value, with strings as UTF-16 code units so that lone
/// surrogates (legal in Jackson string values) survive.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Node {
    Obj(IndexMap<Vec<u16>, Node>),
    Arr(Vec<Node>),
    Str(Vec<u16>),
    /// An integer literal's source text.
    Int(String),
    /// A floating-point literal's source text.
    Float(String),
    Bool(bool),
    Null,
}

impl Node {
    /// The member named `key`, when this is an object that has one.
    pub(crate) fn member(&self, key: &str) -> Option<&Node> {
        match self {
            Node::Obj(members) => {
                let key: Vec<u16> = key.encode_utf16().collect();
                members.get(&key)
            }
            _ => None,
        }
    }

    /// Jackson `JsonNode.isInt()` value: an integer literal that fits `i32`.
    pub(crate) fn as_i32(&self) -> Option<i32> {
        match self {
            Node::Int(text) => text.parse().ok(),
            _ => None,
        }
    }

    /// Jackson's `isMissingNode() || isNull()`, for an optional member.
    pub(crate) fn is_absent(node: Option<&Node>) -> bool {
        matches!(node, None | Some(Node::Null))
    }
}

/// The input was not JSON by Jackson's rules.
#[derive(Debug)]
pub(crate) struct Malformed;

/// `readTree(bytes)`: `Ok(None)` when there is no content at all (Jackson's
/// missing root), `Err` when the input is not a single valid JSON value.
pub(crate) fn read_tree(bytes: &[u8]) -> Result<Option<Node>, Malformed> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let mut p = Reader { s: bytes, pos: 0 };
    p.skip_ws();
    if p.pos == bytes.len() {
        return Ok(None);
    }
    let node = p.value(0)?;
    p.skip_ws();
    if p.pos != bytes.len() {
        return Err(Malformed);
    }
    Ok(Some(node))
}

struct Reader<'a> {
    s: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn skip_ws(&mut self) {
        while matches!(self.s.get(self.pos), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn next(&mut self) -> Result<u8, Malformed> {
        let b = *self.s.get(self.pos).ok_or(Malformed)?;
        self.pos += 1;
        Ok(b)
    }

    fn value(&mut self, depth: usize) -> Result<Node, Malformed> {
        match self.s.get(self.pos).ok_or(Malformed)? {
            b'{' => self.object(depth + 1),
            b'[' => self.array(depth + 1),
            b'"' => {
                self.pos += 1;
                self.string(false).map(Node::Str)
            }
            b't' => self.literal(b"true", Node::Bool(true)),
            b'f' => self.literal(b"false", Node::Bool(false)),
            b'n' => self.literal(b"null", Node::Null),
            b'-' | b'0'..=b'9' => self.number(),
            _ => Err(Malformed),
        }
    }

    fn object(&mut self, depth: usize) -> Result<Node, Malformed> {
        if depth > MAX_DEPTH {
            return Err(Malformed);
        }
        self.pos += 1;
        let mut members = IndexMap::new();
        self.skip_ws();
        if self.s.get(self.pos) == Some(&b'}') {
            self.pos += 1;
            return Ok(Node::Obj(members));
        }
        loop {
            self.skip_ws();
            if self.next()? != b'"' {
                return Err(Malformed);
            }
            let key = self.string(true)?;
            if utf8_len(&key) > MAX_NAME_LEN {
                return Err(Malformed);
            }
            self.skip_ws();
            if self.next()? != b':' {
                return Err(Malformed);
            }
            self.skip_ws();
            let value = self.value(depth)?;
            // `ObjectNode.set`: a repeated key replaces the value in place.
            members.insert(key, value);
            self.skip_ws();
            match self.next()? {
                b',' => {}
                b'}' => return Ok(Node::Obj(members)),
                _ => return Err(Malformed),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Node, Malformed> {
        if depth > MAX_DEPTH {
            return Err(Malformed);
        }
        self.pos += 1;
        let mut items = Vec::new();
        self.skip_ws();
        if self.s.get(self.pos) == Some(&b']') {
            self.pos += 1;
            return Ok(Node::Arr(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value(depth)?);
            self.skip_ws();
            match self.next()? {
                b',' => {}
                b']' => return Ok(Node::Arr(items)),
                _ => return Err(Malformed),
            }
        }
    }

    fn literal(&mut self, text: &[u8], node: Node) -> Result<Node, Malformed> {
        if self.s[self.pos..].starts_with(text) {
            self.pos += text.len();
            Ok(node)
        } else {
            Err(Malformed)
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.pos;
        while self.s.get(self.pos).is_some_and(u8::is_ascii_digit) {
            self.pos += 1;
        }
        self.pos - start
    }

    fn number(&mut self) -> Result<Node, Malformed> {
        let start = self.pos;
        if self.s[self.pos] == b'-' {
            self.pos += 1;
        }
        let int_start = self.pos;
        let int_len = self.digits();
        if int_len == 0 || (int_len > 1 && self.s[int_start] == b'0') {
            return Err(Malformed);
        }
        let mut frac_len = 0;
        let mut exp_len = 0;
        let mut float = false;
        if self.s.get(self.pos) == Some(&b'.') {
            self.pos += 1;
            frac_len = self.digits();
            if frac_len == 0 {
                return Err(Malformed);
            }
            float = true;
        }
        if matches!(self.s.get(self.pos), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.s.get(self.pos), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            exp_len = self.digits();
            if exp_len == 0 {
                return Err(Malformed);
            }
            float = true;
        }
        let text = String::from_utf8_lossy(&self.s[start..self.pos]).into_owned();
        if float {
            if int_len + frac_len + exp_len > MAX_NUM_LEN {
                return Err(Malformed);
            }
            Ok(Node::Float(text))
        } else {
            if int_len > MAX_NUM_LEN {
                return Err(Malformed);
            }
            Ok(Node::Int(text))
        }
    }

    /// The rest of a string whose opening quote has been consumed. Property
    /// names (`name`) are stricter about escaped surrogates than values
    /// (jackson-core #1541): a high one must be followed by an escaped low
    /// one, and a lone low one is refused.
    fn string(&mut self, name: bool) -> Result<Vec<u16>, Malformed> {
        let mut out = Vec::new();
        loop {
            let b = self.next()?;
            match b {
                b'"' => break,
                b'\\' => {
                    let c = self.escape()?;
                    if name && (0xD800..=0xDBFF).contains(&c) {
                        if self.next()? != b'\\' {
                            return Err(Malformed);
                        }
                        let lo = self.escape()?;
                        if !(0xDC00..=0xDFFF).contains(&lo) {
                            return Err(Malformed);
                        }
                        out.push(c);
                        out.push(lo);
                    } else if name && (0xDC00..=0xDFFF).contains(&c) {
                        return Err(Malformed);
                    } else {
                        out.push(c);
                    }
                }
                0x00..=0x1F => return Err(Malformed),
                0x20..=0x7F => out.push(u16::from(b)),
                _ => self.utf8(b, name, &mut out)?,
            }
            if out.len() > MAX_STRING_LEN {
                return Err(Malformed);
            }
        }
        Ok(out)
    }

    /// `UTF8StreamJsonParser._decodeUtf8_{2,3,4}` (and `addName` for names).
    fn utf8(&mut self, lead: u8, name: bool, out: &mut Vec<u16>) -> Result<(), Malformed> {
        let cont = |r: &mut Self| -> Result<u32, Malformed> {
            let b = r.next()?;
            if b & 0xC0 != 0x80 {
                return Err(Malformed);
            }
            Ok(u32::from(b & 0x3F))
        };
        if lead & 0xE0 == 0xC0 {
            let c = (u32::from(lead & 0x1F) << 6) | cont(self)?;
            out.push(c as u16);
        } else if lead & 0xF0 == 0xE0 {
            let c = (u32::from(lead & 0x0F) << 12) | (cont(self)? << 6) | cont(self)?;
            if (0xD800..=0xDFFF).contains(&c) {
                return Err(Malformed);
            }
            out.push(c as u16);
        } else if lead & 0xF8 == 0xF0 {
            let cp = (u32::from(lead & 0x07) << 18)
                | (cont(self)? << 12)
                | (cont(self)? << 6)
                | cont(self)?;
            // Jackson does no range check here: an overlong or >U+10FFFF
            // sequence yields whatever these (Java int) operations give.
            let c = cp as i32 - 0x10000;
            let hi = if name {
                0xD800 + (c >> 10)
            } else {
                0xD800 | (c >> 10)
            };
            out.push(hi as u16);
            out.push((0xDC00 | (c & 0x3FF)) as u16);
        } else {
            return Err(Malformed);
        }
        Ok(())
    }

    /// `_decodeEscaped`: the character after a backslash.
    fn escape(&mut self) -> Result<u16, Malformed> {
        Ok(match self.next()? {
            b'b' => 0x08,
            b't' => 0x09,
            b'n' => 0x0A,
            b'f' => 0x0C,
            b'r' => 0x0D,
            b'"' => 0x22,
            b'/' => 0x2F,
            b'\\' => 0x5C,
            b'u' => {
                let mut value = 0u16;
                for _ in 0..4 {
                    let digit = char::from(self.next()?).to_digit(16).ok_or(Malformed)?;
                    value = (value << 4) | digit as u16;
                }
                value
            }
            _ => return Err(Malformed),
        })
    }
}

/// The UTF-8 length of a name, as Jackson measures it for `maxNameLength`.
fn utf8_len(units: &[u16]) -> usize {
    char::decode_utf16(units.iter().copied())
        .map(|r| r.map_or(3, char::len_utf8))
        .sum()
}

/// `ObjectMapper.writeValueAsBytes(node)` for a tree [`read_tree`] produced.
pub(crate) fn write_node(node: &Node, out: &mut Vec<u8>) {
    match node {
        Node::Obj(members) => {
            out.push(b'{');
            for (i, (key, value)) in members.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_utf16_string(key, out);
                out.push(b':');
                write_node(value, out);
            }
            out.push(b'}');
        }
        Node::Arr(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_node(item, out);
            }
            out.push(b']');
        }
        Node::Str(units) => write_utf16_string(units, out),
        // IntNode / LongNode / BigIntegerNode write the value, so `-0` is `0`.
        Node::Int(text) if text == "-0" => out.push(b'0'),
        Node::Int(text) => out.extend_from_slice(text.as_bytes()),
        Node::Float(text) => {
            let value: f64 = text.parse().unwrap_or(f64::NAN);
            if value.is_infinite() {
                // JsonWriteFeature.WRITE_NAN_AS_STRINGS (on by default).
                write_str(if value > 0.0 { "Infinity" } else { "-Infinity" }, out);
            } else {
                out.extend_from_slice(java_double_to_string(value).as_bytes());
            }
        }
        Node::Bool(true) => out.extend_from_slice(b"true"),
        Node::Bool(false) => out.extend_from_slice(b"false"),
        Node::Null => out.extend_from_slice(b"null"),
    }
}

/// A JSON string with Jackson's escaping, from a Rust string.
pub(crate) fn write_str(s: &str, out: &mut Vec<u8>) {
    out.push(b'"');
    for c in s.chars() {
        write_char(c, out);
    }
    out.push(b'"');
}

/// A JSON string with Jackson's escaping, from UTF-16 code units; a lone
/// surrogate is written as an upper-case `\uXXXX` escape.
fn write_utf16_string(units: &[u16], out: &mut Vec<u8>) {
    out.push(b'"');
    for r in char::decode_utf16(units.iter().copied()) {
        match r {
            Ok(c) => write_char(c, out),
            Err(e) => {
                out.extend_from_slice(format!("\\u{:04X}", e.unpaired_surrogate()).as_bytes())
            }
        }
    }
    out.push(b'"');
}

fn write_char(c: char, out: &mut Vec<u8>) {
    match c {
        '"' => out.extend_from_slice(b"\\\""),
        '\\' => out.extend_from_slice(b"\\\\"),
        '\u{08}' => out.extend_from_slice(b"\\b"),
        '\u{09}' => out.extend_from_slice(b"\\t"),
        '\u{0A}' => out.extend_from_slice(b"\\n"),
        '\u{0C}' => out.extend_from_slice(b"\\f"),
        '\u{0D}' => out.extend_from_slice(b"\\r"),
        '\u{00}'..='\u{1F}' => out.extend_from_slice(format!("\\u{:04X}", c as u32).as_bytes()),
        _ => {
            let mut buf = [0; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
    }
}

/// Java's `Double.toString(double)` (JDK 19+, the shortest decimal that
/// rounds to the value; plain notation for `1e-3 <= |x| < 1e7`, otherwise
/// `d.dddE±n`). `x` must be finite.
pub(crate) fn java_double_to_string(x: f64) -> String {
    if x == 0.0 {
        return if x.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    let abs = x.abs();
    let (mut digits, mut exp) = decimal_digits(&format!("{abs:e}"));
    if digits.len() == 1 {
        // When the shortest decimal has one digit, Java chooses the closest
        // among the one- and two-digit decimals that round to the value
        // (visible only for tiny subnormals: 4.9E-324, not 5e-324).
        if let Some((d2, e2)) = closest_two_digit(abs) {
            digits = d2;
            exp = e2;
        }
    }
    let mut s = String::new();
    if x < 0.0 {
        s.push('-');
    }
    if (-3..7).contains(&exp) {
        if exp >= 0 {
            let int_len = exp as usize + 1;
            if digits.len() > int_len {
                s.push_str(&digits[..int_len]);
                s.push('.');
                s.push_str(&digits[int_len..]);
            } else {
                s.push_str(&digits);
                s.push_str(&"0".repeat(int_len - digits.len()));
                s.push_str(".0");
            }
        } else {
            s.push_str("0.");
            s.push_str(&"0".repeat((-exp - 1) as usize));
            s.push_str(&digits);
        }
    } else {
        s.push_str(&digits[..1]);
        s.push('.');
        s.push_str(if digits.len() > 1 { &digits[1..] } else { "0" });
        s.push('E');
        s.push_str(&exp.to_string());
    }
    s
}

/// `"1.2345e-7"` → (`"12345"`, -7): significant digits without trailing
/// zeros, and the power of ten of the first one.
fn decimal_digits(sci: &str) -> (String, i32) {
    let (mantissa, exp) = sci.split_once('e').unwrap_or((sci, "0"));
    let mut digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    while digits.len() > 1 && digits.ends_with('0') {
        digits.pop();
    }
    (digits, exp.parse().unwrap_or(0))
}

/// The decimal Java's `Double.toString` picks when the shortest one has a
/// single digit: the closest to `abs` among the one- and two-digit decimals
/// that round to it. The two-digit decimals bracketing `abs` are the only
/// candidates (a one-digit `d` is the two-digit `d.0`); the correctly rounded
/// one wins when it rounds back to `abs`, else the other one, which lies
/// between `abs` and the shortest and so always does. Trailing zeros dropped.
fn closest_two_digit(abs: f64) -> Option<(String, i32)> {
    let rounded = format!("{abs:.1e}");
    let (mantissa, exp) = rounded.split_once('e')?;
    let exp: i32 = exp.parse().ok()?;
    let n: i32 = mantissa.replace('.', "").parse().ok()?;
    let candidate = |n: i32, exp: i32| -> Option<(i32, i32)> {
        let (n, exp) = match n {
            9 => (90, exp - 1),
            100 => (10, exp + 1),
            _ => (n, exp),
        };
        let value: f64 = format!("{}.{}e{}", n / 10, n % 10, exp).parse().ok()?;
        (value == abs).then_some((n, exp))
    };
    let chosen = candidate(n, exp).or_else(|| {
        let value: f64 = format!("{}.{}e{}", n / 10, n % 10, exp).parse().ok()?;
        candidate(if value > abs { n - 1 } else { n + 1 }, exp)
    })?;
    let (n, exp) = chosen;
    let digits = if n % 10 == 0 { n / 10 } else { n };
    Some((digits.to_string(), exp))
}

/// A `serde_json` formatter that escapes strings as Jackson does: control
/// characters without a short form as `\u00XX` with **upper-case** hex
/// (serde_json's default is lower-case). Everything else about compact
/// `serde_json` output already matches Jackson's.
pub(crate) struct JacksonFormatter;

impl serde_json::ser::Formatter for JacksonFormatter {
    fn write_char_escape<W>(
        &mut self,
        writer: &mut W,
        char_escape: serde_json::ser::CharEscape,
    ) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        use serde_json::ser::CharEscape::*;
        let bytes: &[u8] = match char_escape {
            Quote => b"\\\"",
            ReverseSolidus => b"\\\\",
            Solidus => b"/",
            Backspace => b"\\b",
            FormFeed => b"\\f",
            LineFeed => b"\\n",
            CarriageReturn => b"\\r",
            Tab => b"\\t",
            AsciiControl(byte) => {
                return writer.write_all(format!("\\u{:04X}", byte).as_bytes());
            }
        };
        writer.write_all(bytes)
    }
}

/// Serialises with [`JacksonFormatter`].
pub(crate) fn to_vec<T: serde::Serialize + ?Sized>(value: &T) -> Vec<u8> {
    let mut out = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(&mut out, JacksonFormatter);
    // Serialising our own types into a Vec cannot fail: every map key is a
    // string and no Serialize impl here returns an error.
    value
        .serialize(&mut ser)
        .expect("in-memory JSON serialisation");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn double_to_string_matches_java() {
        let rows = [
            (100.0, "100.0"),
            (1.5, "1.5"),
            (1e7, "1.0E7"),
            (9_999_999.0, "9999999.0"),
            (0.001, "0.001"),
            (0.0001, "1.0E-4"),
            (123_456_789.125, "1.23456789125E8"),
            (-2.5e-10, "-2.5E-10"),
            (5e-324, "4.9E-324"),
            (1e-322, "9.9E-323"),
            (2e-323, "2.0E-323"),
            (1.797_693_134_862_315_7e308, "1.7976931348623157E308"),
            (-0.0, "-0.0"),
        ];
        for (value, java) in rows {
            assert_eq!(java_double_to_string(value), java, "{value:e}");
        }
    }

    #[test]
    fn missing_root_is_not_an_error() {
        assert!(matches!(read_tree(b""), Ok(None)));
        assert!(matches!(read_tree(b" \n"), Ok(None)));
        assert!(matches!(read_tree(b"\xEF\xBB\xBF"), Ok(None)));
        assert!(read_tree(b"{} x").is_err());
    }
}
