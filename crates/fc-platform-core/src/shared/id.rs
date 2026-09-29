//! Typed entity ids: `Id<ClientKind>` is a `clt_…` id, `Id<PrincipalKind>` a
//! `prn_…` one, and the compiler refuses to pass one where the other belongs.
//!
//! An id is the string it always was, on the wire (`serde` transparent), in
//! the database (`text`/`varchar`, arrays included) and in the OpenAPI
//! document (`string`). What changes is that the only ways to get one are
//! [`Id::generate`] and [`Id::parse`], and `parse` checks the TSID prefix.

use std::cmp::Ordering;
use std::error::Error;
use std::fmt::{self, Debug, Display, Formatter};
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::str::FromStr;

use serde::de::{self, Deserialize, Deserializer};
use serde::{Serialize, Serializer};
use sqlx::encode::IsNull;
use sqlx::error::BoxDynError;
use sqlx::postgres::{PgArgumentBuffer, PgHasArrayType, PgTypeInfo, PgValueRef};
use sqlx::{Decode, Encode, Postgres, Type};
use utoipa::openapi::schema::{Schema, Type as SchemaType};
use utoipa::openapi::{ObjectBuilder, RefOr};
use utoipa::{PartialSchema, ToSchema};

use crate::shared::enum_str::corrupt_value;
use crate::shared::error::PlatformError;
use crate::shared::tsid::{self, EntityType};

/// Marks the entity an [`Id`] identifies. Implemented by empty marker types.
pub trait IdKind {
    const ENTITY: EntityType;
}

/// A `{prefix}_{tsid}` id of the entity `K`.
pub struct Id<K>(String, PhantomData<fn() -> K>);

/// A string that is not an id of the kind it was parsed as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidId {
    pub expected_prefix: &'static str,
    pub value: String,
}

impl Display for InvalidId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} is not a {}_… id", self.value, self.expected_prefix)
    }
}

impl Error for InvalidId {}

impl<K: IdKind> Id<K> {
    /// The 3-letter prefix ids of this kind carry.
    pub fn prefix() -> &'static str {
        K::ENTITY.prefix()
    }

    /// A new id.
    pub fn generate() -> Self {
        Self(tsid::generate(K::ENTITY), PhantomData)
    }

    /// `value` if it is `{prefix}_{something}` for this kind.
    pub fn parse(value: impl Into<String>) -> Result<Self, InvalidId> {
        let value = value.into();
        let prefix = K::ENTITY.prefix();
        match value.strip_prefix(prefix).and_then(|r| r.strip_prefix('_')) {
            Some(rest) if !rest.is_empty() => Ok(Self(value, PhantomData)),
            _ => Err(InvalidId {
                expected_prefix: prefix,
                value,
            }),
        }
    }
}

/// [`Id::parse`] for a column of a stored row: a value of the wrong kind means
/// the row is corrupt (or predates the prefixed ids), which is a loud read error
/// naming the table, column, value and row id, like `enum_str::decode`.
pub fn decode_id<K: IdKind>(
    value: impl Into<String>,
    table: &str,
    column: &str,
    row_id: &str,
) -> Result<Id<K>, PlatformError> {
    let value = value.into();
    Id::parse(value.clone()).map_err(|_| corrupt_value(table, column, &value, row_id))
}

impl<K> Id<K> {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

// The std traits are written out: deriving them would demand `K: Clone`,
// `K: Eq`, … of a marker that is never instantiated.

impl<K> Clone for Id<K> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), PhantomData)
    }
}

impl<K> Debug for Id<K> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Debug::fmt(&self.0, f)
    }
}

impl<K> Display for Id<K> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<K> PartialEq for Id<K> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<K> Eq for Id<K> {}

impl<K> Hash for Id<K> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl<K> PartialOrd for Id<K> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<K> Ord for Id<K> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

impl<K> AsRef<str> for Id<K> {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl<K: IdKind> FromStr for Id<K> {
    type Err = InvalidId;
    fn from_str(s: &str) -> Result<Self, InvalidId> {
        Self::parse(s)
    }
}

impl<K> Serialize for Id<K> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de, K: IdKind> Deserialize<'de> for Id<K> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = String::deserialize(d)?;
        Self::parse(value).map_err(de::Error::custom)
    }
}

