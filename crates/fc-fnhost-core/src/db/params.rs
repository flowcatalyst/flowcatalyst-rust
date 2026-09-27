//! Statement parameters (Java `DbSession.bind`): positional, never
//! interpolated. A boolean, an integer, a float and a decimal are sent
//! with their own type (`bool`, `int8`, `float8`, `numeric`); text and
//! `NULL` are sent untyped, so the server reads them as whatever the
//! statement needs at that position, as pgjdbc's `Types.OTHER` does.
//!
//! PostgreSQL's extended protocol here carries values in binary, so an
//! untyped text value is encoded in the binary form of the type the server
//! inferred for its placeholder: the text types, `json`/`jsonb`, enums and
//! other text-like types as the text itself; `bool`, the integers, the
//! floats, `numeric`, `uuid`, `date`, `time`, `timetz`, `timestamp`,
//! `timestamptz`, `bytea` (`\x…` hex, or the raw text) and `inet`/`cidr`
//! parsed from it. Any other type (`interval`, arrays, ranges, geometric
//! types, …) is `DB_BAD_REQUEST`: read it through text in the SQL
//! (`?::text::interval`).

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use sqlx::encode::IsNull;
use sqlx::error::BoxDynError;
use sqlx::postgres::types::Oid;
use sqlx::postgres::{PgArgumentBuffer, PgTypeInfo, PgTypeKind};
use sqlx::{Encode, Postgres, Type, TypeInfo};

use super::rows::oid;

/// One statement parameter, as the guest gave it.
#[derive(Debug, Clone, PartialEq)]
pub enum Param {
    Null,
    Boolean(bool),
    Integer(i64),
    Float(f64),
    /// An exact decimal's text.
    Decimal(String),
    Text(String),
}

/// The type a statement is prepared with for `param`: Java's explicit
/// types, and unspecified (OID 0) for text and `NULL`.
pub fn declared_type(param: &Param) -> PgTypeInfo {
    PgTypeInfo::with_oid(Oid(match param {
        Param::Null | Param::Text(_) => 0,
        Param::Boolean(_) => oid::BOOL,
        Param::Integer(_) => oid::INT8,
        Param::Float(_) => oid::FLOAT8,
        Param::Decimal(_) => oid::NUMERIC,
    }))
}

/// A parameter's bytes in the binary form of its placeholder's type.
pub struct Bound {
    oid: u32,
    bytes: Option<Vec<u8>>,
}

impl Type<Postgres> for Bound {
    fn type_info() -> PgTypeInfo {
        PgTypeInfo::with_oid(Oid(0))
    }

    fn compatible(_: &PgTypeInfo) -> bool {
        true
    }
}

impl Encode<'_, Postgres> for Bound {
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
        match &self.bytes {
            None => Ok(IsNull::Yes),
            Some(bytes) => {
                buf.extend_from_slice(bytes);
                Ok(IsNull::No)
            }
        }
    }

    fn produces(&self) -> Option<PgTypeInfo> {
        Some(PgTypeInfo::with_oid(Oid(self.oid)))
    }
}

/// `param` (the statement's parameter `index`, for the message) bound to a
/// placeholder of type `ty`. The error never contains the value.
pub fn bind(index: usize, param: &Param, ty: &PgTypeInfo) -> Result<Bound, String> {
    let (target, kind) = resolve(ty);
    let oid = ty.oid().map_or(0, |o| o.0);
    let bytes = match param {
        Param::Null => None,
        Param::Boolean(b) if target == oid::BOOL => Some(vec![u8::from(*b)]),
        Param::Integer(v) if matches!(target, oid::INT2 | oid::INT4 | oid::INT8 | oid::OID) => {
            Some(integer(index, *v, target)?)
        }
        Param::Float(v) if target == oid::FLOAT8 => Some(v.to_be_bytes().to_vec()),
        Param::Float(v) if target == oid::FLOAT4 => Some((*v as f32).to_be_bytes().to_vec()),
        Param::Decimal(text) if target == oid::NUMERIC => Some(
            super::numeric::encode(text)
                .ok_or_else(|| format!("params[{index}] is not a decimal number"))?,
        ),
        Param::Boolean(b) => Some(text_as(
            index,
            if *b { "true" } else { "false" },
            target,
            kind,
            ty,
        )?),
        Param::Integer(v) => Some(text_as(index, &v.to_string(), target, kind, ty)?),
        Param::Float(v) => Some(text_as(index, &float_text(*v), target, kind, ty)?),
        Param::Decimal(text) | Param::Text(text) => Some(text_as(index, text, target, kind, ty)?),
    };
    Ok(Bound { oid, bytes })
}

