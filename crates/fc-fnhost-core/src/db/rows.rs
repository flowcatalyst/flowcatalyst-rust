//! A result set as a query's answer (Java `fnhost/wasm/RowJson.java`),
//! written **row by row** under the caps: at most [`MAX_ROWS`] rows or
//! [`MAX_ROW_BYTES`] bytes of row JSON, whichever comes first; the row that
//! would cross either is not written and the answer says `truncated`.
//!
//! SQL to JSON, by the column's PostgreSQL type (Java's table):
//!
//! | Type | JSON |
//! |---|---|
//! | `int2`, `int4`, `int8`, `oid` | number |
//! | `float4`, `float8` | number; `NaN`/`Infinity`/`-Infinity` as those strings |
//! | `numeric` | string, exactly as PostgreSQL prints it (`"12.50"`) |
//! | `bool` | boolean |
//! | `timestamptz` / `timestamp` | ISO-8601, in UTC with `Z` / without an offset; `"infinity"`/`"-infinity"` |
//! | `date`, `time`, `timetz` | ISO-8601 |
//! | `bytea` | base64 (standard alphabet, padded) |
//! | `json`, `jsonb` | the JSON value itself |
//! | `NULL` of any type | `null` |
//! | anything else | PostgreSQL's text form, as a string |
//!
//! Results arrive in PostgreSQL's binary format (the extended protocol), so
//! "PostgreSQL's text form" is rendered here for: every type above, the
//! text types, `uuid`, `interval`, `inet`, `cidr`, `macaddr[8]`, `bit`,
//! `varbit`, `"char"`, `xid`, enums, domains (as their base type) and
//! one-or-more-dimensional arrays of any of these. Any other type (ranges,
//! composites, geometric types, `money`, `tsvector`, …) comes back as its
//! binary value read as UTF-8 text, or `\x`-hex when it is not text: cast
//! it in the SQL (`col::text`) for PostgreSQL's own text form.

use base64::Engine as _;
use chrono::{Duration as ChronoDuration, NaiveDate, NaiveDateTime, NaiveTime};
use serde_json::{Map, Value};
use sqlx::postgres::{PgRow, PgTypeInfo, PgTypeKind};
use sqlx::{Column, Row, TypeInfo};

/// The row cap (Java: 10 000).
pub const MAX_ROWS: usize = 10_000;
/// The row-JSON byte cap (Java: 8 MiB): the rows array's content, commas
/// included.
pub const MAX_ROW_BYTES: usize = 8 * 1024 * 1024;

pub mod oid {
    pub const BOOL: u32 = 16;
    pub const BYTEA: u32 = 17;
    pub const CHAR: u32 = 18;
    pub const NAME: u32 = 19;
    pub const INT8: u32 = 20;
    pub const INT2: u32 = 21;
    pub const INT4: u32 = 23;
    pub const TEXT: u32 = 25;
    pub const OID: u32 = 26;
    pub const XID: u32 = 28;
    pub const CID: u32 = 29;
    pub const JSON: u32 = 114;
    pub const XML: u32 = 142;
    pub const CIDR: u32 = 650;
    pub const FLOAT4: u32 = 700;
    pub const FLOAT8: u32 = 701;
    pub const UNKNOWN: u32 = 705;
    pub const MACADDR8: u32 = 774;
    pub const MACADDR: u32 = 829;
    pub const INET: u32 = 869;
    pub const BPCHAR: u32 = 1042;
    pub const VARCHAR: u32 = 1043;
    pub const DATE: u32 = 1082;
    pub const TIME: u32 = 1083;
    pub const TIMESTAMP: u32 = 1114;
    pub const TIMESTAMPTZ: u32 = 1184;
    pub const INTERVAL: u32 = 1186;
    pub const TIMETZ: u32 = 1266;
    pub const BIT: u32 = 1560;
    pub const VARBIT: u32 = 1562;
    pub const NUMERIC: u32 = 1700;
    pub const UUID: u32 = 2950;
    pub const JSONB: u32 = 3802;
    pub const XID8: u32 = 5069;
}

/// A query's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowsAnswer {
    /// The rows array, as JSON text.
    pub json: String,
    pub count: u32,
    pub truncated: bool,
}

impl RowsAnswer {
    pub fn empty() -> Self {
        Self {
            json: "[]".into(),
            count: 0,
            truncated: false,
        }
    }
}

/// Collects rows under the caps.
pub struct RowWriter {
    out: Vec<u8>,
    count: usize,
    bytes: usize,
    truncated: bool,
    max_rows: usize,
    max_bytes: usize,
}