// The database column is text/varchar; an id binds and reads as one.

impl<K> Type<Postgres> for Id<K> {
    fn type_info() -> PgTypeInfo {
        <String as Type<Postgres>>::type_info()
    }

    fn compatible(ty: &PgTypeInfo) -> bool {
        <String as Type<Postgres>>::compatible(ty)
    }
}

impl<K> PgHasArrayType for Id<K> {
    fn array_type_info() -> PgTypeInfo {
        <String as PgHasArrayType>::array_type_info()
    }

    fn array_compatible(ty: &PgTypeInfo) -> bool {
        <String as PgHasArrayType>::array_compatible(ty)
    }
}

impl<'q, K> Encode<'q, Postgres> for Id<K> {
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
        <&str as Encode<Postgres>>::encode_by_ref(&self.0.as_str(), buf)
    }
}

impl<'r, K: IdKind> Decode<'r, Postgres> for Id<K> {
    fn decode(value: PgValueRef<'r>) -> Result<Self, BoxDynError> {
        let s = <String as Decode<Postgres>>::decode(value)?;
        Ok(Self::parse(s)?)
    }
}

impl<K> PartialSchema for Id<K> {
    fn schema() -> RefOr<Schema> {
        ObjectBuilder::new().schema_type(SchemaType::String).into()
    }
}

impl<K> ToSchema for Id<K> {}

/// Declares the marker and alias for one id: `id_kind!(ClientKind, ClientId, Client)`.
macro_rules! id_kind {
    ($kind:ident, $alias:ident, $entity:ident) => {
        #[derive(Debug, Clone, Copy)]
        pub enum $kind {}
        impl IdKind for $kind {
            const ENTITY: EntityType = EntityType::$entity;
        }
        pub type $alias = Id<$kind>;
    };
}

id_kind!(ClientKind, ClientId, Client);
id_kind!(PrincipalKind, PrincipalId, Principal);
id_kind!(ServiceAccountKind, ServiceAccountId, ServiceAccount);

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgArguments;
    use sqlx::query::Query;
    use std::collections::HashSet;

    #[test]
    fn generate_carries_the_prefix_and_parses_back() {
        let id = ClientId::generate();
        assert!(id.as_str().starts_with("clt_"));
        assert_eq!(ClientId::parse(id.as_str()).unwrap(), id);
    }

    #[test]
    fn parse_refuses_another_kinds_prefix_and_junk() {
        let prn = PrincipalId::generate();
        assert!(ClientId::parse(prn.as_str()).is_err());
        assert!(ClientId::parse("").is_err());
        assert!(ClientId::parse("clt_").is_err());
        assert!(ClientId::parse("cltx_abc").is_err());
        assert!(ClientId::parse("0HZXEQ5Y8JY5Z").is_err());
    }

    #[test]
    fn std_traits_work_without_bounds_on_the_marker() {
        let a = ClientId::generate();
        let b = a.clone();
        assert_eq!(a, b);
        let mut set = HashSet::new();
        set.insert(a.clone());
        assert!(set.contains(&b));
        assert_eq!(a.to_string(), a.as_str());
        assert!(a.cmp(&b) == Ordering::Equal);
    }

    #[test]
    fn serde_is_a_plain_string_and_deserialize_checks_the_prefix() {
        let id = ClientId::parse("clt_0HZXEQ5Y8JY5Z").unwrap();
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"clt_0HZXEQ5Y8JY5Z\"");
        let back: ClientId = serde_json::from_str("\"clt_0HZXEQ5Y8JY5Z\"").unwrap();
        assert_eq!(back, id);
        assert!(serde_json::from_str::<ClientId>("\"prn_0HZXEQ5Y8JY5Z\"").is_err());
    }

    #[test]
    fn openapi_schema_is_a_string() {
        let json = serde_json::to_value(ClientId::schema()).unwrap();
        assert_eq!(json["type"], "string");
    }

    /// Compile-time: an id binds as a scalar and as an array.
    #[allow(dead_code)]
    fn binds<'q>(
        q: Query<'q, Postgres, PgArguments>,
        one: &'q ClientId,
        many: &'q Vec<ClientId>,
    ) -> Query<'q, Postgres, PgArguments> {
        q.bind(one).bind(many)
    }
}