/// Why a parameter's type is not one text can be bound to here.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A built-in or resolved simple type: go by OID.
    Simple,
    /// An enum or another type whose binary form is its text.
    TextLike,
    /// An array, range or composite.
    Structured,
}

/// The OID to encode for (a domain's base type) and how.
fn resolve(ty: &PgTypeInfo) -> (u32, Kind) {
    let mut ty = ty;
    loop {
        if ty.name() == "?" {
            return (ty.oid().map_or(0, |o| o.0), Kind::Simple);
        }
        match ty.kind() {
            PgTypeKind::Domain(base) => ty = base,
            PgTypeKind::Enum(_) => return (ty.oid().map_or(0, |o| o.0), Kind::TextLike),
            PgTypeKind::Array(_) | PgTypeKind::Range(_) | PgTypeKind::Composite(_) => {
                return (ty.oid().map_or(0, |o| o.0), Kind::Structured)
            }
            PgTypeKind::Simple | PgTypeKind::Pseudo => {
                return (ty.oid().map_or(0, |o| o.0), Kind::Simple)
            }
        }
    }
}

fn float_text(v: f64) -> String {
    if v.is_nan() {
        "NaN".into()
    } else if v.is_infinite() {
        if v > 0.0 { "Infinity" } else { "-Infinity" }.into()
    } else {
        v.to_string()
    }
}

fn integer(index: usize, v: i64, target: u32) -> Result<Vec<u8>, String> {
    let out_of_range = || format!("params[{index}] is out of range for the column's integer type");
    Ok(match target {
        oid::INT2 => i16::try_from(v)
            .map_err(|_| out_of_range())?
            .to_be_bytes()
            .to_vec(),
        oid::INT4 => i32::try_from(v)
            .map_err(|_| out_of_range())?
            .to_be_bytes()
            .to_vec(),
        oid::OID => u32::try_from(v)
            .map_err(|_| out_of_range())?
            .to_be_bytes()
            .to_vec(),
        _ => v.to_be_bytes().to_vec(),
    })
}

/// Text read as the placeholder's type (the type's input function, for the
/// types handled here).
fn text_as(
    index: usize,
    text: &str,
    target: u32,
    kind: Kind,
    ty: &PgTypeInfo,
) -> Result<Vec<u8>, String> {
    let invalid = |what: &str| format!("params[{index}] is not a valid {what}");
    match kind {
        Kind::TextLike => return Ok(text.as_bytes().to_vec()),
        Kind::Structured => {
            return Err(format!(
                "params[{index}]: a value cannot be bound to a {} placeholder; cast it in the SQL (?::text::{})",
                ty.name(),
                ty.name()
            ))
        }
        Kind::Simple => {}
    }
    let trimmed = text.trim();
    Ok(match target {
        // Unspecified to the end (`SELECT ?`: the server calls it text),
        // the text types, and the types whose binary form is their text.
        0 | oid::TEXT | oid::VARCHAR | oid::BPCHAR | oid::NAME | oid::UNKNOWN | oid::JSON
        | oid::XML | oid::CHAR => text.as_bytes().to_vec(),
        oid::JSONB => {
            let mut out = Vec::with_capacity(text.len() + 1);
            out.push(1);
            out.extend_from_slice(text.as_bytes());
            out
        }
        oid::BOOL => vec![u8::from(parse_bool(trimmed).ok_or_else(|| invalid("boolean"))?)],
        oid::INT2 | oid::INT4 | oid::INT8 | oid::OID => {
            let v: i64 = trimmed.parse().map_err(|_| invalid("integer"))?;
            integer(index, v, target)?
        }
        oid::FLOAT4 | oid::FLOAT8 => {
            let v = parse_float(trimmed).ok_or_else(|| invalid("number"))?;
            if target == oid::FLOAT4 {
                (v as f32).to_be_bytes().to_vec()
            } else {
                v.to_be_bytes().to_vec()
            }
        }
        oid::NUMERIC => super::numeric::encode(trimmed).ok_or_else(|| invalid("decimal number"))?,
        oid::UUID => parse_uuid(trimmed).ok_or_else(|| invalid("uuid"))?.to_vec(),
        oid::DATE => date_days(trimmed)
            .ok_or_else(|| invalid("date"))?
            .to_be_bytes()
            .to_vec(),
        oid::TIME => parse_time(trimmed)
            .ok_or_else(|| invalid("time"))?
            .to_be_bytes()
            .to_vec(),
        oid::TIMETZ => {
            let (time, offset) = split_offset(trimmed);
            let micros = parse_time(time).ok_or_else(|| invalid("time with time zone"))?;
            let east = offset
                .map(parse_offset)
                .unwrap_or(Some(0))
                .ok_or_else(|| invalid("time with time zone"))?;
            let mut out = micros.to_be_bytes().to_vec();
            out.extend_from_slice(&(-east).to_be_bytes());
            out
        }
        oid::TIMESTAMP => timestamp_micros(trimmed, false)
            .ok_or_else(|| invalid("timestamp"))?
            .to_be_bytes()
            .to_vec(),
        oid::TIMESTAMPTZ => timestamp_micros(trimmed, true)
            .ok_or_else(|| invalid("timestamp with time zone"))?
            .to_be_bytes()
            .to_vec(),
        oid::BYTEA => match trimmed.strip_prefix("\\x") {
            Some(hex_text) => hex::decode(hex_text).map_err(|_| invalid("bytea hex string"))?,
            None => text.as_bytes().to_vec(),
        },
        oid::INET | oid::CIDR => {
            inet_bytes(trimmed, target == oid::CIDR).ok_or_else(|| invalid("network address"))?
        }
        // A resolved type this host knows nothing of (an extension's, such
        // as citext): its text. The server refuses it if that is not its
        // binary form.
        _ if ty.name() != "?" && !is_builtin(target) => text.as_bytes().to_vec(),
        _ => {
            return Err(format!(
                "params[{index}]: a value cannot be bound to a {} placeholder; cast it in the SQL (?::text::{})",
                ty.name(),
                ty.name()
            ))
        }
    })
}

