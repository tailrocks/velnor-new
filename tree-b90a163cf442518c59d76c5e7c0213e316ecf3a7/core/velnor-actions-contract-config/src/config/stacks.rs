//! Stack selection and per-stack options.
use super::release::RustReleaseConfig;
use super::rust_policy::RustPolicyConfig;
use super::tofu::TofuStackConfig;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use velnor_actions_contract::errors::ContractError;
use velnor_actions_contract::ids::is_component_byte;
use velnor_actions_contract::stack::Stack;

/// Stack selection and per-stack options.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StacksConfig {
    /// Sorted, duplicate-free exact registered stack IDs to ignore.
    #[serde(default)]
    pub ignore: Vec<String>,
    /// Rust stack options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rust: Option<RustStackConfig>,
    /// Tofu stack options; absent means no tofu validation roots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tofu: Option<TofuStackConfig>,
}

/// Declared compile driver (`[stacks.rust] compile_driver`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclaredCompileDriver {
    /// Plain Cargo compilation.
    Cargo,
    /// MBX compilation.
    Mbx,
}

/// Declared test runner (`[stacks.rust] test_runner`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclaredTestRunner {
    /// Plain `cargo test`.
    CargoTest,
    /// Nextest execution.
    CargoNextest,
}

/// Rust stack options.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustStackConfig {
    /// Rust task configuration variants.
    #[serde(default = "default_configurations")]
    pub configurations: Vec<RustConfiguration>,
    /// Sticky declared compile driver; conflicts with durable evidence fail closed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compile_driver: Option<DeclaredCompileDriver>,
    /// Sticky declared test runner; conflicts with durable evidence fail closed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_runner: Option<DeclaredTestRunner>,
    /// Ignored test execution mode ("all", "only", "ignored-only", "default").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_ignored: Option<String>,
    /// Rust release policy (`[stacks.rust.release]`); disabled by default.
    #[serde(default)]
    pub release: RustReleaseConfig,
    /// Shared Rust policy identity (`[stacks.rust.policy]`); absent means
    /// no policy lane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<RustPolicyConfig>,
}

/// Documented default: one `default` configuration variant list.
fn default_configurations() -> Vec<RustConfiguration> {
    vec![RustConfiguration {
        name: "default".to_owned(),
        features: vec!["default".to_owned()],
        target: "host".to_owned(),
    }]
}

/// One Rust task configuration variant.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustConfiguration {
    /// Configuration name.
    pub name: String,
    /// Cargo features for this variant.
    #[serde(default)]
    pub features: Vec<String>,
    /// Target triple or `host`.
    pub target: String,
}

/// True for a render-safe Rust target: `host` or a lowercase triple over
/// `[a-z0-9_.+-]` that never starts with `-`.
///
/// Targets flow into `--target` argv and quoted `run:` lines; the
/// allowlist admits nothing the shell or `${{ }}` could evaluate, and
/// flag-shaped values fail closed at config load.
#[must_use]
pub fn is_valid_rust_target(target: &str) -> bool {
    if target == "host" {
        return true;
    }
    !target.is_empty()
        && !target.starts_with('-')
        && target
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'+' | b'-'))
}

/// True for a render-safe Cargo feature name.
///
/// Admits Cargo feature syntax (names plus `dep:`, weak `?`, and `/`
/// qualifiers) and nothing the shell or `${{ }}` could evaluate.
/// Unknown names still fail closed later against declared features.
#[must_use]
pub fn is_valid_feature_name(feature: &str) -> bool {
    !feature.is_empty()
        && feature.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'+' | b'/' | b':' | b'?')
        })
}

impl RustStackConfig {
    /// Documented default: one `default` configuration, no declarations,
    /// release disabled.
    #[must_use]
    pub fn default_config() -> Self {
        Self {
            configurations: default_configurations(),
            compile_driver: None,
            test_runner: None,
            run_ignored: None,
            release: RustReleaseConfig::default(),
            policy: None,
        }
    }
}

impl StacksConfig {
    /// Validate ignore list and Rust configurations.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        let mut sorted = self.ignore.clone();
        sorted.sort();
        if sorted != self.ignore {
            return Err(ContractError::config(
                file,
                "stacks.ignore",
                "must_be_sorted",
            ));
        }
        let unique: BTreeSet<&str> = self.ignore.iter().map(String::as_str).collect();
        if unique.len() != self.ignore.len() {
            return Err(ContractError::config(
                file,
                "stacks.ignore",
                "duplicate_stack_id",
            ));
        }
        for id in &self.ignore {
            let stack = Stack::from_id(id).ok_or_else(|| {
                ContractError::config(file, "stacks.ignore", format!("unknown_stack_id:{id}"))
            })?;
            if !stack.is_ignorable() {
                return Err(ContractError::config(
                    file,
                    "stacks.ignore",
                    format!("stack_not_ignorable:{id}"),
                ));
            }
        }
        if let Some(rust) = &self.rust {
            rust.validate(file)?;
        }
        if let Some(tofu) = &self.tofu {
            tofu.validate(file)?;
        }
        Ok(())
    }
}

impl RustStackConfig {
    /// Validate Rust configurations (unique nonempty names).
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if self.configurations.is_empty() {
            return Err(ContractError::config(
                file,
                "stacks.rust.configurations",
                "empty_configurations",
            ));
        }
        let mut names = BTreeSet::new();
        for config in &self.configurations {
            if config.name.trim().is_empty() {
                return Err(ContractError::config(
                    file,
                    "stacks.rust.configurations.name",
                    "empty_name",
                ));
            }
            if !config.name.bytes().all(is_component_byte) {
                return Err(ContractError::config(
                    file,
                    "stacks.rust.configurations.name",
                    format!("bad_component:{}", config.name),
                ));
            }
            if !names.insert(config.name.as_str()) {
                return Err(ContractError::config(
                    file,
                    "stacks.rust.configurations",
                    format!("duplicate_configuration:{}", config.name),
                ));
            }
            if !is_valid_rust_target(&config.target) {
                return Err(ContractError::config(
                    file,
                    "stacks.rust.configurations.target",
                    format!("bad_target:{}", config.target),
                ));
            }
            for feature in &config.features {
                if !is_valid_feature_name(feature) {
                    return Err(ContractError::config(
                        file,
                        "stacks.rust.configurations.features",
                        format!("bad_feature:{feature}"),
                    ));
                }
            }
        }
        if let Some(mode) = &self.run_ignored
            && !matches!(mode.as_str(), "all" | "only" | "ignored-only" | "default")
        {
            return Err(ContractError::config(
                file,
                "stacks.rust.run_ignored",
                format!("bad_run_ignored:{mode}"),
            ));
        }
        self.release.validate(file)?;
        if let Some(policy) = &self.policy {
            policy.validate(file)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
