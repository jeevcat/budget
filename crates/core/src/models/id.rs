use std::fmt;

use serde::{Deserialize, Serialize};
use sqlx::postgres::types::Oid;
use sqlx::postgres::{PgArgumentBuffer, PgHasArrayType, PgTypeInfo, PgValueRef};
use uuid::Uuid;

/// `PostgreSQL` UUID type OID.
const PG_UUID_OID: Oid = Oid(2950);
/// `PostgreSQL` UUID array type OID.
const PG_UUID_ARRAY_OID: Oid = Oid(2951);

macro_rules! define_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(Uuid);

        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            #[must_use]
            pub const fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            #[must_use]
            pub const fn as_uuid(&self) -> &Uuid {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl From<Uuid> for $name {
            fn from(uuid: Uuid) -> Self {
                Self(uuid)
            }
        }

        impl From<$name> for Uuid {
            fn from(id: $name) -> Self {
                id.0
            }
        }

        // sqlx integration: encode/decode as native PostgreSQL UUID (16 bytes)
        impl sqlx::Type<sqlx::Postgres> for $name {
            fn type_info() -> PgTypeInfo {
                PgTypeInfo::with_oid(PG_UUID_OID)
            }

            fn compatible(ty: &PgTypeInfo) -> bool {
                *ty == Self::type_info()
            }
        }

        impl PgHasArrayType for $name {
            fn array_type_info() -> PgTypeInfo {
                PgTypeInfo::with_oid(PG_UUID_ARRAY_OID)
            }
        }

        impl<'r> sqlx::Decode<'r, sqlx::Postgres> for $name {
            fn decode(value: PgValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
                let bytes = <&[u8] as sqlx::Decode<'r, sqlx::Postgres>>::decode(value)?;
                let uuid = Uuid::from_slice(bytes)?;
                Ok(Self(uuid))
            }
        }

        impl sqlx::Encode<'_, sqlx::Postgres> for $name {
            fn encode_by_ref(
                &self,
                buf: &mut PgArgumentBuffer,
            ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
                buf.extend_from_slice(self.0.as_bytes());
                Ok(sqlx::encode::IsNull::No)
            }
        }
    };
}

define_id!(AccountId);
define_id!(TransactionId);
define_id!(CategoryId);
define_id!(RuleId);
define_id!(BudgetMonthId);
define_id!(ConnectionId);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_display_roundtrip() {
        let id = AccountId::new();
        let s = id.to_string();
        let uuid: Uuid = id.into();
        assert_eq!(s, uuid.to_string());
    }

    #[test]
    fn id_equality() {
        let uuid = Uuid::new_v4();
        let a = TransactionId::from_uuid(uuid);
        let b = TransactionId::from_uuid(uuid);
        assert_eq!(a, b);
    }
}
