use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Configuration for an action
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ActionConfig {
    /// Name of the action
    pub name: String,
    /// Path to the action file
    pub file: PathBuf,
}