/// Built-in OIDs are below `FirstGenbkiObjectId` (10000).
fn is_builtin(oid: u32) -> bool {
    oid < 10_000
}

fn parse_bool(text: &str) -> Option<bool> {
    match text.to_ascii_lowercase().as_str() {
        "t" | "true" | "y" | "yes" | "on" | "1" => Some(true),
        "f" | "false" | "n" | "no" | "off" | "0" => Some(false),
        _ => None,
    }
}

fn parse_float(text: &str) -> Option<f64> {
    match text.to_ascii_lowercase().as_str() {
        "nan" => Some(f64::NAN),
        "infinity" | "+infinity" | "inf" | "+inf" => Some(f64::INFINITY),
        "-infinity" | "-inf" => Some(f64::NEG_INFINITY),
        other => other.parse().ok(),
    }
}

/// PostgreSQL's `uuid_in`: 32 hex digits, optionally in braces, hyphens
/// anywhere between groups of four.
fn parse_uuid(text: &str) -> Option<[u8; 16]> {
    let inner = text
        .strip_prefix('{')
        .and_then(|t| t.strip_suffix('}'))
        .unwrap_or(text);
    let hex_digits: String = inner.chars().filter(|c| *c != '-').collect();
    if hex_digits.len() != 32 || inner.starts_with('-') || inner.ends_with('-') {
        return None;
    }
    hex::decode(hex_digits).ok()?.try_into().ok()
}

fn pg_epoch_date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2000, 1, 1).expect("a valid date")
}

fn date_days(text: &str) -> Option<i32> {
    match text.to_ascii_lowercase().as_str() {
        "infinity" | "+infinity" => return Some(i32::MAX),
        "-infinity" => return Some(i32::MIN),
        _ => {}
    }
    let date = NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()?;
    i32::try_from((date - pg_epoch_date()).num_days()).ok()
}

/// `HH:MM[:SS[.fraction]]` as microseconds since midnight.
fn parse_time(text: &str) -> Option<i64> {
    let time = NaiveTime::parse_from_str(text, "%H:%M:%S%.f")
        .or_else(|_| NaiveTime::parse_from_str(text, "%H:%M"))
        .ok()?;
    use chrono::Timelike;
    Some(time.num_seconds_from_midnight() as i64 * 1_000_000 + (time.nanosecond() / 1000) as i64)
}

