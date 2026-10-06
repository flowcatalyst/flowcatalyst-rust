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

/// A string that names no variant of the enum it was parsed as. Defined in
/// `fc-function-model`, whose enums (`Runtime`, `HttpMethod`, …) the stored-row
/// decoders below take as they take this crate's own.
pub use fc_function_model::UnknownEnumValue;
#[cfg(any(test, feature = "test-support"))]
use serde::de::DeserializeOwned;
use sqlx::error::BoxDynError;
use sqlx::postgres::{PgTypeInfo, PgValueRef};
use sqlx::{Decode, Postgres, Type};
#[cfg(any(test, feature = "test-support"))]
use std::fmt;
use std::str::FromStr;

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
    T: FromStr<Err = UnknownEnumValue>,
{
    value
        .map(str::parse)
        .transpose()
        .map_err(PlatformError::from)
}

/// Treats an empty string like an absent one, for optional request fields
/// where `""` has always meant "unspecified".
pub fn non_empty(value: Option<&str>) -> Option<&str> {
    value.filter(|v| !v.is_empty())
}

/// Decode an enum column of a stored row. An unknown value means the row is
/// corrupt (or was written by something that doesn't share this vocabulary):
/// it is logged and surfaced as an internal error naming where it came from,
/// rather than being coerced to a default.
pub fn decode<T>(value: &str, table: &str, column: &str, row_id: &str) -> Result<T, PlatformError>
where
    T: FromStr<Err = UnknownEnumValue>,
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
    T: FromStr<Err = UnknownEnumValue>,
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

/// A text column of a stored row that holds a `T`, read without judging it.
///
/// A plain `sqlx::Decode` for an enum can only say "column `status` holds an
/// unknown value"; it cannot name the table or the row, and that is what makes
/// a corrupt row findable. So a row struct holds `Stored<T>`: reading it never
/// fails on an unknown value, and [`Stored::decode`] (called in the row's
/// `TryFrom`, where the table, column and row id are known) turns it into the
/// enum or the same loud error [`decode`] gives. The field is typed, so the
/// enum a column maps to is fixed by the row struct, not by what a later
/// `decode` call happens to infer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored<T>(Result<T, String>);

impl<T: FromStr<Err = UnknownEnumValue>> Stored<T> {
    /// What a column holding `text` reads as; for a test that needs a row
    /// with a spelling no variant has.
    pub fn of_text(text: &str) -> Self {
        Self(text.parse().map_err(|_| text.to_string()))
    }
}

impl<T> Stored<T> {
    /// The enum, or the text the column held when no variant spells it, for a
    /// reader that words its own error.
    pub fn into_result(self) -> Result<T, String> {
        self.0
    }

    /// The enum if the column held a known spelling, `None` if not: for the
    /// few reads that have always treated an unrecognised value as "no
    /// information" rather than as a corrupt row. Prefer [`Stored::decode`].
    pub fn known(self) -> Option<T> {
        self.0.ok()
    }

    /// The enum, or the loud error naming `table.column` of row `row_id`.
    pub fn decode(self, table: &str, column: &str, row_id: &str) -> Result<T, PlatformError> {
        self.0
            .map_err(|value| corrupt_value(table, column, &value, row_id))
    }
}

/// [`Stored::decode`] for a nullable column.
pub fn decode_stored_opt<T>(
    value: Option<Stored<T>>,
    table: &str,
    column: &str,
    row_id: &str,
) -> Result<Option<T>, PlatformError> {
    value.map(|v| v.decode(table, column, row_id)).transpose()
}

impl<T> From<T> for Stored<T> {
    fn from(value: T) -> Self {
        Self(Ok(value))
    }
}

impl<T> Type<Postgres> for Stored<T> {
    fn type_info() -> PgTypeInfo {
        <String as Type<Postgres>>::type_info()
    }

    fn compatible(ty: &PgTypeInfo) -> bool {
        <String as Type<Postgres>>::compatible(ty)
    }
}

impl<'r, T: FromStr<Err = UnknownEnumValue>> Decode<'r, Postgres> for Stored<T> {
    fn decode(value: PgValueRef<'r>) -> Result<Self, BoxDynError> {
        let raw = <String as Decode<Postgres>>::decode(value)?;
        Ok(Self(raw.parse().map_err(|_| raw)))
    }
}