impl Default for RowWriter {
    fn default() -> Self {
        Self::with_caps(MAX_ROWS, MAX_ROW_BYTES)
    }
}

impl RowWriter {
    pub fn with_caps(max_rows: usize, max_bytes: usize) -> Self {
        Self {
            out: b"[".to_vec(),
            count: 0,
            bytes: 0,
            truncated: false,
            max_rows,
            max_bytes,
        }
    }

    /// Adds `row`; `false` once the answer is full (the row was not added
    /// and the answer is truncated): stop reading.
    pub fn push(&mut self, row: &PgRow) -> bool {
        self.push_object(row_json(row))
    }

    fn push_object(&mut self, object: Map<String, Value>) -> bool {
        if self.count == self.max_rows {
            self.truncated = true;
            return false;
        }
        let bytes = serde_json::to_vec(&Value::Object(object)).expect("a JSON value serializes");
        let next = self.bytes + bytes.len() + usize::from(self.count > 0);
        if next > self.max_bytes {
            self.truncated = true;
            return false;
        }
        if self.count > 0 {
            self.out.push(b',');
        }
        self.out.extend_from_slice(&bytes);
        self.bytes = next;
        self.count += 1;
        true
    }

    pub fn finish(mut self) -> RowsAnswer {
        self.out.push(b']');
        RowsAnswer {
            json: String::from_utf8(self.out).expect("serde_json writes UTF-8"),
            count: self.count as u32,
            truncated: self.truncated,
        }
    }
}

/// One row as a JSON object keyed by column label; a later column with the
/// same label replaces the value in the earlier one's place (Jackson's
/// `ObjectNode.put`).
pub fn row_json(row: &PgRow) -> Map<String, Value> {
    let mut object = Map::new();
    for (i, column) in row.columns().iter().enumerate() {
        let value = match row.try_get_raw(i) {
            Ok(raw) => {
                let raw: sqlx::postgres::PgValueRef<'_> = raw;
                if sqlx::ValueRef::is_null(&raw) {
                    Value::Null
                } else {
                    let ty = sqlx::ValueRef::type_info(&raw).into_owned();
                    match raw.as_bytes() {
                        Ok(bytes) => to_json(&ty, bytes),
                        Err(_) => Value::Null,
                    }
                }
            }
            Err(_) => Value::Null,
        };
        object.insert(column.name().to_owned(), value);
    }
    object
}

/// A type's kind, when sqlx resolved it (every type a row description
/// carries is; a bare `PgTypeInfo::with_oid` is not, and has none).
fn kind_of(ty: &PgTypeInfo) -> Option<&PgTypeKind> {
    (ty.name() != "?").then(|| ty.kind())
}

/// The OID a value is rendered as: a domain's base type, else its own.
fn effective(ty: &PgTypeInfo) -> (u32, &PgTypeInfo) {
    let mut ty = ty;
    while let Some(PgTypeKind::Domain(base)) = kind_of(ty) {
        ty = base;
    }
    (ty.oid().map_or(0, |o| o.0), ty)
}

/// Java's mapping of one non-null value.
pub fn to_json(ty: &PgTypeInfo, bytes: &[u8]) -> Value {
    let (oid, ty) = effective(ty);
    let text = |s: Option<String>| Value::String(s.unwrap_or_else(|| fallback_text(bytes)));
    match oid {
        oid::INT2 => be_i16(bytes).map_or_else(|| text(None), |v| Value::from(v as i64)),
        oid::INT4 => be_i32(bytes).map_or_else(|| text(None), |v| Value::from(v as i64)),
        oid::INT8 => be_i64(bytes).map_or_else(|| text(None), Value::from),
        oid::OID => be_u32(bytes).map_or_else(|| text(None), |v| Value::from(v as i64)),
        oid::FLOAT4 => be_f32(bytes).map_or_else(|| text(None), |v| float_json(f32_as_f64(v))),
        oid::FLOAT8 => be_f64(bytes).map_or_else(|| text(None), float_json),
        oid::BOOL => bytes
            .first()
            .map_or_else(|| text(None), |b| Value::Bool(*b != 0)),
        oid::TIMESTAMPTZ => text(timestamp_iso(bytes, true)),
        oid::TIMESTAMP => text(timestamp_iso(bytes, false)),
        oid::DATE => text(date_iso(bytes)),
        oid::TIME => text(be_i64(bytes).map(time_iso)),
        oid::TIMETZ => text(timetz_iso(bytes)),
        oid::BYTEA => Value::String(base64::engine::general_purpose::STANDARD.encode(bytes)),
        oid::JSON | oid::JSONB => {
            let body = if oid == oid::JSONB && bytes.first() == Some(&1) {
                &bytes[1..]
            } else {
                bytes
            };
            match std::str::from_utf8(body) {
                Ok(s) => serde_json::from_str(s).unwrap_or_else(|_| Value::String(s.to_owned())),
                Err(_) => text(None),
            }
        }
        _ => text(pg_text(ty, bytes)),
    }
}

