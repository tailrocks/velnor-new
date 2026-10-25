use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const OVERRIDABLE_TOOLS: &[&str] = &[
    "cargo-nextest",
    "gh",
    "jactionlint",
    "mr-boxington",
    "nextest",
    "opentofu",
    "rust",
    "shellcheck",
];

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ToolsConfig {
    pub overrides: BTreeMap<String, String>,
}

impl ToolsConfig {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.overrides.is_empty()
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        if matches!(name, "nextest" | "cargo-nextest") {
            self.overrides
                .get("cargo-nextest")
                .or_else(|| self.overrides.get("nextest"))
                .map(String::as_str)
        } else {
            self.overrides.get(name).map(String::as_str)
        }
    }

    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if self.overrides.contains_key("nextest") && self.overrides.contains_key("cargo-nextest") {
            return Err(ContractError::config(
                file,
                "tools.cargo-nextest",
                "duplicate_nextest_tool_override",
            ));
        }
        for (name, version) in &self.overrides {
            if !OVERRIDABLE_TOOLS.contains(&name.as_str()) {
                return Err(ContractError::config(
                    file,
                    format!("tools.{name}"),
                    format!("unknown_tool_override:{name}"),
                ));
            }
            if !is_exact_semver(version) {
                return Err(ContractError::config(
                    file,
                    format!("tools.{name}"),
                    format!("invalid_tool_version:{version}"),
                ));
            }
        }
        Ok(())
    }
}

#[must_use]
pub fn is_exact_semver(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !(part.is_empty() || part.len() > 1 && part.starts_with('0'))
                && part.bytes().all(|byte| byte.is_ascii_digit())
        })
}
