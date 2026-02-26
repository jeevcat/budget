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