/// A float as JSON: the number, or Java's strings for what JSON cannot say.
fn float_json(v: f64) -> Value {
    if v.is_finite() {
        serde_json::Number::from_f64(v).map_or(Value::Null, Value::Number)
    } else if v.is_nan() {
        Value::String("NaN".into())
    } else if v > 0.0 {
        Value::String("Infinity".into())
    } else {
        Value::String("-Infinity".into())
    }
}

/// `float4` widened as its shortest decimal text reads (`0.1`, not
/// `0.10000000149011612`), which is what PostgreSQL prints.
fn f32_as_f64(v: f32) -> f64 {
    if v.is_finite() {
        v.to_string().parse().unwrap_or(v as f64)
    } else {
        v as f64
    }
}

/// Not a type this module renders: its bytes as UTF-8 text, or `\x`-hex.
fn fallback_text(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) if !s.chars().any(|c| c.is_control() && c != '\n' && c != '\t') => s.to_owned(),
        _ => format!("\\x{}", hex::encode(bytes)),
    }
}

/// PostgreSQL's text form of a binary value (its `typoutput`, with this
/// connection's settings: `DateStyle=ISO`, `TimeZone=UTC`,
/// `IntervalStyle=postgres`); `None` for a type not rendered here.
pub fn pg_text(ty: &PgTypeInfo, bytes: &[u8]) -> Option<String> {
    let (oid, ty) = effective(ty);
    match kind_of(ty) {
        Some(PgTypeKind::Enum(_)) => return std::str::from_utf8(bytes).ok().map(str::to_owned),
        Some(PgTypeKind::Array(element)) => return array_text(element, bytes),
        _ => {}
    }
    match oid {
        oid::BOOL => bytes
            .first()
            .map(|b| if *b != 0 { "t" } else { "f" }.to_owned()),
        oid::INT2 => be_i16(bytes).map(|v| v.to_string()),
        oid::INT4 => be_i32(bytes).map(|v| v.to_string()),
        oid::INT8 => be_i64(bytes).map(|v| v.to_string()),
        oid::OID | oid::XID | oid::CID => be_u32(bytes).map(|v| v.to_string()),
        oid::XID8 => be_i64(bytes).map(|v| (v as u64).to_string()),
        oid::FLOAT4 => be_f32(bytes).map(float_text),
        oid::FLOAT8 => be_f64(bytes).map(float_text),
        oid::NUMERIC => super::numeric::decode(bytes),
        oid::TEXT
        | oid::VARCHAR
        | oid::BPCHAR
        | oid::NAME
        | oid::XML
        | oid::UNKNOWN
        | oid::JSON => std::str::from_utf8(bytes).ok().map(str::to_owned),
        oid::JSONB => bytes
            .split_first()
            .filter(|(version, _)| **version == 1)
            .and_then(|(_, body)| std::str::from_utf8(body).ok())
            .map(str::to_owned),
        oid::CHAR => Some(match bytes.first() {
            None | Some(0) => String::new(),
            Some(b) if b.is_ascii() && !b.is_ascii_control() => (*b as char).to_string(),
            Some(b) => format!("\\{b:03o}"),
        }),
        oid::UUID => uuid_text(bytes),
        oid::BYTEA => Some(format!("\\x{}", hex::encode(bytes))),
        oid::DATE => date_pg(bytes),
        oid::TIME => be_i64(bytes).map(time_pg),
        oid::TIMETZ => timetz_pg(bytes),
        oid::TIMESTAMP => timestamp_pg(bytes, false),
        oid::TIMESTAMPTZ => timestamp_pg(bytes, true),
        oid::INTERVAL => interval_text(bytes),
        oid::INET | oid::CIDR => inet_text(bytes),
        oid::MACADDR | oid::MACADDR8 => (bytes.len() == 6 || bytes.len() == 8).then(|| {
            bytes
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<Vec<_>>()
                .join(":")
        }),
        oid::BIT | oid::VARBIT => bit_text(bytes),
        _ => None,
    }
}

