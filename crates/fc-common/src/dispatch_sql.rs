//! `DispatchStatus` and `DispatchMode` as the TEXT they are stored as, for
//! binding: `.bind(job.status)` writes `as_str`, so the spelling is written in
//! one place.
//!
//! Binding only. Reading either takes a decision a plain `Decode` cannot make
//! (the status is read strictly, with the legacy spellings, and a corrupt row
//! named: `fc-platform-messaging`'s `parse_dispatch_status`; the mode is read
//! leniently, ruling X-01), so neither implements `Decode`.

use sqlx::encode::IsNull;
use sqlx::error::BoxDynError;
use sqlx::postgres::{PgArgumentBuffer, PgHasArrayType, PgTypeInfo};
use sqlx::{Encode, Postgres, Type};

use crate::{DispatchMode, DispatchStatus};

macro_rules! text_bind {
    ($ty:ty) => {
        impl Type<Postgres> for $ty {
            fn type_info() -> PgTypeInfo {
                <String as Type<Postgres>>::type_info()
            }

            fn compatible(ty: &PgTypeInfo) -> bool {
                <String as Type<Postgres>>::compatible(ty)
            }
        }

        impl PgHasArrayType for $ty {
            fn array_type_info() -> PgTypeInfo {
                <String as PgHasArrayType>::array_type_info()
            }

            fn array_compatible(ty: &PgTypeInfo) -> bool {
                <String as PgHasArrayType>::array_compatible(ty)
            }
        }

        impl Encode<'_, Postgres> for $ty {
            fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
                <&str as Encode<Postgres>>::encode_by_ref(&self.as_str(), buf)
            }
        }
    };
}

text_bind!(DispatchStatus);
text_bind!(DispatchMode);
