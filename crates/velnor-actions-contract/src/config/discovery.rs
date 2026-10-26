// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryConfig {
    #[serde(default)]
    pub exclude: Vec<String>,
}

impl DiscoveryConfig {
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