fn be_i16(b: &[u8]) -> Option<i16> {
    Some(i16::from_be_bytes(b.try_into().ok()?))
}
fn be_i32(b: &[u8]) -> Option<i32> {
    Some(i32::from_be_bytes(b.try_into().ok()?))
}
fn be_u32(b: &[u8]) -> Option<u32> {
    Some(u32::from_be_bytes(b.try_into().ok()?))
}
fn be_i64(b: &[u8]) -> Option<i64> {
    Some(i64::from_be_bytes(b.try_into().ok()?))
}
fn be_f32(b: &[u8]) -> Option<f32> {
    Some(f32::from_be_bytes(b.try_into().ok()?))
}
fn be_f64(b: &[u8]) -> Option<f64> {
    Some(f64::from_be_bytes(b.try_into().ok()?))
}

/// PostgreSQL's float output: shortest exact, `NaN`, `Infinity`.
fn float_text<F: Into<f64> + std::fmt::Display + Copy>(v: F) -> String {
    let wide: f64 = v.into();
    if wide.is_nan() {
        "NaN".into()
    } else if wide.is_infinite() {
        if wide > 0.0 { "Infinity" } else { "-Infinity" }.into()
    } else {
        v.to_string()
    }
}

fn uuid_text(b: &[u8]) -> Option<String> {
    if b.len() != 16 {
        return None;
    }
    let h = hex::encode(b);
    Some(format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    ))
}

// ── dates and times (PostgreSQL's epoch is 2000-01-01) ───────────────────

fn pg_epoch() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2000, 1, 1)
        .expect("a valid date")
        .and_hms_opt(0, 0, 0)
        .expect("a valid time")
}

fn datetime_of(micros: i64) -> Option<NaiveDateTime> {
    pg_epoch().checked_add_signed(ChronoDuration::microseconds(micros))
}

fn date_of(days: i32) -> Option<NaiveDate> {
    pg_epoch()
        .date()
        .checked_add_signed(ChronoDuration::days(days as i64))
}

/// Java's `ISO_LOCAL_DATE`: four-digit years, a sign past 9999 or before 1.
fn iso_date(d: NaiveDate) -> String {
    use chrono::Datelike;
    let year = d.year();
    let y = if (0..=9999).contains(&year) {
        format!("{year:04}")
    } else if year > 9999 {
        format!("+{year}")
    } else {
        format!("-{:04}", -year)
    };
    format!("{y}-{:02}-{:02}", d.month(), d.day())
}

/// Java's `ISO_LOCAL_TIME`: seconds always, the fraction with as many
/// digits as it needs.
fn iso_time(t: NaiveTime) -> String {
    use chrono::Timelike;
    let mut out = format!("{:02}:{:02}:{:02}", t.hour(), t.minute(), t.second());
    push_fraction(&mut out, t.nanosecond() / 1000);
    out
}

fn push_fraction(out: &mut String, micros: u32) {
    if micros > 0 {
        let digits = format!("{micros:06}");
        out.push('.');
        out.push_str(digits.trim_end_matches('0'));
    }
}

fn timestamp_iso(bytes: &[u8], utc: bool) -> Option<String> {
    let micros = be_i64(bytes)?;
    match micros {
        i64::MAX => return Some("infinity".into()),
        i64::MIN => return Some("-infinity".into()),
        _ => {}
    }
    let dt = datetime_of(micros)?;
    Some(format!(
        "{}T{}{}",
        iso_date(dt.date()),
        iso_time(dt.time()),
        if utc { "Z" } else { "" }
    ))
}

fn date_iso(bytes: &[u8]) -> Option<String> {
    let days = be_i32(bytes)?;
    match days {
        i32::MAX => Some("infinity".into()),
        i32::MIN => Some("-infinity".into()),
        _ => date_of(days).map(iso_date),
    }
}

fn time_of(micros: i64) -> NaiveTime {
    // 24:00:00 is a legal PostgreSQL time; NaiveTime tops out just below.
    let micros = micros.clamp(0, 86_400_000_000 - 1);
    NaiveTime::from_num_seconds_from_midnight_opt(
        (micros / 1_000_000) as u32,
        ((micros % 1_000_000) * 1000) as u32,
    )
    .expect("a time within one day")
}

fn time_iso(micros: i64) -> String {
    if micros == 86_400_000_000 {
        return "24:00:00".into();
    }
    iso_time(time_of(micros))
}

/// An offset east of UTC (Java `ZoneOffset`): `Z`, `+05:30`, `-03:00:15`.
fn iso_offset(east_seconds: i32) -> String {
    if east_seconds == 0 {
        return "Z".into();
    }
    let sign = if east_seconds < 0 { '-' } else { '+' };
    let s = east_seconds.unsigned_abs();
    let mut out = format!("{sign}{:02}:{:02}", s / 3600, (s / 60) % 60);
    if !s.is_multiple_of(60) {
        out.push_str(&format!(":{:02}", s % 60));
    }
    out
}

