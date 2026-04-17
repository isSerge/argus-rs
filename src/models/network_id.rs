//! Network identifier newtype.

use serde::{Deserialize, Serialize};

/// Domain newtype representing a network identifier (e.g., `"mainnet"`,
/// `"ethereum"`).
///
/// Use explicit construction (`NetworkId::from("mainnet")`) in production code.
/// `Default` returns `"testnet"` and exists solely for test convenience via
/// struct-update syntax (`..Default::default()`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[sqlx(transparent)]
pub struct NetworkId(String);

impl NetworkId {
    /// Returns the network ID as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for NetworkId {
    fn default() -> Self {
        NetworkId("testnet".to_string())
    }
}

impl std::fmt::Display for NetworkId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl From<&str> for NetworkId {
    fn from(s: &str) -> Self {
        NetworkId(s.to_owned())
    }
}

impl From<String> for NetworkId {
    fn from(s: String) -> Self {
        NetworkId(s)
    }
}

impl AsRef<str> for NetworkId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}
