use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

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

        // sqlx: delegate to the inner Uuid (which has Type/Encode/Decode
        // via the sqlx "uuid" feature).
        #[cfg(feature = "sqlx")]
        impl sqlx::Type<sqlx::Postgres> for $name {
            fn type_info() -> sqlx::postgres::PgTypeInfo {
                <Uuid as sqlx::Type<sqlx::Postgres>>::type_info()
            }

            fn compatible(ty: &sqlx::postgres::PgTypeInfo) -> bool {
                <Uuid as sqlx::Type<sqlx::Postgres>>::compatible(ty)
            }
        }

        #[cfg(feature = "sqlx")]
        impl sqlx::postgres::PgHasArrayType for $name {
            fn array_type_info() -> sqlx::postgres::PgTypeInfo {
                <Uuid as sqlx::postgres::PgHasArrayType>::array_type_info()
            }
        }

        #[cfg(feature = "sqlx")]
        impl<'r> sqlx::Decode<'r, sqlx::Postgres> for $name {
            fn decode(
                value: sqlx::postgres::PgValueRef<'r>,
            ) -> Result<Self, sqlx::error::BoxDynError> {
                <Uuid as sqlx::Decode<'r, sqlx::Postgres>>::decode(value).map(Self)
            }
        }

        #[cfg(feature = "sqlx")]
        impl sqlx::Encode<'_, sqlx::Postgres> for $name {
            fn encode_by_ref(
                &self,
                buf: &mut sqlx::postgres::PgArgumentBuffer,
            ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
                <Uuid as sqlx::Encode<'_, sqlx::Postgres>>::encode_by_ref(&self.0, buf)
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