fn timetz_iso(bytes: &[u8]) -> Option<String> {
    if bytes.len() != 12 {
        return None;
    }
    let micros = be_i64(&bytes[..8])?;
    // Stored as seconds WEST of UTC.
    let west = be_i32(&bytes[8..])?;
    Some(format!("{}{}", time_iso(micros), iso_offset(-west)))
}

/// PostgreSQL's `DateStyle=ISO` date: BC years as `YYYY-MM-DD BC`.
fn date_pg_of(d: NaiveDate) -> String {
    use chrono::Datelike;
    let year = d.year();
    if year <= 0 {
        format!("{:04}-{:02}-{:02} BC", 1 - year, d.month(), d.day())
    } else {
        format!("{year:04}-{:02}-{:02}", d.month(), d.day())
    }
}

fn date_pg(bytes: &[u8]) -> Option<String> {
    let days = be_i32(bytes)?;
    match days {
        i32::MAX => Some("infinity".into()),
        i32::MIN => Some("-infinity".into()),
        _ => date_of(days).map(date_pg_of),
    }
}

fn time_pg(micros: i64) -> String {
    time_iso(micros)
}

/// `+05:30`, `-03`, `+00` as PostgreSQL prints a zone.
fn pg_offset(east_seconds: i32) -> String {
    let sign = if east_seconds < 0 { '-' } else { '+' };
    let s = east_seconds.unsigned_abs();
    let mut out = format!("{sign}{:02}", s / 3600);
    if !s.is_multiple_of(3600) {
        out.push_str(&format!(":{:02}", (s / 60) % 60));
        if !s.is_multiple_of(60) {
            out.push_str(&format!(":{:02}", s % 60));
        }
    }
    out
}

fn timetz_pg(bytes: &[u8]) -> Option<String> {
    if bytes.len() != 12 {
        return None;
    }
    let micros = be_i64(&bytes[..8])?;
    let west = be_i32(&bytes[8..])?;
    Some(format!("{}{}", time_pg(micros), pg_offset(-west)))
}

fn timestamp_pg(bytes: &[u8], utc: bool) -> Option<String> {
    let micros = be_i64(bytes)?;
    match micros {
        i64::MAX => return Some("infinity".into()),
        i64::MIN => return Some("-infinity".into()),
        _ => {}
    }
    let dt = datetime_of(micros)?;
    use chrono::Datelike;
    let bc = dt.date().year() <= 0;
    let date = date_pg_of(dt.date());
    let (date, suffix) = match date.strip_suffix(" BC") {
        Some(d) if bc => (d.to_owned(), " BC"),
        _ => (date, ""),
    };
    Some(format!(
        "{date} {}{}{suffix}",
        iso_time(dt.time()),
        if utc { "+00" } else { "" }
    ))
}

/// `IntervalStyle=postgres` (`EncodeInterval`'s `INTSTYLE_POSTGRES`).
fn interval_text(bytes: &[u8]) -> Option<String> {
    if bytes.len() != 16 {
        return None;
    }
    let time = be_i64(&bytes[..8])?;
    let days = be_i32(&bytes[8..12])? as i64;
    let months = be_i32(&bytes[12..])? as i64;
    let (year, mon) = (months / 12, months % 12);
    let mut out = String::new();
    let mut is_zero = true;
    let mut is_before = false;
    let mut part = |out: &mut String, value: i64, unit: &str| {
        if value == 0 {
            return;
        }
        out.push_str(&format!(
            "{}{}{value} {unit}{}",
            if is_zero { "" } else { " " },
            if is_before && value > 0 { "+" } else { "" },
            if value != 1 { "s" } else { "" }
        ));
        is_before = value < 0;
        is_zero = false;
    };
    part(&mut out, year, "year");
    part(&mut out, mon, "mon");
    part(&mut out, days, "day");
    let hour = time / 3_600_000_000;
    let min = (time / 60_000_000) % 60;
    let sec = (time / 1_000_000) % 60;
    let fsec = time % 1_000_000;
    if is_zero || time != 0 {
        let minus = hour < 0 || min < 0 || sec < 0 || fsec < 0;
        out.push_str(&format!(
            "{}{}{:02}:{:02}:{:02}",
            if is_zero { "" } else { " " },
            if minus {
                "-"
            } else if is_before {
                "+"
            } else {
                ""
            },
            hour.unsigned_abs(),
            min.unsigned_abs(),
            sec.unsigned_abs()
        ));
        push_fraction(&mut out, fsec.unsigned_abs() as u32);
    }
    Some(out)
}

