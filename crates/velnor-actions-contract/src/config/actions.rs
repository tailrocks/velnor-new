// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const OVERRIDABLE_ACTIONS: [&str; 7] = [
    "jdx/mise-action",
    "actions/checkout",
    "actions/download-artifact",
    "actions/upload-artifact",
    "actions/cache/restore",
    "actions/cache/save",
    "jdx/mr-boxington-action",
];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionsConfig {
    #[serde(default)]
    pub overrides: BTreeMap<String, ActionPinOverride>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionPinOverride {
    pub sha: String,

    pub version: String,
}

impl ActionsConfig {
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        for (action, pin) in &self.overrides {
            let key_path = format!("actions.overrides.{action}");
            if !OVERRIDABLE_ACTIONS.contains(&action.as_str()) {
                return Err(ContractError::config(file, key_path, "unknown_action"));
            }
            if !is_full_sha(&pin.sha) {
                return Err(ContractError::config(
                    file,
                    key_path,
                    "ref_must_be_full_sha",
                ));
            }
            if !is_version_tag(&pin.version) {
                return Err(ContractError::config(
                    file,
                    key_path,
                    format!("invalid_version:{}", pin.version),
                ));
            }
        }
        Ok(())
    }
}

fn is_full_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

fn is_version_tag(value: &str) -> bool {
    let Some(number) = value.strip_prefix('v') else {
        return false;
    };
    let parts: Vec<&str> = number.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}
