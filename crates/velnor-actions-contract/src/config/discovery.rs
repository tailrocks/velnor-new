//! Discovery exclusions (repo-relative POSIX globs).
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

/// Discovery exclusions (repo-relative POSIX globs).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryConfig {
    /// Exclusion globs applied before detectors.
    #[serde(default)]
    pub exclude: Vec<String>,
}

impl DiscoveryConfig {
    /// Validate exclusion globs (relative, no traversal, well-formed).
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        for pattern in &self.exclude {
            if pattern.is_empty()
                || pattern.starts_with('/')
                || pattern.contains('\\')
                || pattern.split('/').any(|seg| seg == "..")
                || !pattern.bytes().all(is_glob_byte)
            {
                return Err(ContractError::config(
                    file,
                    "discovery.exclude",
                    format!("malformed_pattern:{pattern}"),
                ));
            }
        }
        Ok(())
    }
}

/// Bytes allowed in discovery globs (paths plus `*?[]{}!` classes).
fn is_glob_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'/' | b'.'
                | b'-'
                | b'_'
                | b'*'
                | b'?'
                | b'['
                | b']'
                | b'{'
                | b'}'
                | b'!'
                | b'+'
                | b'@'
        )
}