/// `sqlx`, for [`str_enum!`] to name from a crate that does not depend on it.
#[doc(hidden)]
pub use sqlx as __sqlx;

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
#[doc(hidden)]
#[macro_export]
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

        // The column is text; the variant's spelling is `as_str`, the one
        // place it is written. Binding one is the point; reading one with
        // this `Decode` is strict but cannot name the row, so row structs
        // hold `Stored<Self>` instead (see `Stored`).
        impl $crate::shared::enum_str::__sqlx::Type<$crate::shared::enum_str::__sqlx::Postgres>
            for $ty
        {
            fn type_info() -> $crate::shared::enum_str::__sqlx::postgres::PgTypeInfo {
                <::std::string::String as $crate::shared::enum_str::__sqlx::Type<
                    $crate::shared::enum_str::__sqlx::Postgres,
                >>::type_info()
            }

            fn compatible(
                ty: &$crate::shared::enum_str::__sqlx::postgres::PgTypeInfo,
            ) -> bool {
                <::std::string::String as $crate::shared::enum_str::__sqlx::Type<
                    $crate::shared::enum_str::__sqlx::Postgres,
                >>::compatible(ty)
            }
        }

        impl $crate::shared::enum_str::__sqlx::postgres::PgHasArrayType for $ty {
            fn array_type_info() -> $crate::shared::enum_str::__sqlx::postgres::PgTypeInfo {
                <::std::string::String as $crate::shared::enum_str::__sqlx::postgres::PgHasArrayType>::array_type_info()
            }

            fn array_compatible(
                ty: &$crate::shared::enum_str::__sqlx::postgres::PgTypeInfo,
            ) -> bool {
                <::std::string::String as $crate::shared::enum_str::__sqlx::postgres::PgHasArrayType>::array_compatible(ty)
            }
        }

        impl<'q> $crate::shared::enum_str::__sqlx::Encode<'q, $crate::shared::enum_str::__sqlx::Postgres>
            for $ty
        {
            fn encode_by_ref(
                &self,
                buf: &mut $crate::shared::enum_str::__sqlx::postgres::PgArgumentBuffer,
            ) -> ::std::result::Result<
                $crate::shared::enum_str::__sqlx::encode::IsNull,
                $crate::shared::enum_str::__sqlx::error::BoxDynError,
            > {
                <&str as $crate::shared::enum_str::__sqlx::Encode<
                    $crate::shared::enum_str::__sqlx::Postgres,
                >>::encode_by_ref(&self.as_str(), buf)
            }
        }

        impl<'r> $crate::shared::enum_str::__sqlx::Decode<'r, $crate::shared::enum_str::__sqlx::Postgres>
            for $ty
        {
            fn decode(
                value: $crate::shared::enum_str::__sqlx::postgres::PgValueRef<'r>,
            ) -> ::std::result::Result<Self, $crate::shared::enum_str::__sqlx::error::BoxDynError> {
                let s = <::std::string::String as $crate::shared::enum_str::__sqlx::Decode<
                    $crate::shared::enum_str::__sqlx::Postgres,
                >>::decode(value)?;
                Ok(s.parse::<Self>()?)
            }
        }
    };
}
pub use crate::str_enum;

/// Asserts, for every variant, that `as_str` round-trips through `FromStr` and
/// matches the serde spelling, so the hand-listed strings and the serde
/// derive can't drift apart. Call as `assert_str_enum(X::ALL, X::as_str)`.
#[cfg(any(test, feature = "test-support"))]
#[expect(
    clippy::unwrap_used,
    reason = "test-support assertion helper: an unserialisable variant is the failure it reports"
)]
pub fn assert_str_enum<T>(all: &[T], as_str: fn(&T) -> &'static str)
where
    T: PartialEq
        + fmt::Debug
        + serde::Serialize
        + DeserializeOwned
        + FromStr<Err = UnknownEnumValue>,
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
    fn a_stored_column_keeps_the_row_diagnostic() {
        assert_eq!(
            Stored::<Sample>::of_text("TWOWORDS")
                .decode("t_things", "kind", "thg_1")
                .unwrap(),
            Sample::TwoWords
        );
        let err = Stored::<Sample>::of_text("two_words")
            .decode("t_things", "kind", "thg_1")
            .unwrap_err()
            .to_string();
        for part in ["t_things.kind", "thg_1", "\"two_words\""] {
            assert!(err.contains(part), "{err} should mention {part}");
        }
        assert!(Stored::<Sample>::of_text("two_words").known().is_none());
        assert_eq!(Stored::from(Sample::One).known(), Some(Sample::One));
        assert_eq!(
            decode_stored_opt::<Sample>(None, "t", "c", "r").unwrap(),
            None
        );
    }

    /// An enum binds as a scalar and as an array (compile-time).
    #[test]
    fn an_enum_binds_as_text() {
        let query = sqlx::query("SELECT $1, $2")
            .bind(Sample::One)
            .bind(vec![Sample::TwoWords]);
        assert!(sqlx::Execute::sql(&query).contains("$2"));
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