/// A trailing `Z` or `±hh[:mm[:ss]]` / `±hhmm` split off the text.
fn split_offset(text: &str) -> (&str, Option<&str>) {
    if let Some(rest) = text.strip_suffix(['Z', 'z']) {
        return (rest.trim_end(), Some("Z"));
    }
    // The last sign after the time's colon, if any.
    let colon = text.find(':').unwrap_or(0);
    match text[colon..].rfind(['+', '-']) {
        Some(p) => {
            let p = colon + p;
            (text[..p].trim_end(), Some(&text[p..]))
        }
        None => (text, None),
    }
}

/// Seconds EAST of UTC.
fn parse_offset(text: &str) -> Option<i32> {
    if text.eq_ignore_ascii_case("z") {
        return Some(0);
    }
    let (sign, rest) = match text.as_bytes().first()? {
        b'+' => (1, &text[1..]),
        b'-' => (-1, &text[1..]),
        _ => return None,
    };
    let parts: Vec<&str> = if rest.contains(':') {
        rest.split(':').collect()
    } else if rest.len() == 4 {
        vec![&rest[..2], &rest[2..]]
    } else {
        vec![rest]
    };
    if parts.is_empty() || parts.len() > 3 || parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    let mut seconds = 0i32;
    for (i, part) in parts.iter().enumerate() {
        let v: i32 = part.parse().ok()?;
        seconds += v * [3600, 60, 1][i];
    }
    (seconds <= 16 * 3600).then_some(sign * seconds)
}

/// `YYYY-MM-DD[( |T)HH:MM[:SS[.f]]][offset]` as microseconds since
/// 2000-01-01 (UTC when `with_zone`, the offset applied; a timestamp
/// without a zone ignores one, as PostgreSQL does).
fn timestamp_micros(text: &str, with_zone: bool) -> Option<i64> {
    match text.to_ascii_lowercase().as_str() {
        "infinity" | "+infinity" => return Some(i64::MAX),
        "-infinity" => return Some(i64::MIN),
        _ => {}
    }
    let (date, rest) = match text.find(['T', 't', ' ']) {
        Some(p) => (&text[..p], text[p + 1..].trim()),
        None => (text, ""),
    };
    let date = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    let (time, offset) = if rest.is_empty() {
        ("", None)
    } else {
        split_offset(rest)
    };
    let micros_of_day = if time.is_empty() {
        0
    } else {
        parse_time(time)?
    };
    let local: NaiveDateTime = date.and_hms_opt(0, 0, 0)?;
    let base = (local - pg_epoch_date().and_hms_opt(0, 0, 0)?).num_microseconds()?;
    let mut micros = base.checked_add(micros_of_day)?;
    if with_zone {
        if let Some(offset) = offset {
            micros = micros.checked_sub(parse_offset(offset)? as i64 * 1_000_000)?;
        }
    }
    Some(micros)
}