fn inet_text(bytes: &[u8]) -> Option<String> {
    let (&family, rest) = bytes.split_first()?;
    let (&bits, rest) = rest.split_first()?;
    let (&is_cidr, rest) = rest.split_first()?;
    let (&nb, addr) = rest.split_first()?;
    if addr.len() != nb as usize {
        return None;
    }
    let (text, max) = match (family, addr.len()) {
        (2, 4) => (
            std::net::Ipv4Addr::new(addr[0], addr[1], addr[2], addr[3]).to_string(),
            32,
        ),
        (3, 16) => {
            let octets: [u8; 16] = addr.try_into().ok()?;
            (std::net::Ipv6Addr::from(octets).to_string(), 128)
        }
        _ => return None,
    };
    if is_cidr == 0 && bits == max {
        Some(text)
    } else {
        Some(format!("{text}/{bits}"))
    }
}

fn bit_text(bytes: &[u8]) -> Option<String> {
    let len = be_i32(bytes.get(..4)?)? as usize;
    let data = &bytes[4..];
    if data.len() * 8 < len {
        return None;
    }
    Some(
        (0..len)
            .map(|i| {
                if data[i / 8] & (0x80 >> (i % 8)) != 0 {
                    '1'
                } else {
                    '0'
                }
            })
            .collect(),
    )
}

/// An array's text form (`array_out`): `{a,b}`, nested per dimension, a
/// `[lo:hi]=` prefix when a lower bound is not 1, elements quoted when
/// they must be, `NULL` for a null element.
fn array_text(element: &PgTypeInfo, bytes: &[u8]) -> Option<String> {
    let ndim = be_i32(bytes.get(0..4)?)?;
    let _has_nulls = be_i32(bytes.get(4..8)?)?;
    let _element_oid = be_u32(bytes.get(8..12)?)?;
    if ndim == 0 {
        return Some("{}".into());
    }
    let ndim = usize::try_from(ndim).ok().filter(|n| *n <= 6)?;
    let mut dims = Vec::with_capacity(ndim);
    let mut at = 12;
    for _ in 0..ndim {
        let len = be_i32(bytes.get(at..at + 4)?)?;
        let lower = be_i32(bytes.get(at + 4..at + 8)?)?;
        dims.push((usize::try_from(len).ok()?, lower));
        at += 8;
    }
    let total: usize = dims.iter().map(|d| d.0).product();
    let mut elements = Vec::with_capacity(total);
    for _ in 0..total {
        let len = be_i32(bytes.get(at..at + 4)?)?;
        at += 4;
        if len < 0 {
            elements.push(None);
        } else {
            let len = len as usize;
            let value = bytes.get(at..at + len)?;
            at += len;
            let text = pg_text(element, value).unwrap_or_else(|| fallback_text(value));
            elements.push(Some(text));
        }
    }
    let mut out = String::new();
    if dims.iter().any(|d| d.1 != 1) {
        for (len, lower) in &dims {
            out.push_str(&format!("[{lower}:{}]", *lower as i64 + *len as i64 - 1));
        }
        out.push('=');
    }
    let delimiter = if element.oid().map(|o| o.0) == Some(603) {
        ';' // box[] uses ';'
    } else {
        ','
    };
    let mut next = 0;
    write_dimension(&mut out, &dims, 0, &elements, &mut next, delimiter);
    Some(out)
}

fn write_dimension(
    out: &mut String,
    dims: &[(usize, i32)],
    level: usize,
    elements: &[Option<String>],
    next: &mut usize,
    delimiter: char,
) {
    out.push('{');
    for i in 0..dims[level].0 {
        if i > 0 {
            out.push(delimiter);
        }
        if level + 1 < dims.len() {
            write_dimension(out, dims, level + 1, elements, next, delimiter);
        } else {
            match &elements[*next] {
                None => out.push_str("NULL"),
                Some(text) => push_array_element(out, text, delimiter),
            }
            *next += 1;
        }
    }
    out.push('}');
}

