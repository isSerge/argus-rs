//! Action identifier newtype.

use serde::{Deserialize, Serialize};

/// Domain newtype representing an action identifier.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[sqlx(transparent)]
pub struct ActionId(i64);

impl std::fmt::Display for ActionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<i64> for ActionId {
    fn from(value: i64) -> Self {
        ActionId(value)
    }
}
