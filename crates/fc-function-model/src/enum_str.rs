//! String-backed enums, the same shape as the platform's
//! `shared::enum_str::str_enum!`: one canonical spelling per variant, strict
//! parsing, and [`UnknownEnumValue`](crate::UnknownEnumValue) (the platform
//! re-exports that type, so its stored-row decoders take these enums as
//! they take its own).

/// Implements `as_str`, `ALL`, `FromStr` and `Display` for a fieldless enum.
macro_rules! str_enum {
    (
        $ty:ident, $kind:literal,
        { $( $variant:ident => $s:literal ),+ $(,)? }
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
            type Err = $crate::UnknownEnumValue;

            fn from_str(s: &str) -> ::std::result::Result<Self, Self::Err> {
                match s {
                    $( $s => Ok(Self::$variant), )+
                    _ => Err($crate::UnknownEnumValue::new($kind, s, &[$($s),+])),
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