fn push_array_element(out: &mut String, text: &str, delimiter: char) {
    let quote = text.is_empty()
        || text.eq_ignore_ascii_case("NULL")
        || text.chars().any(|c| {
            c == '"' || c == '\\' || c == '{' || c == '}' || c == delimiter || c.is_whitespace()
        });
    if !quote {
        out.push_str(text);
        return;
    }
    out.push('"');
    for c in text.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::types::Oid;

    fn ty(oid: u32) -> PgTypeInfo {
        PgTypeInfo::with_oid(Oid(oid))
    }

    /// A type as sqlx resolves it (with its kind).
    fn typed<T: sqlx::Type<sqlx::Postgres>>() -> PgTypeInfo {
        T::type_info()
    }

    fn json(oid: u32, bytes: &[u8]) -> Value {
        to_json(&ty(oid), bytes)
    }

    #[test]
    fn numbers_booleans_and_bytes_follow_javas_table() {
        assert_eq!(json(oid::INT2, &(-3i16).to_be_bytes()), Value::from(-3));
        assert_eq!(json(oid::INT4, &7i32.to_be_bytes()), Value::from(7));
        assert_eq!(
            json(oid::INT8, &i64::MAX.to_be_bytes()),
            Value::from(i64::MAX)
        );
        assert_eq!(
            json(oid::OID, &u32::MAX.to_be_bytes()),
            Value::from(u32::MAX as i64)
        );
        assert_eq!(
            json(oid::FLOAT4, &0.1f32.to_be_bytes()),
            serde_json::json!(0.1)
        );
        assert_eq!(
            json(oid::FLOAT8, &2.5f64.to_be_bytes()),
            serde_json::json!(2.5)
        );
        assert_eq!(
            json(oid::FLOAT8, &f64::NAN.to_be_bytes()),
            Value::from("NaN")
        );
        assert_eq!(
            json(oid::FLOAT8, &f64::NEG_INFINITY.to_be_bytes()),
            Value::from("-Infinity")
        );
        assert_eq!(json(oid::BOOL, &[1]), Value::Bool(true));
        assert_eq!(json(oid::BYTEA, b"\x00\xffhi"), Value::from("AP9oaQ=="));
        let numeric = crate::db::numeric::encode("12.50").unwrap();
        assert_eq!(json(oid::NUMERIC, &numeric), Value::from("12.50"));
    }

    #[test]
    fn json_columns_are_the_value_itself() {
        assert_eq!(
            json(oid::JSON, br#"{"a":[1]}"#),
            serde_json::json!({"a": [1]})
        );
        assert_eq!(
            json(oid::JSONB, b"\x01{\"a\": 2}"),
            serde_json::json!({"a": 2})
        );
    }

    #[test]
    fn times_are_javas_iso_forms() {
        // 2024-01-02T03:04:05.120Z: microseconds since 2000-01-01.
        let at = NaiveDate::from_ymd_opt(2024, 1, 2)
            .unwrap()
            .and_hms_micro_opt(3, 4, 5, 120_000)
            .unwrap();
        let micros = (at - pg_epoch()).num_microseconds().unwrap();
        assert_eq!(
            json(oid::TIMESTAMPTZ, &micros.to_be_bytes()),
            Value::from("2024-01-02T03:04:05.12Z")
        );
        assert_eq!(
            json(oid::TIMESTAMP, &micros.to_be_bytes()),
            Value::from("2024-01-02T03:04:05.12")
        );
        let whole = micros - 120_000 - 5_000_000;
        assert_eq!(
            json(oid::TIMESTAMP, &whole.to_be_bytes()),
            Value::from("2024-01-02T03:04:00")
        );
        assert_eq!(
            json(oid::TIMESTAMPTZ, &i64::MAX.to_be_bytes()),
            Value::from("infinity")
        );
        assert_eq!(
            json(oid::DATE, &(-1i32).to_be_bytes()),
            Value::from("1999-12-31")
        );
        assert_eq!(
            json(oid::DATE, &i32::MIN.to_be_bytes()),
            Value::from("-infinity")
        );
        assert_eq!(
            json(oid::TIME, &(3_723_000_001i64).to_be_bytes()),
            Value::from("01:02:03.000001")
        );
        let mut timetz = 3_600_000_000i64.to_be_bytes().to_vec();
        timetz.extend_from_slice(&(-19_800i32).to_be_bytes()); // +05:30
        assert_eq!(json(oid::TIMETZ, &timetz), Value::from("01:00:00+05:30"));
        let mut utc = 0i64.to_be_bytes().to_vec();
        utc.extend_from_slice(&0i32.to_be_bytes());
        assert_eq!(json(oid::TIMETZ, &utc), Value::from("00:00:00Z"));
    }

    #[test]
    fn everything_else_is_postgres_text() {
        assert_eq!(
            json(
                oid::UUID,
                &hex::decode("a0eebc999c0b4ef8bb6d6bb9bd380a11").unwrap()
            ),
            Value::from("a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11")
        );
        assert_eq!(json(oid::TEXT, "héllo".as_bytes()), Value::from("héllo"));
        let interval = |time: i64, days: i32, months: i32| {
            let mut b = time.to_be_bytes().to_vec();
            b.extend_from_slice(&days.to_be_bytes());
            b.extend_from_slice(&months.to_be_bytes());
            json(oid::INTERVAL, &b)
        };
        assert_eq!(interval(0, 0, 0), Value::from("00:00:00"));
        assert_eq!(interval(0, 1, 0), Value::from("1 day"));
        assert_eq!(interval(0, -1, 0), Value::from("-1 days"));
        assert_eq!(interval(0, 0, 14), Value::from("1 year 2 mons"));
        assert_eq!(interval(0, 0, -10), Value::from("-10 mons"));
        assert_eq!(interval(0, 3, -12), Value::from("-1 years +3 days"));
        assert_eq!(
            interval(14_706_789_000, 3, 14),
            Value::from("1 year 2 mons 3 days 04:05:06.789")
        );
        assert_eq!(
            interval(-3_600_000_000, 1, 0),
            Value::from("1 day -01:00:00")
        );
        assert_eq!(
            json(oid::INET, &[2, 32, 0, 4, 10, 0, 0, 1]),
            Value::from("10.0.0.1")
        );
        assert_eq!(
            json(oid::CIDR, &[2, 8, 1, 4, 10, 0, 0, 0]),
            Value::from("10.0.0.0/8")
        );
        assert_eq!(
            json(oid::MACADDR, &[8, 0, 0x2b, 1, 2, 3]),
            Value::from("08:00:2b:01:02:03")
        );
        assert_eq!(
            json(oid::VARBIT, &[0, 0, 0, 5, 0b1010_1000]),
            Value::from("10101")
        );
        assert_eq!(json(oid::CHAR, b"x"), Value::from("x"));
        assert_eq!(json(99_999, b"\x00\x01"), Value::from("\\x0001"));
        assert_eq!(json(99_999, b"custom"), Value::from("custom"));
    }

    #[test]
    fn arrays_are_postgres_array_text() {
        // int4[] {1,NULL,3}
        let mut b = Vec::new();
        for w in [1i32, 1, oid::INT4 as i32, 3, 1] {
            b.extend_from_slice(&w.to_be_bytes());
        }
        for v in [Some(1i32), None, Some(3)] {
            match v {
                Some(v) => {
                    b.extend_from_slice(&4i32.to_be_bytes());
                    b.extend_from_slice(&v.to_be_bytes());
                }
                None => b.extend_from_slice(&(-1i32).to_be_bytes()),
            }
        }
        assert_eq!(to_json(&typed::<Vec<i32>>(), &b), Value::from("{1,NULL,3}"));
        // text[] {"a b","",plain,"q\"x"}
        let mut b = Vec::new();
        for w in [1i32, 0, oid::TEXT as i32, 4, 1] {
            b.extend_from_slice(&w.to_be_bytes());
        }
        for s in ["a b", "", "plain", "q\"x"] {
            b.extend_from_slice(&(s.len() as i32).to_be_bytes());
            b.extend_from_slice(s.as_bytes());
        }
        assert_eq!(
            to_json(&typed::<Vec<String>>(), &b),
            Value::from(r#"{"a b","",plain,"q\"x"}"#)
        );
        // An empty array.
        let empty = [0i32, 0, oid::INT4 as i32]
            .iter()
            .flat_map(|w| w.to_be_bytes())
            .collect::<Vec<_>>();
        assert_eq!(to_json(&typed::<Vec<i32>>(), &empty), Value::from("{}"));
    }

    #[test]
    fn the_writer_truncates_at_either_cap() {
        let row = |n: i64| {
            let mut m = Map::new();
            m.insert("n".into(), Value::from(n));
            m
        };
        let mut rows = RowWriter::with_caps(2, 1000);
        assert!(rows.push_object(row(1)));
        assert!(rows.push_object(row(2)));
        assert!(!rows.push_object(row(3)));
        let answer = rows.finish();
        assert_eq!(answer.json, r#"[{"n":1},{"n":2}]"#);
        assert_eq!((answer.count, answer.truncated), (2, true));

        // `{"n":1}` is 7 bytes; two rows and a comma are 15.
        let mut rows = RowWriter::with_caps(10, 15);
        assert!(rows.push_object(row(1)));
        assert!(rows.push_object(row(2)));
        assert!(!rows.push_object(row(3)));
        let answer = rows.finish();
        assert_eq!((answer.count, answer.truncated), (2, true));
        let mut rows = RowWriter::with_caps(10, 14);
        assert!(rows.push_object(row(1)));
        assert!(!rows.push_object(row(2)));

        let answer = RowWriter::default().finish();
        assert_eq!(answer, RowsAnswer::empty());
    }
}