/// `inet`/`cidr` binary: family (2 = IPv4, 3 = IPv6), bits, is_cidr,
/// address length, address.
fn inet_bytes(text: &str, cidr: bool) -> Option<Vec<u8>> {
    let (addr, bits) = match text.split_once('/') {
        Some((a, b)) => (a, Some(b.parse::<u8>().ok()?)),
        None => (text, None),
    };
    let ip: std::net::IpAddr = addr.parse().ok()?;
    let (family, octets, max): (u8, Vec<u8>, u8) = match ip {
        std::net::IpAddr::V4(v4) => (2, v4.octets().to_vec(), 32),
        std::net::IpAddr::V6(v6) => (3, v6.octets().to_vec(), 128),
    };
    let bits = bits.unwrap_or(max);
    if bits > max {
        return None;
    }
    let mut out = vec![family, bits, u8::from(cidr), octets.len() as u8];
    out.extend_from_slice(&octets);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ty(oid: u32) -> PgTypeInfo {
        PgTypeInfo::with_oid(Oid(oid))
    }

    fn bytes(param: Param, oid: u32) -> Result<Option<Vec<u8>>, String> {
        bind(0, &param, &ty(oid)).map(|b| b.bytes)
    }

    #[test]
    fn javas_types_for_everything_but_text_and_null() {
        assert_eq!(
            declared_type(&Param::Integer(1)).oid(),
            Some(Oid(oid::INT8))
        );
        assert_eq!(
            declared_type(&Param::Boolean(true)).oid(),
            Some(Oid(oid::BOOL))
        );
        assert_eq!(
            declared_type(&Param::Float(1.0)).oid(),
            Some(Oid(oid::FLOAT8))
        );
        assert_eq!(
            declared_type(&Param::Decimal("1".into())).oid(),
            Some(Oid(oid::NUMERIC))
        );
        assert_eq!(declared_type(&Param::Text("x".into())).oid(), Some(Oid(0)));
        assert_eq!(declared_type(&Param::Null).oid(), Some(Oid(0)));
    }

    #[test]
    fn typed_values_bind_directly() {
        assert_eq!(
            bytes(Param::Integer(7), oid::INT8),
            Ok(Some(7i64.to_be_bytes().to_vec()))
        );
        assert_eq!(
            bytes(Param::Integer(7), oid::INT4),
            Ok(Some(7i32.to_be_bytes().to_vec()))
        );
        assert!(bytes(Param::Integer(1 << 40), oid::INT4)
            .unwrap_err()
            .contains("out of range"));
        assert_eq!(bytes(Param::Boolean(true), oid::BOOL), Ok(Some(vec![1])));
        assert_eq!(bytes(Param::Null, oid::UUID), Ok(None));
        assert_eq!(
            bytes(Param::Decimal("12.50".into()), oid::NUMERIC),
            Ok(Some(crate::db::numeric::encode("12.50").unwrap()))
        );
        // A typed value in a text placeholder: its text.
        assert_eq!(
            bytes(Param::Integer(42), oid::TEXT),
            Ok(Some(b"42".to_vec()))
        );
    }

    #[test]
    fn text_reads_as_the_placeholders_type() {
        let text = |t: &str| Param::Text(t.into());
        assert_eq!(
            bytes(text("a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11"), oid::UUID),
            Ok(Some(
                hex::decode("a0eebc999c0b4ef8bb6d6bb9bd380a11").unwrap()
            ))
        );
        assert_eq!(
            bytes(text(r#"{"a":1}"#), oid::JSONB),
            Ok(Some(b"\x01{\"a\":1}".to_vec()))
        );
        assert_eq!(bytes(text("yes"), oid::BOOL), Ok(Some(vec![1])));
        assert_eq!(
            bytes(text(" 12 "), oid::INT4),
            Ok(Some(12i32.to_be_bytes().to_vec()))
        );
        assert_eq!(
            bytes(text("1999-12-31"), oid::DATE),
            Ok(Some((-1i32).to_be_bytes().to_vec()))
        );
        assert_eq!(
            bytes(text("2000-01-01T00:00:01+01:00"), oid::TIMESTAMPTZ),
            Ok(Some((1_000_000i64 - 3_600_000_000).to_be_bytes().to_vec()))
        );
        assert_eq!(
            bytes(text("2000-01-01 00:00:01.5Z"), oid::TIMESTAMPTZ),
            Ok(Some(1_500_000i64.to_be_bytes().to_vec()))
        );
        assert_eq!(
            bytes(text("2000-01-01T00:00:01+01:00"), oid::TIMESTAMP),
            Ok(Some(1_000_000i64.to_be_bytes().to_vec())),
            "a timestamp without a zone ignores one"
        );
        assert_eq!(
            bytes(text("2000-01-02"), oid::TIMESTAMP),
            Ok(Some(86_400_000_000i64.to_be_bytes().to_vec()))
        );
        assert_eq!(bytes(text("\\x00ff"), oid::BYTEA), Ok(Some(vec![0, 255])));
        assert_eq!(
            bytes(text("10.0.0.0/8"), oid::CIDR),
            Ok(Some(vec![2, 8, 1, 4, 10, 0, 0, 0]))
        );
        let mut timetz = 3_600_000_000i64.to_be_bytes().to_vec();
        timetz.extend_from_slice(&(-19_800i32).to_be_bytes());
        assert_eq!(bytes(text("01:00:00+05:30"), oid::TIMETZ), Ok(Some(timetz)));
        assert_eq!(bytes(text("hello"), 0), Ok(Some(b"hello".to_vec())));
    }

    #[test]
    fn a_bad_value_or_an_unsupported_type_is_refused_without_the_value() {
        let secret = "s3cr3t-value";
        for oid in [
            oid::UUID,
            oid::INT4,
            oid::DATE,
            oid::TIMESTAMPTZ,
            oid::BOOL,
            oid::NUMERIC,
        ] {
            let err = bytes(Param::Text(secret.into()), oid).unwrap_err();
            assert!(err.starts_with("params[0]"), "{err}");
            assert!(!err.contains(secret), "{err}");
        }
        let err = bytes(Param::Text("1 day".into()), oid::INTERVAL).unwrap_err();
        assert!(err.contains("?::text::"), "{err}");
    }
}
