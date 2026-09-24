//! String-backed domain enums.
//!
//! Every enum that crosses the wire or sits in a text column has exactly one
//! canonical spelling per variant (`as_str`), and parsing is strict: an
//! unknown value is an error, never a silent default (owner ruling X-06).
//! Request input that fails to parse becomes a 400; a stored row that fails
//! to parse becomes a loud read error naming the table, column, value and
//! row id (see [`decode`]).
//!
//! The one exemption is dispatch mode (ruling X-01), which stays lenient:
//! see `dispatch_job::entity::parse_dispatch_mode`.

use crate::shared::error::PlatformError;

/// A string that names no variant of the enum it was parsed as.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown {kind} {value:?} (expected one of: {})", expected.join(", "))]
pub struct UnknownEnumValue {
    /// What was being parsed, e.g. "client status".
    pub kind: &'static str,
    /// The rejected input.
    pub value: String,
    /// The canonical spellings that would have been accepted.
    pub expected: &'static [&'static str],
}

impl UnknownEnumValue {
    pub fn new(
        kind: &'static str,
        value: impl Into<String>,
        expected: &'static [&'static str],
    ) -> Self {
        Self {
            kind,
            value: value.into(),
            expected,
        }
    }
}

/// Request input: an unknown enum value is the caller's mistake, so it maps to
/// the standard 400 validation response.
impl From<UnknownEnumValue> for PlatformError {
    fn from(e: UnknownEnumValue) -> Self {
        PlatformError::validation(e.to_string())
    }
}

/// Parse an optional request field. `None` stays `None`; a present but unknown
/// value is a 400.
pub fn parse_opt<T>(value: Option<&str>) -> Result<Option<T>, PlatformError>
where
    T: std::str::FromStr<Err = UnknownEnumValue>,
{
    value
        .map(str::parse)
        .transpose()
        .map_err(PlatformError::from)
}

/// Decode an enum column of a stored row. An unknown value means the row is
/// corrupt (or was written by something that doesn't share this vocabulary):
/// it is logged and surfaced as an internal error naming where it came from,
/// rather than being coerced to a default.
pub fn decode<T>(value: &str, table: &str, column: &str, row_id: &str) -> Result<T, PlatformError>
where
    T: std::str::FromStr<Err = UnknownEnumValue>,
{
    value
        .parse()
        .map_err(|_| corrupt_value(table, column, value, row_id))
}

/// [`decode`] for a nullable column.
pub fn decode_opt<T>(
    value: Option<&str>,
    table: &str,
    column: &str,
    row_id: &str,
) -> Result<Option<T>, PlatformError>
where
    T: std::str::FromStr<Err = UnknownEnumValue>,
{
    value.map(|v| decode(v, table, column, row_id)).transpose()
}

/// The error for a stored value that doesn't parse. Public so decoders for
/// enums this crate doesn't own (fc-common's) can report the same way.
pub fn corrupt_value(table: &str, column: &str, value: &str, row_id: &str) -> PlatformError {
    tracing::error!(
        table,
        column,
        value,
        row_id,
        "stored row holds an unknown enum value"
    );
    PlatformError::internal(format!(
        "{table}.{column} of row {row_id} holds unknown value {value:?}"
    ))
}

/// Implements `as_str`, `ALL`, `FromStr` and `Display` for a fieldless enum.
///
/// Each variant maps to its canonical spelling, optionally followed by
/// `| "ALIAS"` spellings that are accepted on parse but never produced
/// (legacy values still present in stored rows). Anything else is an
/// [`UnknownEnumValue`].
///
/// ```ignore
/// str_enum!(ClientStatus, "client status", {
///     Active => "ACTIVE",
///     Inactive => "INACTIVE",
/// });
/// ```
macro_rules! str_enum {
    (
        $ty:ident, $kind:literal,
        { $( $variant:ident => $s:literal $( | $alias:literal )* ),+ $(,)? }
    ) => {
        impl $ty {
            /// Every variant, in declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// The canonical wire and storage spelling.
            pub fn as_str(&self) -> &'static str {
                match self {
                    $(Self::$variant => $s,)+
                }
            }
        }

        impl ::std::str::FromStr for $ty {
            type Err = $crate::shared::enum_str::UnknownEnumValue;

            fn from_str(s: &str) -> ::std::result::Result<Self, Self::Err> {
                match s {
                    $( $s $( | $alias )* => Ok(Self::$variant), )+
                    _ => Err($crate::shared::enum_str::UnknownEnumValue::new(
                        $kind,
                        s,
                        &[$($s),+],
                    )),
                }
            }
        }

        impl ::std::fmt::Display for $ty {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}
pub(crate) use str_enum;

/// Asserts, for every variant, that `as_str` round-trips through `FromStr` and
/// matches the serde spelling, so the hand-listed strings and the serde
/// derive can't drift apart. Call as `assert_str_enum(X::ALL, X::as_str)`.
#[cfg(test)]
pub(crate) fn assert_str_enum<T>(all: &[T], as_str: fn(&T) -> &'static str)
where
    T: PartialEq
        + std::fmt::Debug
        + serde::Serialize
        + serde::de::DeserializeOwned
        + std::str::FromStr<Err = UnknownEnumValue>,
{
    for v in all {
        let s = as_str(v);
        assert_eq!(s.parse::<T>().ok().as_ref(), Some(v), "round trip of {s}");
        assert_eq!(
            serde_json::to_value(v).unwrap(),
            serde_json::Value::String(s.to_string()),
            "serde spelling of {s}"
        );
        assert_eq!(
            serde_json::from_value::<T>(serde_json::Value::String(s.to_string()))
                .ok()
                .as_ref(),
            Some(v),
            "serde parse of {s}"
        );
    }
    assert!("not-a-variant".parse::<T>().is_err());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
    enum Sample {
        One,
        TwoWords,
    }
    str_enum!(Sample, "sample", {
        One => "ONE",
        TwoWords => "TWO_WORDS" | "TWOWORDS",
    });

    #[test]
    fn strict_parse_with_aliases() {
        assert_str_enum(Sample::ALL, Sample::as_str);
        assert_eq!("TWOWORDS".parse::<Sample>(), Ok(Sample::TwoWords));
        assert_eq!(Sample::TwoWords.as_str(), "TWO_WORDS");
        let err = "one".parse::<Sample>().unwrap_err();
        assert_eq!(
            err.to_string(),
            "unknown sample \"one\" (expected one of: ONE, TWO_WORDS)"
        );
    }

    #[test]
    fn unknown_request_value_is_a_validation_error() {
        let err: PlatformError = "x".parse::<Sample>().unwrap_err().into();
        assert!(matches!(err, PlatformError::Validation { .. }));
    }

    #[test]
    fn corrupt_stored_value_names_its_origin() {
        let err = decode::<Sample>("BAD", "t_things", "kind", "thg_1").unwrap_err();
        let msg = err.to_string();
        for part in ["t_things.kind", "thg_1", "\"BAD\""] {
            assert!(msg.contains(part), "{msg} should mention {part}");
        }
        assert!(matches!(err, PlatformError::Internal { .. }));
        assert_eq!(decode_opt::<Sample>(None, "t", "c", "r").unwrap(), None);
    }
}
