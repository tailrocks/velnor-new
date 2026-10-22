//! `[tools]` section: exact tool-version overrides.
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Tools accepting per-project version overrides (`[tools]` keys).
pub const OVERRIDABLE_TOOLS: &[&str] = &[
    "cargo-nextest",
    "gh",
    "jactionlint",
    "mr-boxington",
    "nextest",
    "opentofu",
    "rust",
    "shellcheck",
    "zizmor",
];

/// `[tools]` section: exact `major.minor.patch` version overrides.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ToolsConfig {
    /// Version overrides keyed by tool name.
    pub overrides: BTreeMap<String, String>,
}

impl ToolsConfig {
    /// Whether no tool overrides are declared.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.overrides.is_empty()
    }

    /// Look up a tool override by name (`nextest` and `cargo-nextest` alias).
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

    /// Validate override keys (allowlist), alias consistency, and exact semver shapes.
    /// # Errors
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

/// True for exact `major.minor.patch` semver with no leading zeros, prefixes, or suffixes.
#[must_use]
pub fn is_exact_semver(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !(part.is_empty() || part.len() > 1 && part.starts_with('0'))
                && part.bytes().all(|byte| byte.is_ascii_digit())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RustStackConfig;

    #[test]
    fn exact_semver_accepts_only_three_numeric_parts_without_leading_zeros() {
        for valid in ["0.0.0", "1.98.1", "0.9.148", "10.20.30"] {
            assert!(is_exact_semver(valid), "{valid}");
        }
        for invalid in [
            "",
            "1.98",
            "1.98.1.0",
            "v1.98.1",
            "01.98.1",
            "1.09.1",
            "1.98.01",
            "1.98.1-beta.1",
            "1.98.1+build",
            "stable",
            "1.98.1 ",
        ] {
            assert!(!is_exact_semver(invalid), "{invalid:?}");
        }
    }

    #[test]
    fn tools_config_validates_allowlist_aliases_and_versions() {
        let mut cfg = ToolsConfig::default();
        assert!(cfg.is_empty());
        assert_eq!(cfg.validate("config.toml"), Ok(()));

        cfg.overrides.insert("rust".to_owned(), "1.97.0".to_owned());
        cfg.overrides
            .insert("cargo-nextest".to_owned(), "0.9.140".to_owned());
        assert_eq!(cfg.get("rust"), Some("1.97.0"));
        assert_eq!(cfg.get("nextest"), Some("0.9.140"));
        assert_eq!(cfg.get("cargo-nextest"), Some("0.9.140"));
        assert_eq!(cfg.validate("config.toml"), Ok(()));

        cfg.overrides
            .insert("nextest".to_owned(), "0.9.140".to_owned());
        assert!(
            matches!(cfg.validate("config.toml"), Err(ref err) if err.to_string().contains("duplicate_nextest_tool_override"))
        );
        cfg.overrides
            .insert("nextest".to_owned(), "0.9.141".to_owned());
        assert!(
            matches!(cfg.validate("config.toml"), Err(ref err) if err.to_string().contains("duplicate_nextest_tool_override"))
        );

        let unknown = ToolsConfig {
            overrides: BTreeMap::from([("mise".to_owned(), "2026.10.13".to_owned())]),
        };
        assert!(
            matches!(unknown.validate("config.toml"), Err(ref err) if err.to_string().contains("unknown_tool_override:mise"))
        );

        let bad_ver = ToolsConfig {
            overrides: BTreeMap::from([("gh".to_owned(), "v2.100.0".to_owned())]),
        };
        assert!(
            matches!(bad_ver.validate("config.toml"), Err(ref err) if err.to_string().contains("invalid_tool_version:v2.100.0"))
        );
    }

    #[test]
    fn rust_stack_toolchain_validates_exact_semver() {
        let mut stack = RustStackConfig::default_config();
        stack.toolchain = Some("1.97.0".to_owned());
        assert_eq!(stack.validate("config.toml"), Ok(()));

        for invalid in ["", " 1.97.0", "stable", "v1.97.0", "1.97"] {
            stack.toolchain = Some(invalid.to_owned());
            assert!(
                matches!(stack.validate("config.toml"), Err(ref err) if err.to_string().contains(&format!("invalid_rust_toolchain:{invalid}")))
            );
        }
    }
}
